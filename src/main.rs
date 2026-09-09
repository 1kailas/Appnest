use appnest::AppManagerApplication;
use gtk4::glib;

fn main() -> glib::ExitCode {
    let app = AppManagerApplication::new();
    app.run()
}
