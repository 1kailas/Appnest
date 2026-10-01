use crate::config::paths::AppPaths;
use crate::config::settings::ThemePreference;
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::config_store::ConfigStore;
use crate::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use crate::services::desktop_entry::DesktopEntryService;
use crate::ui::window::MainWindow;
use gtk4::gdk::Display;
use gtk4::gio::ApplicationFlags;
use gtk4::prelude::*;
use gtk4::CssProvider;
use libadwaita::{Application, ColorScheme, StyleManager};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub struct AppManagerApplication {
    pub app: Application,
}

impl Default for AppManagerApplication {
    fn default() -> Self {
        Self::new()
    }
}

impl AppManagerApplication {
    pub fn new() -> Self {
        let app = Application::builder()
            .application_id("io.github._1kailas.AppNest")
            .flags(ApplicationFlags::HANDLES_OPEN)
            .build();

        let paths = AppPaths::new();
        let _ = paths.ensure_dirs();

        let config_store = Arc::new(ConfigStore::new(paths.clone()));
        let repo: Arc<dyn AppImageRepository> =
            Arc::new(FilesystemAppImageRepository::new(paths.clone()));

        // Configure global keyboard accelerators
        app.set_accels_for_action("win.add", &["<Control>o", "<Control>n"]);
        app.set_accels_for_action("win.refresh", &["<Control>r", "F5"]);
        app.set_accels_for_action("win.search", &["<Control>f"]);
        app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        app.set_accels_for_action("win.close", &["<Control>q", "<Control>w"]);

        let config_store_for_startup = Arc::clone(&config_store);
        app.connect_startup(move |_| {
            let settings = config_store_for_startup.load();
            Self::apply_theme(settings.theme);
            Self::load_styles();
            // Run MIME registration in background to avoid blocking startup
            std::thread::spawn(|| {
                DesktopEntryService::ensure_mime_integration();
            });
        });

        let main_window_cell: Rc<RefCell<Option<Rc<MainWindow>>>> = Rc::new(RefCell::new(None));

        let get_or_create_window = {
            let main_window_cell = Rc::clone(&main_window_cell);
            let paths = paths.clone();
            let repo = Arc::clone(&repo);
            let config_store = Arc::clone(&config_store);

            move |app: &Application| -> Rc<MainWindow> {
                let mut cell = main_window_cell.borrow_mut();
                if let Some(ref win) = *cell {
                    if win.window.is_visible() {
                        return Rc::clone(win);
                    }
                }
                let win = Rc::new(MainWindow::new(
                    app,
                    paths.clone(),
                    Arc::clone(&repo),
                    Arc::clone(&config_store),
                ));
                *cell = Some(Rc::clone(&win));
                win
            }
        };

        let get_win_activate = get_or_create_window.clone();
        app.connect_activate(move |app| {
            let win = get_win_activate(app);
            win.window.present();
        });

        let get_win_open = get_or_create_window.clone();
        app.connect_open(move |app, files, _hint| {
            let win = get_win_open(app);
            win.window.present();

            for file in files {
                if let Some(path) = file.path() {
                    win.prompt_open_file(path);
                }
            }
        });

        Self { app }
    }

    pub fn run(&self) -> glib::ExitCode {
        self.app.run()
    }

    pub fn apply_theme(theme: ThemePreference) {
        let sm = StyleManager::default();
        match theme {
            ThemePreference::System => sm.set_color_scheme(ColorScheme::Default),
            ThemePreference::Light => sm.set_color_scheme(ColorScheme::ForceLight),
            ThemePreference::Dark => sm.set_color_scheme(ColorScheme::ForceDark),
        }
    }

    fn load_styles() {
        let provider = CssProvider::new();
        let css = include_str!("../resources/styles/style.css");
        provider.load_from_data(css);

        if let Some(display) = Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }
}
