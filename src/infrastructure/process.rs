use std::ffi::OsStr;
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
