use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::os::unix::process::CommandExt;

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
        let status_path = format!("/proc/{}/status", pid);
        if let Ok(content) = std::fs::read_to_string(&status_path) {
            for line in content.lines() {
                if line.starts_with("State:") {
                    // If zombie (Z) or dead (X), it is not running
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
        let full_path_str = app_path.to_string_lossy();
        let file_name = app_path.file_name().map(|n| n.to_string_lossy().to_string());
        let apprun_str = apprun_path.map(|p| p.to_string_lossy().to_string());

        let proc_dir = match std::fs::read_dir("/proc") {
            Ok(d) => d,
            Err(_) => return pids,
        };

        for entry in proc_dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Ok(pid) = name_str.parse::<u32>() {
                if pid == current_pid || !Self::is_pid_alive(pid) {
                    continue;
                }

                let mut matched = false;

                // 1. Check cmdline
                let cmdline_path = entry.path().join("cmdline");
                if let Ok(cmdline_bytes) = std::fs::read(&cmdline_path) {
                    let cmdline_str = String::from_utf8_lossy(&cmdline_bytes);
                    if cmdline_str.contains(&*full_path_str) {
                        matched = true;
                    } else if let Some(ref apprun) = apprun_str {
                        if cmdline_str.contains(apprun) {
                            matched = true;
                        }
                    } else if let Some(ref fname) = file_name {
                        for arg in cmdline_bytes.split(|&b| b == 0) {
                            let arg_str = String::from_utf8_lossy(arg);
                            if arg_str == *fname || arg_str.ends_with(&format!("/{}", fname)) {
                                matched = true;
                                break;
                            }
                        }
                    }
                }

                // 2. Check environ for APPIMAGE=
                if !matched {
                    let environ_path = entry.path().join("environ");
                    if let Ok(environ_bytes) = std::fs::read(&environ_path) {
                        let environ_str = String::from_utf8_lossy(&environ_bytes);
                        let target_env = format!("APPIMAGE={}", full_path_str);
                        if environ_str.contains(&target_env) {
                            matched = true;
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

