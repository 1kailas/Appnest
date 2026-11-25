use appnest::config::paths::AppPaths;
use appnest::AppManagerApplication;
use gtk4::glib;

fn main() -> glib::ExitCode {
    if AppPaths::try_new().is_none() {
        eprintln!("appnest: cannot determine the home directory; refusing to start");
        return glib::ExitCode::FAILURE;
    }
    AppManagerApplication::new().run()
}
