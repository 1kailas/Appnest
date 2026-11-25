use gio::prelude::*;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// A process identity: the PID plus its start time, so a recycled PID is never mistaken
/// for the process that was originally launched or discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcId {
    pub pid: u32,
    pub start_time: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl ExitInfo {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

impl std::fmt::Display for ExitInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.code, self.signal) {
            (Some(c), _) => write!(f, "exit code {}", c),
            (None, Some(s)) => write!(f, "signal {}", s),
            _ => write!(f, "unknown status"),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SpawnOptions {
    pub env: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
    /// Regular file that receives the process's stderr. A file (not a pipe) is used so the
    /// application keeps running normally after AppNest exits.
    pub stderr_log: Option<PathBuf>,
}

pub struct SpawnedProcess {
    pub id: ProcId,
    exit_rx: Receiver<ExitInfo>,
}

impl SpawnedProcess {
    /// Waits up to `timeout` for the process to exit. `None` means it is still running.
    pub fn wait_for_exit(&self, timeout: Duration) -> Option<ExitInfo> {
        self.exit_rx.recv_timeout(timeout).ok()
    }
}

pub struct ScanTarget {
    pub id: String,
    pub app_path: PathBuf,
    pub apprun_path: Option<PathBuf>,
}

pub struct ProcessLauncher;

impl ProcessLauncher {
    /// Starts a process in its own process group and reaps it in a background thread.
    pub fn spawn(
        program: &Path,
        args: &[OsString],
        opts: &SpawnOptions,
    ) -> std::io::Result<SpawnedProcess> {
        let mut cmd = Command::new(program);
        cmd.args(args).envs(opts.env.iter().map(|(k, v)| (k, v)));
        if let Some(dir) = &opts.cwd {
            cmd.current_dir(dir);
        }
        cmd.process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        match &opts.stderr_log {
            Some(log) => {
                let file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(log)?;
                cmd.stderr(Stdio::from(file));
            }
            None => {
                cmd.stderr(Stdio::null());
            }
        }

        let mut child = cmd.spawn()?;
        let pid = child.id();
        // The child is not reaped yet, so its PID cannot have been reused.
        let start_time = Self::proc_start_time(pid).unwrap_or(0);

        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("appnest-reaper".into())
            .spawn(move || {
                if let Ok(status) = child.wait() {
                    let _ = tx.send(ExitInfo {
                        code: status.code(),
                        signal: status.signal(),
                    });
                }
            })?;

        Ok(SpawnedProcess {
            id: ProcId { pid, start_time },
            exit_rx: rx,
        })
    }

    /// Reads the process state character and start time (clock ticks since boot).
    fn proc_stat(pid: u32) -> Option<(char, u64)> {
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        // The command name is in parentheses and may itself contain spaces or parentheses.
        let rest = &stat[stat.rfind(')')? + 1..];
        let mut fields = rest.split_whitespace();
        let state = fields.next()?.chars().next()?;
        let start = fields.nth(18)?.parse().ok()?;
        Some((state, start))
    }

    pub fn proc_start_time(pid: u32) -> Option<u64> {
        Self::proc_stat(pid).map(|(_, start)| start)
    }

    pub fn is_pid_alive(pid: u32) -> bool {
        pid != 0 && Self::proc_stat(pid).is_some_and(|(state, _)| state != 'Z' && state != 'X')
    }

    /// True only if the same process (same PID and start time) is still running.
    pub fn is_alive(id: ProcId) -> bool {
        id.pid != 0
            && Self::proc_stat(id.pid).is_some_and(|(state, start)| {
                start == id.start_time && state != 'Z' && state != 'X'
            })
    }

    /// Sends SIGTERM to the process, and to its process group when `kill_group` is set and the
    /// process leads that group. Does nothing if the identity no longer matches.
    pub fn terminate(id: ProcId, kill_group: bool) -> std::io::Result<()> {
        if !Self::is_alive(id) {
            return Ok(());
        }
        let pid = id.pid as libc::pid_t;
        // SAFETY: plain libc calls on integer arguments.
        unsafe {
            if kill_group && libc::getpgid(pid) == pid {
                let _ = libc::kill(-pid, libc::SIGTERM);
            }
            if libc::kill(pid, libc::SIGTERM) != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    /// Finds running processes belonging to any of the targets in a single pass over /proc.
    /// Only processes of the current user are inspected.
    pub fn find_running(targets: &[ScanTarget]) -> HashMap<String, Vec<ProcId>> {
        let mut exe_map: HashMap<PathBuf, &str> = HashMap::new();
        let mut env_map: HashMap<Vec<u8>, &str> = HashMap::new();
        for t in targets {
            for p in [Some(&t.app_path), t.apprun_path.as_ref()]
                .into_iter()
                .flatten()
            {
                exe_map.insert(p.clone(), &t.id);
                if let Ok(c) = std::fs::canonicalize(p) {
                    exe_map.insert(c, &t.id);
                }
            }
            for p in [
                Some(t.app_path.clone()),
                std::fs::canonicalize(&t.app_path).ok(),
            ]
            .into_iter()
            .flatten()
            {
                let mut var = b"APPIMAGE=".to_vec();
                var.extend_from_slice(p.as_os_str().as_encoded_bytes());
                env_map.insert(var, &t.id);
            }
        }

        let mut found: HashMap<String, Vec<ProcId>> = HashMap::new();
        if targets.is_empty() {
            return found;
        }
        let Ok(proc_dir) = std::fs::read_dir("/proc") else {
            return found;
        };
        let me = std::process::id();
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };

        for entry in proc_dir.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == 0 || pid == me {
                continue;
            }
            if entry.metadata().map(|m| m.uid() != uid).unwrap_or(true) {
                continue;
            }
            let Some((state, start_time)) = Self::proc_stat(pid) else {
                continue;
            };
            if state == 'Z' || state == 'X' {
                continue;
            }
            let dir = entry.path();

            let mut owner = std::fs::read_link(dir.join("exe"))
                .ok()
                .and_then(|exe| exe_map.get(&exe).copied());

            if owner.is_none() {
                if let Ok(cmdline) = std::fs::read(dir.join("cmdline")) {
                    if let Some(argv0) = cmdline.split(|&b| b == 0).next() {
                        let argv0 = Path::new(std::ffi::OsStr::from_bytes(argv0));
                        owner = exe_map.get(argv0).copied();
                    }
                }
            }
            if owner.is_none() {
                if let Ok(environ) = std::fs::read(dir.join("environ")) {
                    owner = environ
                        .split(|&b| b == 0)
                        .find_map(|var| env_map.get(var).copied());
                }
            }

            if let Some(id) = owner {
                found
                    .entry(id.to_string())
                    .or_default()
                    .push(ProcId { pid, start_time });
            }
        }
        for ids in found.values_mut() {
            ids.sort_by_key(|p| p.pid);
        }
        found
    }

    pub fn open_containing_folder(path: &Path) -> std::io::Result<()> {
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let uri = gio::File::for_path(folder).uri();
        gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}
