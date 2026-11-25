use appnest::config::paths::AppPaths;
use appnest::{cli, AppManagerApplication};
use gtk4::glib;
use std::ffi::OsString;

fn main() -> glib::ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Command-line commands never initialize GTK
    if let Some(code) = cli::dispatch(&args) {
        return glib::ExitCode::from(code as u8);
    }

    if AppPaths::try_new().is_none() {
        eprintln!("appnest: cannot determine the home directory; refusing to start");
        return glib::ExitCode::FAILURE;
    }
    AppManagerApplication::new().run()
}
