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
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

type SharedHook = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

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
            .title("Create Application Menu Entries")
            .subtitle("Add imported AppImages to the system's application menu")
            .active(current_settings.auto_integrate_desktop)
            .build();
        paths_group.add(&desktop_switch);

        let shortcut_switch = SwitchRow::builder()
            .title("Create Desktop Shortcuts")
            .subtitle("Also place a launcher on your desktop when an AppImage is added")
            .active(current_settings.auto_desktop_shortcut)
            .build();
        paths_group.add(&shortcut_switch);

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
            "Extract and run (no FUSE)",
        ]);

        let initial_idx = match current_settings.default_runtime_method {
            RuntimeMethodPreference::Auto => 0,
            RuntimeMethodPreference::Extracted => 1,
            RuntimeMethodPreference::Native => 2,
            RuntimeMethodPreference::ExtractAndRun => 3,
        };

        let runtime_combo = ComboRow::builder()
            .title("Default Launch Method")
            .subtitle("Preferred method for new AppImages")
            .model(&runtime_methods)
            .selected(initial_idx)
            .build();
        exec_group.add(&runtime_combo);

        page.add(&exec_group);

        // Group 4: folders searched for AppImages that are not in the library yet
        let scan_group = PreferencesGroup::builder()
            .title("Discovery")
            .description("Folders searched for AppImages to offer for import. Press refresh to search again.")
            .build();
        let scan_dirs: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(
            if current_settings.scan_directories.is_empty() {
                paths.default_scan_directories()
            } else {
                current_settings.scan_directories.clone()
            },
        ));
        let scan_rows: Rc<RefCell<Vec<ActionRow>>> = Rc::new(RefCell::new(Vec::new()));
        let add_folder_row = ActionRow::builder()
            .title("Add Folder")
            .activatable(true)
            .build();
        let add_folder_btn = Button::builder()
            .icon_name("folder-new-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("Choose a folder to search")
            .build();
        add_folder_btn.add_css_class("flat");
        add_folder_row.add_suffix(&add_folder_btn);
        scan_group.add(&add_folder_row);
        page.add(&scan_group);

        let rebuild: SharedHook = Rc::new(RefCell::new(None));
        {
            let scan_group = scan_group.clone();
            let scan_dirs = Rc::clone(&scan_dirs);
            let scan_rows = Rc::clone(&scan_rows);
            let config_store = Arc::clone(&config_store);
            let rebuild_weak = Rc::downgrade(&rebuild);
            let f: Rc<dyn Fn()> = Rc::new(move || {
                for row in scan_rows.borrow_mut().drain(..) {
                    scan_group.remove(&row);
                }
                let mut settings = config_store.load();
                settings.scan_directories = scan_dirs.borrow().clone();
                let _ = config_store.save(&settings);

                for (index, dir) in scan_dirs.borrow().iter().enumerate() {
                    let row = ActionRow::builder()
                        .title(gtk4::glib::markup_escape_text(&dir.to_string_lossy()))
                        .build();
                    let remove = Button::builder()
                        .icon_name("user-trash-symbolic")
                        .valign(gtk4::Align::Center)
                        .tooltip_text("Stop searching this folder")
                        .build();
                    remove.add_css_class("flat");
                    let scan_dirs = Rc::clone(&scan_dirs);
                    let rebuild_weak = rebuild_weak.clone();
                    remove.connect_clicked(move |_| {
                        if index < scan_dirs.borrow().len() {
                            scan_dirs.borrow_mut().remove(index);
                        }
                        if let Some(rebuild) = rebuild_weak.upgrade() {
                            if let Some(f) = rebuild.borrow().clone() {
                                f();
                            }
                        }
                    });
                    row.add_suffix(&remove);
                    scan_group.add(&row);
                    scan_rows.borrow_mut().push(row);
                }
            });
            *rebuild.borrow_mut() = Some(f);
        }
        if let Some(f) = rebuild.borrow().clone() {
            // Show the rows without rewriting an untouched configuration
            let saved = config_store.load();
            f();
            let _ = config_store.save(&saved);
        }

        {
            let parent = parent.clone();
            let scan_dirs = Rc::clone(&scan_dirs);
            let rebuild = Rc::clone(&rebuild);
            add_folder_btn.connect_clicked(move |_| {
                let window = parent.root().and_downcast::<gtk4::Window>();
                let chooser = gtk4::FileDialog::builder()
                    .title("Choose a Folder to Search")
                    .build();
                let scan_dirs = Rc::clone(&scan_dirs);
                let rebuild = Rc::clone(&rebuild);
                chooser.select_folder(
                    window.as_ref(),
                    None::<&gtk4::gio::Cancellable>,
                    move |result| {
                        if let Some(path) = result.ok().and_then(|f| f.path()) {
                            if !scan_dirs.borrow().contains(&path) {
                                scan_dirs.borrow_mut().push(path);
                                if let Some(f) = rebuild.borrow().clone() {
                                    f();
                                }
                            }
                        }
                    },
                );
            });
        }

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

        let cs_clone_shortcut = Arc::clone(&config_store);
        shortcut_switch.connect_active_notify(move |sw| {
            let mut s = cs_clone_shortcut.load();
            s.auto_desktop_shortcut = sw.is_active();
            let _ = cs_clone_shortcut.save(&s);
        });

        let cs_clone3 = Arc::clone(&config_store);
        runtime_combo.connect_selected_notify(move |combo| {
            let mut s = cs_clone3.load();
            s.default_runtime_method = match combo.selected() {
                0 => RuntimeMethodPreference::Auto,
                1 => RuntimeMethodPreference::Extracted,
                2 => RuntimeMethodPreference::Native,
                3 => RuntimeMethodPreference::ExtractAndRun,
                _ => RuntimeMethodPreference::Auto,
            };
            let _ = cs_clone3.save(&s);
        });

        dialog.present(Some(parent));
    }
}
