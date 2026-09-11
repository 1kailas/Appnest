use crate::config::paths::AppPaths;
use crate::config::settings::ThemePreference;
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::config_store::ConfigStore;
use crate::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use crate::ui::window::MainWindow;
use gtk4::gdk::Display;
use gtk4::prelude::*;
use gtk4::CssProvider;
use libadwaita::{Application, ColorScheme, StyleManager};
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
        });

        let paths_clone = paths.clone();
        let repo_clone = Arc::clone(&repo);
        let config_store_clone = Arc::clone(&config_store);

        app.connect_activate(move |app| {
            if let Some(win) = app.active_window() {
                win.present();
                return;
            }

            let main_window = MainWindow::new(
                app,
                paths_clone.clone(),
                Arc::clone(&repo_clone),
                Arc::clone(&config_store_clone),
            );
            main_window.window.present();
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
