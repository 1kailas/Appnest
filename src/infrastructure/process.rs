use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct ProcessLauncher;

impl ProcessLauncher {
    pub fn spawn_detached<I, S>(
        program: &Path,
        args: I,
        working_dir: Option<&Path>,
    ) -> std::io::Result<u32>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(program);
        cmd.args(args);

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        } else if let Some(parent) = program.parent() {
            cmd.current_dir(parent);
        }

        // Put process into its own process group so all subprocesses can be signaled together
        #[cfg(unix)]
        cmd.process_group(0);

        // Detach process so GUI application doesn't lock up or kill the app when closed
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = cmd.spawn()?;
        Ok(child.id())
    }

    pub fn run_sync<I, S>(program: &Path, args: I) -> std::io::Result<(bool, String, String)>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new(program).args(args).output()?;
        let success = output.status.success();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((success, stdout, stderr))
    }

    pub fn is_pid_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        let status_path = format!("/proc/{}/status", pid);
        if let Ok(content) = std::fs::read_to_string(&status_path) {
            for line in content.lines() {
                if let Some(state_val) = line.strip_prefix("State:\t") {
                    if let Some(first_char) = state_val.trim().chars().next() {
                        if first_char == 'Z' || first_char == 'X' {
                            return false;
                        }
                    }
                    return true;
                } else if line.starts_with("State:") {
                    if line.contains('Z') || line.contains('X') {
                        return false;
                    }
                    return true;
                }
            }
            true
        } else {
            false
        }
    }

    pub fn terminate_pid(pid: u32) -> Result<(), std::io::Error> {
        if pid == 0 {
            return Ok(());
        }
        #[cfg(unix)]
        unsafe {
            let pid_i32 = pid as libc::pid_t;
            // Send SIGTERM to the process group first (negative PID)
            let _ = libc::kill(-pid_i32, libc::SIGTERM);
            // Also send SIGTERM directly to the PID in case it wasn't the group leader
            let res = libc::kill(pid_i32, libc::SIGTERM);
            if res != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(err);
                }
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Ok(())
        }
    }

    pub fn find_pids_for_appimage(app_path: &Path, apprun_path: Option<&Path>) -> Vec<u32> {
        let mut pids = Vec::new();
        let current_pid = std::process::id();
        let canonical_app_path = std::fs::canonicalize(app_path).ok();
        let app_path_str = app_path.to_string_lossy();
        let target_env = format!("APPIMAGE={}", app_path_str);
        let target_env_canonical = canonical_app_path
            .as_ref()
            .map(|p| format!("APPIMAGE={}", p.to_string_lossy()));

        let canonical_apprun = apprun_path.and_then(|p| std::fs::canonicalize(p).ok());

        let proc_dir = match std::fs::read_dir("/proc") {
            Ok(d) => d,
            Err(_) => return pids,
        };

        for entry in proc_dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Ok(pid) = name_str.parse::<u32>() {
                if pid == current_pid || pid == 0 || !Self::is_pid_alive(pid) {
                    continue;
                }

                let mut matched = false;

                // 1. Check /proc/<pid>/exe symlink
                let exe_link = entry.path().join("exe");
                if let Ok(target) = std::fs::read_link(&exe_link) {
                    if target == app_path
                        || canonical_app_path.as_ref().map_or(false, |p| *p == target)
                    {
                        matched = true;
                    } else if let Some(ref apprun) = canonical_apprun {
                        if target == *apprun {
                            matched = true;
                        }
                    } else if let Some(apprun) = apprun_path {
                        if target == apprun {
                            matched = true;
                        }
                    }
                }

                // 2. Check /proc/<pid>/environ for exact APPIMAGE=<path> item
                if !matched {
                    let environ_path = entry.path().join("environ");
                    if let Ok(environ_bytes) = std::fs::read(&environ_path) {
                        for var in environ_bytes.split(|&b| b == 0) {
                            if let Ok(var_str) = std::str::from_utf8(var) {
                                if var_str == target_env
                                    || target_env_canonical.as_deref() == Some(var_str)
                                {
                                    matched = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                // 3. Check /proc/<pid>/cmdline: Only match the FIRST argument (argv[0])
                if !matched {
                    let cmdline_path = entry.path().join("cmdline");
                    if let Ok(cmdline_bytes) = std::fs::read(&cmdline_path) {
                        if let Some(argv0_bytes) = cmdline_bytes.split(|&b| b == 0).next() {
                            let argv0 = String::from_utf8_lossy(argv0_bytes);
                            let argv0_path = Path::new(&*argv0);
                            if argv0_path == app_path
                                || canonical_app_path
                                    .as_ref()
                                    .map_or(false, |p| p.as_path() == argv0_path)
                            {
                                matched = true;
                            } else if let Some(apprun) = apprun_path {
                                if argv0_path == apprun
                                    || canonical_apprun
                                        .as_ref()
                                        .map_or(false, |p| p.as_path() == argv0_path)
                                {
                                    matched = true;
                                }
                            }
                        }
                    }
                }

                if matched {
                    pids.push(pid);
                }
            }
        }

        pids.sort_unstable();
        pids.dedup();
        pids
    }

    pub fn open_containing_folder(path: &Path) -> std::io::Result<()> {
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };

        Command::new("xdg-open")
            .arg(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        Ok(())
    }
}
