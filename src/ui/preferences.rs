use crate::app::AppManagerApplication;
use crate::config::paths::AppPaths;
use crate::config::settings::{RuntimeMethodPreference, ThemePreference};
use crate::infrastructure::config_store::ConfigStore;
use crate::infrastructure::process::ProcessLauncher;
use gtk4::prelude::*;
use gtk4::{Button, StringList};
use libadwaita::prelude::*;
use libadwaita::{
    ActionRow, ComboRow, PreferencesDialog, PreferencesGroup, PreferencesPage, SwitchRow,
};
use std::sync::Arc;

pub struct PreferencesWindow;

impl PreferencesWindow {
    pub fn show(parent: &impl IsA<gtk4::Widget>, paths: AppPaths, config_store: Arc<ConfigStore>) {
        let current_settings = config_store.load();

        let page = PreferencesPage::builder()
            .title("General")
            .icon_name("preferences-system-symbolic")
            .build();

        // Group 1: Appearance
        let appearance_group = PreferencesGroup::builder()
            .title("Appearance")
            .description("Customize visual style and color scheme")
            .build();

        let theme_options = StringList::new(&["System Default", "Force Light", "Force Dark"]);

        let theme_idx = match current_settings.theme {
            ThemePreference::System => 0,
            ThemePreference::Light => 1,
            ThemePreference::Dark => 2,
        };

        let theme_combo = ComboRow::builder()
            .title("Color Scheme")
            .subtitle("Application dark / light style preference")
            .model(&theme_options)
            .selected(theme_idx)
            .build();
        appearance_group.add(&theme_combo);
        page.add(&appearance_group);

        // Group 2: Storage & Paths
        let paths_group = PreferencesGroup::builder()
            .title("Storage &amp; Integration")
            .description("Locations and default behaviors for AppImages")
            .build();

        let copy_switch = SwitchRow::builder()
            .title("Copy to Managed Storage")
            .subtitle(format!(
                "Store imported AppImages in {}",
                paths.applications_dir.display()
            ))
            .active(current_settings.copy_on_import)
            .build();

        paths_group.add(&copy_switch);

        let desktop_switch = SwitchRow::builder()
            .title("Automatic Desktop Integration")
            .subtitle(
                "Create .desktop entry in ~/.local/share/applications for system menus and dock",
            )
            .active(current_settings.auto_integrate_desktop)
            .build();
        paths_group.add(&desktop_switch);

        let open_dir_row = ActionRow::builder()
            .title("Open Applications Directory")
            .subtitle(&*paths.applications_dir.to_string_lossy())
            .activatable(true)
            .build();

        let open_btn = Button::builder()
            .icon_name("folder-open-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("Open folder in file manager")
            .build();
        open_btn.add_css_class("flat");

        let app_dir = paths.applications_dir.clone();
        open_btn.connect_clicked(move |_| {
            let _ = ProcessLauncher::open_containing_folder(&app_dir);
        });
        open_dir_row.add_suffix(&open_btn);
        paths_group.add(&open_dir_row);

        page.add(&paths_group);

        // Group 3: Execution Preferences
        let exec_group = PreferencesGroup::builder()
            .title("Execution Strategy")
            .description("Default fallback behavior when launching AppImages")
            .build();

        let runtime_methods = StringList::new(&[
            "Auto (Adaptive)",
            "Extracted AppRun",
            "Direct Native",
            "FUSE / Extract-and-run",
        ]);

        let initial_idx = match current_settings.default_runtime_method {
            RuntimeMethodPreference::Auto => 0,
            RuntimeMethodPreference::Extracted => 1,
            RuntimeMethodPreference::Native => 2,
            RuntimeMethodPreference::Fuse => 3,
        };

        let runtime_combo = ComboRow::builder()
            .title("Default Launch Method")
            .subtitle("Preferred method for new AppImages")
            .model(&runtime_methods)
            .selected(initial_idx)
            .build();
        exec_group.add(&runtime_combo);

        page.add(&exec_group);

        let dialog = PreferencesDialog::builder().title("Preferences").build();
        dialog.add(&page);

        // Save on changes
        let cs_clone_theme = Arc::clone(&config_store);
        theme_combo.connect_selected_notify(move |combo| {
            let new_theme = match combo.selected() {
                0 => ThemePreference::System,
                1 => ThemePreference::Light,
                2 => ThemePreference::Dark,
                _ => ThemePreference::System,
            };
            AppManagerApplication::apply_theme(new_theme);
            let mut s = cs_clone_theme.load();
            s.theme = new_theme;
            let _ = cs_clone_theme.save(&s);
        });

        let cs_clone1 = Arc::clone(&config_store);
        copy_switch.connect_active_notify(move |sw| {
            let mut s = cs_clone1.load();
            s.copy_on_import = sw.is_active();
            let _ = cs_clone1.save(&s);
        });

        let cs_clone2 = Arc::clone(&config_store);
        desktop_switch.connect_active_notify(move |sw| {
            let mut s = cs_clone2.load();
            s.auto_integrate_desktop = sw.is_active();
            let _ = cs_clone2.save(&s);
        });

        let cs_clone3 = Arc::clone(&config_store);
        runtime_combo.connect_selected_notify(move |combo| {
            let mut s = cs_clone3.load();
            s.default_runtime_method = match combo.selected() {
                0 => RuntimeMethodPreference::Auto,
                1 => RuntimeMethodPreference::Extracted,
                2 => RuntimeMethodPreference::Native,
                3 => RuntimeMethodPreference::Fuse,
                _ => RuntimeMethodPreference::Auto,
            };
            let _ = cs_clone3.save(&s);
        });

        dialog.present(Some(parent));
    }
}
