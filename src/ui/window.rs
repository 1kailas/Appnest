use crate::application::commands::{
    CloseAppImageCommand, ImportAppImageCommand, LaunchAppImageCommand, RemoveAppImageCommand,
    UpdateAppImageCommand,
};
use crate::application::queries::{AppImagesListing, ListAppImagesQuery};
use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, RuntimeMethod};
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::config_store::ConfigStore;
use crate::infrastructure::process::ProcessLauncher;
use crate::services::desktop_entry::DesktopEntryService;
use crate::services::extractor::AppImageExtractor;
use crate::services::importer::AppImageImporter;
use crate::services::launcher::AppImageLauncher;
use crate::services::validator::AppImageValidator;
use crate::ui::app_details::AppDetailsWidget;
use crate::ui::app_list::AppListWidget;
use crate::ui::app_row::AppRowWidget;
use crate::ui::preferences::PreferencesWindow;
use gtk4::gdk;
use gtk4::gio::{self, SimpleAction};
use gtk4::prelude::*;
use gtk4::{
    Align, Box, Button, DropTarget, FileChooserAction, FileChooserNative, FileFilter, MenuButton,
    Orientation, Paned, ResponseType, Stack,
};
use libadwaita::prelude::*;
use libadwaita::{
    AboutDialog, ActionRow, AlertDialog, Application, ApplicationWindow, HeaderBar, StatusPage,
    Toast, ToastOverlay,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

struct ControllerState {
    apps: Vec<AppImage>,
    filtered_apps: Vec<AppImage>,
    unmanaged: Vec<PathBuf>,
    selected_id: Option<String>,
    row_widgets: HashMap<String, Rc<AppRowWidget>>,
}

pub struct MainWindow {
    pub window: ApplicationWindow,
}

impl MainWindow {
    pub fn new(
        app: &Application,
        paths: AppPaths,
        repo: Arc<dyn AppImageRepository>,
        config_store: Arc<ConfigStore>,
    ) -> Self {
        let importer = Arc::new(AppImageImporter::new(paths.clone()));
        let launcher = Arc::new(AppImageLauncher::new(paths.clone()));

        let import_cmd = Arc::new(ImportAppImageCommand::new(Arc::clone(&repo), importer));
        let remove_cmd = Arc::new(RemoveAppImageCommand::new(
            Arc::clone(&repo),
            paths.clone(),
            Arc::clone(&launcher),
        ));
        let launch_cmd = Arc::new(LaunchAppImageCommand::new(
            Arc::clone(&repo),
            Arc::clone(&launcher),
        ));
        let close_cmd = Arc::new(CloseAppImageCommand::new(
            Arc::clone(&repo),
            Arc::clone(&launcher),
        ));
        let update_cmd = Arc::new(UpdateAppImageCommand::new(Arc::clone(&repo), paths.clone()));
        let list_query = Arc::new(ListAppImagesQuery::new(Arc::clone(&repo)));

        let state = Rc::new(RefCell::new(ControllerState {
            apps: Vec::new(),
            filtered_apps: Vec::new(),
            unmanaged: Vec::new(),
            selected_id: None,
            row_widgets: HashMap::new(),
        }));


        // Prevents signal handler loops when programmatically updating widgets
        let is_updating = Rc::new(Cell::new(false));

        let toast_overlay = ToastOverlay::new();

        // Main vertical layout
        let main_box = Box::builder().orientation(Orientation::Vertical).build();

        // HeaderBar
        let header_bar = HeaderBar::builder().build();

        // Add button
        let add_btn = Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add or Import AppImage")
            .build();
        header_bar.pack_start(&add_btn);

        // Refresh button
        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh Application List")
            .build();
        header_bar.pack_start(&refresh_btn);

        // Menu button
        let menu_btn = MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .build();

        let menu = gio::Menu::new();
        menu.append(Some("Preferences"), Some("win.preferences"));
        menu.append(Some("Open Applications Folder"), Some("win.open_folder"));
        menu.append(Some("About AppNest"), Some("win.about"));
        menu_btn.set_menu_model(Some(&menu));
        header_bar.pack_end(&menu_btn);

        main_box.append(&header_bar);

        // Two-pane paned layout
        let paned = Paned::builder()
            .orientation(Orientation::Horizontal)
            .wide_handle(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .hexpand(true)
            .vexpand(true)
            .build();
        paned.set_position(340);

        let app_list_widget = Rc::new(RefCell::new(AppListWidget::new()));
        paned.set_start_child(Some(&app_list_widget.borrow().container));

        // Right side: Stack with placeholder or details
        let right_stack = Stack::new();

        let placeholder_page = StatusPage::builder()
            .icon_name("application-x-executable-symbolic")
            .title("No Application Selected")
            .description("Select an AppImage from the list to view options or launch.")
            .build();
        right_stack.add_named(&placeholder_page, Some("placeholder"));

        let app_details_widget = Rc::new(RefCell::new(AppDetailsWidget::new()));
        right_stack.add_named(&app_details_widget.borrow().container, Some("details"));
        right_stack.set_visible_child_name("placeholder");

        paned.set_end_child(Some(&right_stack));
        main_box.append(&paned);

        toast_overlay.set_child(Some(&main_box));

        let window = ApplicationWindow::builder()
            .application(app)
            .title("AppNest")
            .default_width(960)
            .default_height(640)
            .content(&toast_overlay)
            .build();

        // Register window actions for menu
        let paths_clone_for_action = paths.clone();
        let config_store_clone_for_action = Arc::clone(&config_store);
        let win_weak = window.downgrade();

        let pref_action = SimpleAction::new("preferences", None);
        let win_weak_pref = win_weak.clone();
        pref_action.connect_activate(move |_, _| {
            if let Some(win) = win_weak_pref.upgrade() {
                PreferencesWindow::show(
                    &win,
                    paths_clone_for_action.clone(),
                    Arc::clone(&config_store_clone_for_action),
                );
            }
        });
        window.add_action(&pref_action);

        let apps_dir_clone = paths.applications_dir.clone();
        let open_folder_action = SimpleAction::new("open_folder", None);
        open_folder_action.connect_activate(move |_, _| {
            let _ = ProcessLauncher::open_containing_folder(&apps_dir_clone);
        });
        window.add_action(&open_folder_action);

        let win_weak_about = win_weak.clone();
        let about_action = SimpleAction::new("about", None);
        about_action.connect_activate(move |_, _| {
            if let Some(win) = win_weak_about.upgrade() {
                let about = AboutDialog::builder()
                    .application_name("AppNest")
                    .application_icon("io.github._1kailas.AppNest")
                    .version("0.1.0")
                    .developer_name("1kailas")
                    .comments("A modern, high-performance AppImage manager built with Rust, GTK4, and Libadwaita.")
                    .website("https://github.com/1kailas/appnest")
                    .issue_url("https://github.com/1kailas/appnest/issues")
                    .copyright("© 2026 1kailas")
                    .license_type(gtk4::License::Gpl30)
                    .build();
                about.present(Some(&win));
            }
        });
        window.add_action(&about_action);

        // Function to refresh apps list
        let refresh_list_fn = {
            let state = Rc::clone(&state);
            let list_query = Arc::clone(&list_query);
            let config_store = Arc::clone(&config_store);
            let app_list_widget = Rc::clone(&app_list_widget);
            let app_details_widget = Rc::clone(&app_details_widget);
            let right_stack = right_stack.clone();
            let toast_overlay = toast_overlay.clone();
            let launch_cmd = Arc::clone(&launch_cmd);
            let close_cmd = Arc::clone(&close_cmd);
            let import_cmd = Arc::clone(&import_cmd);
            let is_updating = Rc::clone(&is_updating);
            let paths_for_refresh = paths.clone();
            let repo_for_refresh = Arc::clone(&repo);

            Rc::new(move || {
                let settings = config_store.load();
                let scan_dirs = if settings.scan_directories.is_empty() {
                    config_store.load().scan_directories
                } else {
                    settings.scan_directories.clone()
                };

                let mut listing = list_query.execute(&scan_dirs).unwrap_or(AppImagesListing {
                    managed: Vec::new(),
                    unmanaged_discovered: Vec::new(),
                });

                // Auto-extract missing icons for managed applications
                for app in &mut listing.managed {
                    if app.icon_path.as_ref().map_or(true, |p| !p.exists()) {
                        if let Some(extracted_icon) = AppImageExtractor::extract_icon_only(
                            &app.path,
                            &paths_for_refresh.icons_dir,
                            &app.id,
                            app.metadata.icon_name.as_deref(),
                        ) {
                            app.icon_path = Some(extracted_icon);
                            let _ = repo_for_refresh.save(app);
                            if app.desktop_integrated {
                                let _ = DesktopEntryService::create_entry(
                                    app,
                                    &paths_for_refresh.desktop_applications_dir,
                                );
                            }
                        }
                    }
                }

                let query = app_list_widget
                    .borrow()
                    .search_entry
                    .text()
                    .to_string()
                    .to_lowercase();

                let filtered_apps: Vec<AppImage> = listing
                    .managed
                    .iter()
                    .filter(|a| {
                        query.is_empty()
                            || a.name.to_lowercase().contains(&query)
                            || a.id.to_lowercase().contains(&query)
                    })
                    .cloned()
                    .collect();

                {
                    let mut st = state.borrow_mut();
                    st.apps = listing.managed.clone();
                    st.filtered_apps = filtered_apps.clone();
                    st.unmanaged = listing.unmanaged_discovered.clone();
                }

                // Clear installed listbox safely
                let listbox = &app_list_widget.borrow().installed_list;
                while let Some(row) = listbox.row_at_index(0) {
                    listbox.remove(&row);
                }

                let mut row_map = HashMap::new();
                for app in &filtered_apps {
                    let row_widget = Rc::new(AppRowWidget::new(app));
                    let app_id = app.id.clone();
                    let app_name = app.name.clone();

                    row_widget.set_running_state(close_cmd.is_running(&app_id));

                    // Connect quick launch / close button
                    let launch_cmd_clone = Arc::clone(&launch_cmd);
                    let close_cmd_clone = Arc::clone(&close_cmd);
                    let toast_overlay_clone = toast_overlay.clone();
                    let row_widget_clone = Rc::clone(&row_widget);
                    let app_details_widget_clone = Rc::clone(&app_details_widget);
                    let state_clone = Rc::clone(&state);
                    let app_id_c = app_id.clone();
                    let app_name_c = app_name.clone();

                    row_widget.launch_button.connect_clicked(move |_| {
                        let is_running = close_cmd_clone.is_running(&app_id_c);
                        if is_running {
                            match close_cmd_clone.execute(&app_id_c) {
                                Ok(_) => {
                                    toast_overlay_clone.add_toast(Toast::new(&format!(
                                        "Closed {}",
                                        app_name_c
                                    )));
                                    row_widget_clone.set_running_state(false);
                                    if state_clone.borrow().selected_id.as_deref()
                                        == Some(&app_id_c)
                                    {
                                        app_details_widget_clone
                                            .borrow()
                                            .set_running_state(false);
                                    }
                                }
                                Err(e) => {
                                    toast_overlay_clone.add_toast(Toast::new(&format!(
                                        "Failed to close {}: {}",
                                        app_name_c, e
                                    )));
                                }
                            }
                        } else {
                            match launch_cmd_clone.execute(&app_id_c, &[]) {
                                Ok(pid) => {
                                    toast_overlay_clone.add_toast(Toast::new(&format!(
                                        "Launched {} (PID: {})",
                                        app_name_c, pid
                                    )));
                                    row_widget_clone.set_running_state(true);
                                    if state_clone.borrow().selected_id.as_deref()
                                        == Some(&app_id_c)
                                    {
                                        app_details_widget_clone
                                            .borrow()
                                            .set_running_state(true);
                                    }
                                }
                                Err(e) => {
                                    toast_overlay_clone.add_toast(Toast::new(&format!(
                                        "Failed to launch {}: {}",
                                        app_name_c, e
                                    )));
                                }
                            }
                        }
                    });

                    row_map.insert(app_id, Rc::clone(&row_widget));
                    listbox.append(&row_widget.row);
                }

                state.borrow_mut().row_widgets = row_map;


                // Populate discovered unmanaged list safely
                let discovered_list = &app_list_widget.borrow().discovered_list;
                while let Some(row) = discovered_list.row_at_index(0) {
                    discovered_list.remove(&row);
                }

                let unmanaged = &listing.unmanaged_discovered;
                if unmanaged.is_empty() {
                    app_list_widget.borrow().discovered_box.set_visible(false);
                } else {
                    app_list_widget.borrow().discovered_box.set_visible(true);
                    for path in unmanaged {
                        let row = ActionRow::builder()
                            .title(&*path.file_name().unwrap_or_default().to_string_lossy())
                            .subtitle(&*path.to_string_lossy())
                            .build();

                        let import_btn = Button::builder()
                            .label("Import")
                            .valign(Align::Center)
                            .build();
                        import_btn.add_css_class("suggested-action");
                        import_btn.add_css_class("pill");

                        let import_cmd_c = Arc::clone(&import_cmd);
                        let config_store_c = Arc::clone(&config_store);
                        let toast_c = toast_overlay.clone();
                        let p_clone = path.clone();
                        let state_c = Rc::clone(&state);

                        import_btn.connect_clicked(move |btn| {
                            btn.set_sensitive(false);
                            btn.set_label("Importing...");
                            let p_inner = p_clone.clone();
                            let import_cmd_inner = Arc::clone(&import_cmd_c);
                            let config_store_inner = Arc::clone(&config_store_c);
                            let toast_inner = toast_c.clone();
                            let btn_inner = btn.clone();
                            let state_inner = Rc::clone(&state_c);

                            let (tx, rx) = async_channel::bounded(1);
                            std::thread::spawn(move || {
                                let s = config_store_inner.load();
                                let res = import_cmd_inner.execute(&p_inner, &s);
                                let _ = tx.send_blocking(res);
                            });

                            glib::spawn_future_local(async move {
                                if let Ok(res) = rx.recv().await {
                                    btn_inner.set_sensitive(true);
                                    btn_inner.set_label("Import");
                                    match res {
                                        Ok(imported) => {
                                            toast_inner.add_toast(Toast::new(&format!(
                                                "Imported {}",
                                                imported.name
                                            )));
                                            state_inner.borrow_mut().selected_id =
                                                Some(imported.id);
                                        }
                                        Err(e) => {
                                            toast_inner.add_toast(Toast::new(&format!(
                                                "Import error: {}",
                                                e
                                            )));
                                        }
                                    }
                                }
                            });
                        });

                        row.add_suffix(&import_btn);
                        discovered_list.append(&row);
                    }
                }

                // Update empty state
                let has_apps = !filtered_apps.is_empty();
                let is_searching = !query.is_empty();
                let has_discovered = !unmanaged.is_empty();
                app_list_widget
                    .borrow()
                    .set_empty_state(has_apps, is_searching, has_discovered);

                // If currently selected app is still present, re-select
                let found_app = {
                    let st = state.borrow();
                    st.selected_id
                        .as_ref()
                        .and_then(|id| st.apps.iter().find(|a| &a.id == id).cloned())
                };

                if let Some(ref found) = found_app {
                    is_updating.set(true);
                    app_details_widget.borrow_mut().update(found);
                    app_details_widget
                        .borrow()
                        .set_running_state(close_cmd.is_running(&found.id));
                    is_updating.set(false);
                    right_stack.set_visible_child_name("details");


                    // Visually highlight row in listbox
                    if let Some(pos) = filtered_apps.iter().position(|a| a.id == found.id) {
                        if let Some(row) = listbox.row_at_index(pos as i32) {
                            listbox.select_row(Some(&row));
                        }
                    }
                } else {
                    right_stack.set_visible_child_name("placeholder");
                }
            })
        };

        // Connect Refresh Button
        let r_fn = Rc::clone(&refresh_list_fn);
        refresh_btn.connect_clicked(move |_| {
            r_fn();
        });

        // Connect SearchEntry change
        let r_fn_search = Rc::clone(&refresh_list_fn);
        app_list_widget
            .borrow()
            .search_entry
            .connect_search_changed(move |_| {
                r_fn_search();
            });

        // Connect row activation / selection in listbox
        {
            let state = Rc::clone(&state);
            let app_details_widget = Rc::clone(&app_details_widget);
            let right_stack = right_stack.clone();
            let is_updating = Rc::clone(&is_updating);
            let paths_for_select = paths.clone();
            let repo_for_select = Arc::clone(&repo);
            let close_cmd = Arc::clone(&close_cmd);

            app_list_widget
                .borrow()
                .installed_list
                .connect_row_selected(move |_, row_opt| {
                    if let Some(row) = row_opt {
                        let idx = row.index() as usize;
                        let app_opt = {
                            let st = state.borrow();
                            st.filtered_apps.get(idx).cloned()
                        };
                        if let Some(mut app) = app_opt {
                            // Ensure icon is extracted and available
                            if app.icon_path.as_ref().map_or(true, |p| !p.exists()) {
                                if let Some(extracted) = AppImageExtractor::extract_icon_only(
                                    &app.path,
                                    &paths_for_select.icons_dir,
                                    &app.id,
                                    app.metadata.icon_name.as_deref(),
                                ) {
                                    app.icon_path = Some(extracted);
                                    let _ = repo_for_select.save(&app);
                                }
                            }

                            {
                                state.borrow_mut().selected_id = Some(app.id.clone());
                            }
                            is_updating.set(true);
                            app_details_widget.borrow_mut().update(&app);
                            app_details_widget
                                .borrow()
                                .set_running_state(close_cmd.is_running(&app.id));
                            is_updating.set(false);
                            right_stack.set_visible_child_name("details");
                        }
                    }
                });
        }

        // Connect Details: Launch / Close Button
        {
            let state = Rc::clone(&state);
            let launch_cmd = Arc::clone(&launch_cmd);
            let close_cmd = Arc::clone(&close_cmd);
            let toast_overlay = toast_overlay.clone();
            let app_details_widget = Rc::clone(&app_details_widget);
            let launch_btn = app_details_widget.borrow().launch_btn.clone();

            launch_btn.connect_clicked(move |_| {

                    let selected = {
                        let st = state.borrow();
                        st.selected_id.as_ref().and_then(|id| {
                            st.apps
                                .iter()
                                .find(|a| &a.id == id)
                                .map(|a| (id.clone(), a.name.clone()))
                        })
                    };

                    if let Some((app_id, app_name)) = selected {
                        let is_running = close_cmd.is_running(&app_id);
                        if is_running {
                            match close_cmd.execute(&app_id) {
                                Ok(_) => {
                                    toast_overlay.add_toast(Toast::new(&format!(
                                        "Closed {}",
                                        app_name
                                    )));
                                    app_details_widget.borrow().set_running_state(false);
                                    let st = state.borrow();
                                    if let Some(row_widget) = st.row_widgets.get(&app_id) {
                                        row_widget.set_running_state(false);
                                    }
                                }
                                Err(e) => {
                                    toast_overlay.add_toast(Toast::new(&format!(
                                        "Failed to close {}: {}",
                                        app_name, e
                                    )));
                                }
                            }
                        } else {
                            match launch_cmd.execute(&app_id, &[]) {
                                Ok(pid) => {
                                    toast_overlay.add_toast(Toast::new(&format!(
                                        "Launched {} (PID: {})",
                                        app_name, pid
                                    )));
                                    app_details_widget.borrow().set_running_state(true);
                                    let st = state.borrow();
                                    if let Some(row_widget) = st.row_widgets.get(&app_id) {
                                        row_widget.set_running_state(true);
                                    }
                                }
                                Err(e) => {
                                    toast_overlay.add_toast(Toast::new(&format!(
                                        "Launch error for {}: {}",
                                        app_name, e
                                    )));
                                }
                            }
                        }
                    }
                });
        }

        // Periodic running status watcher (syncs button states every second)
        {
            let state = Rc::clone(&state);
            let close_cmd = Arc::clone(&close_cmd);
            let app_details_widget = Rc::clone(&app_details_widget);

            glib::timeout_add_local(std::time::Duration::from_millis(1000), move || {
                let (selected_id, row_widgets) = {
                    let st = state.borrow();
                    (st.selected_id.clone(), st.row_widgets.clone())
                };

                if let Some(ref id) = selected_id {
                    let running = close_cmd.is_running(id);
                    app_details_widget.borrow().set_running_state(running);
                }

                for (id, row_widget) in row_widgets {
                    let running = close_cmd.is_running(&id);
                    row_widget.set_running_state(running);
                }

                glib::ControlFlow::Continue
            });
        }


        // Connect Details: Reveal File Button
        {
            let state = Rc::clone(&state);
            app_details_widget
                .borrow()
                .reveal_btn
                .connect_clicked(move |_| {
                    let app_path = {
                        let st = state.borrow();
                        st.selected_id.as_ref().and_then(|id| {
                            st.apps.iter().find(|a| &a.id == id).map(|a| a.path.clone())
                        })
                    };

                    if let Some(path) = app_path {
                        let _ = ProcessLauncher::open_containing_folder(&path);
                    }
                });
        }

        // Connect Details: Copy Path Button
        {
            let state = Rc::clone(&state);
            let toast_overlay = toast_overlay.clone();
            let window_clone = window.clone();
            app_details_widget
                .borrow()
                .copy_path_btn
                .connect_clicked(move |_| {
                    let app_path = {
                        let st = state.borrow();
                        st.selected_id.as_ref().and_then(|id| {
                            st.apps.iter().find(|a| &a.id == id).map(|a| a.path.clone())
                        })
                    };

                    if let Some(path) = app_path {
                        let display = gtk4::prelude::WidgetExt::display(&window_clone);
                        let clipboard = display.clipboard();
                        clipboard.set_text(&path.to_string_lossy());
                        toast_overlay.add_toast(Toast::new("File path copied to clipboard"));
                    }
                });
        }

        // Connect Details: Non-blocking AppDir Extraction Button
        {
            let state = Rc::clone(&state);
            let update_cmd = Arc::clone(&update_cmd);
            let toast_overlay = toast_overlay.clone();
            let r_fn = Rc::clone(&refresh_list_fn);

            app_details_widget
                .borrow()
                .extract_btn
                .connect_clicked(move |btn| {
                    let selected_id = { state.borrow().selected_id.clone() };

                    if let Some(id) = selected_id {
                        btn.set_sensitive(false);
                        btn.set_label("Extracting AppDir...");
                        toast_overlay.add_toast(Toast::new("Extracting SquashFS in background..."));

                        let update_cmd_clone = Arc::clone(&update_cmd);
                        let id_clone = id.clone();
                        let toast_overlay_c = toast_overlay.clone();
                        let r_fn_c = Rc::clone(&r_fn);
                        let btn_clone = btn.clone();
                        let state_c = Rc::clone(&state);

                        let (tx, rx) = async_channel::bounded(1);
                        std::thread::spawn(move || {
                            let res = update_cmd_clone.extract_appdir(&id_clone);
                            let _ = tx.send_blocking(res);
                        });

                        glib::spawn_future_local(async move {
                            if let Ok(res) = rx.recv().await {
                                btn_clone.set_sensitive(true);
                                match res {
                                    Ok(updated) => {
                                        toast_overlay_c.add_toast(Toast::new(&format!(
                                            "Successfully extracted AppDir for {}",
                                            updated.name
                                        )));
                                        r_fn_c();
                                    }
                                    Err(e) => {
                                        toast_overlay_c.add_toast(Toast::new(&format!(
                                            "Extraction failed: {}",
                                            e
                                        )));
                                        let is_ext = {
                                            let st = state_c.borrow();
                                            st.apps
                                                .iter()
                                                .find(|a| a.id == id)
                                                .map_or(false, |a| a.is_extracted())
                                        };
                                        btn_clone.set_label(if is_ext {
                                            "Re-extract AppDir"
                                        } else {
                                            "Extract AppDir"
                                        });
                                    }
                                }
                            }
                        });
                    }
                });
        }

        // Connect Details: Compute SHA-256 Checksum Button
        {
            let state = Rc::clone(&state);
            let repo = Arc::clone(&repo);
            let toast_overlay = toast_overlay.clone();
            let app_details_widget = Rc::clone(&app_details_widget);
            let compute_btn = app_details_widget.borrow().compute_hash_btn.clone();

            compute_btn.connect_clicked(move |btn| {
                let selected_app = {
                    let st = state.borrow();
                    st.selected_id
                        .as_ref()
                        .and_then(|id| st.apps.iter().find(|a| &a.id == id).cloned())
                };

                if let Some(app) = selected_app {
                    let details = app_details_widget.borrow();
                    details
                        .hash_row
                        .set_subtitle("Computing SHA-256 checksum in background...");
                    btn.set_sensitive(false);

                    let app_clone = app.clone();
                    let toast_overlay_c = toast_overlay.clone();
                    let btn_clone = btn.clone();
                    let state_c = Rc::clone(&state);
                    let repo_c = Arc::clone(&repo);
                    let app_details_w_c = Rc::clone(&app_details_widget);
                    let target_id = app.id.clone();

                    let (tx, rx) = async_channel::bounded(1);
                    std::thread::spawn(move || {
                        let hash_opt = app_clone.calculate_sha256();
                        let _ = tx.send_blocking(hash_opt);
                    });

                    glib::spawn_future_local(async move {
                        if let Ok(hash_opt) = rx.recv().await {
                            btn_clone.set_sensitive(true);
                            if let Some(hash) = hash_opt {
                                {
                                    let mut st = state_c.borrow_mut();
                                    if let Some(stored) =
                                        st.apps.iter_mut().find(|a| a.id == target_id)
                                    {
                                        stored.sha256 = Some(hash.clone());
                                        let _ = repo_c.save(stored);
                                    }
                                }

                                let is_current =
                                    { state_c.borrow().selected_id.as_deref() == Some(&target_id) };
                                if is_current {
                                    let details = app_details_w_c.borrow();
                                    details.hash_row.set_subtitle(&hash);
                                    details.copy_hash_btn.set_visible(true);
                                    btn_clone.set_tooltip_text(Some("Recompute SHA-256 hash"));
                                }
                                toast_overlay_c
                                    .add_toast(Toast::new("SHA-256 checksum calculated and saved"));
                            } else {
                                let is_current =
                                    { state_c.borrow().selected_id.as_deref() == Some(&target_id) };
                                if is_current {
                                    app_details_w_c
                                        .borrow()
                                        .hash_row
                                        .set_subtitle("Failed to calculate checksum");
                                }
                                toast_overlay_c
                                    .add_toast(Toast::new("Failed to compute SHA-256 hash"));
                            }
                        }
                    });
                }
            });
        }

        // Connect Details: Copy SHA-256 Checksum Button
        {
            let state = Rc::clone(&state);
            let toast_overlay = toast_overlay.clone();
            let window_clone = window.clone();
            app_details_widget
                .borrow()
                .copy_hash_btn
                .connect_clicked(move |_| {
                    let sha256_val = {
                        let st = state.borrow();
                        st.selected_id.as_ref().and_then(|id| {
                            st.apps
                                .iter()
                                .find(|a| &a.id == id)
                                .and_then(|a| a.sha256.clone())
                        })
                    };

                    if let Some(hash) = sha256_val {
                        let display = gtk4::prelude::WidgetExt::display(&window_clone);
                        let clipboard = display.clipboard();
                        clipboard.set_text(&hash);
                        toast_overlay.add_toast(Toast::new("SHA-256 copied to clipboard"));
                    }
                });
        }

        // Connect Details: Desktop Integration Switch
        {
            let state = Rc::clone(&state);
            let update_cmd = Arc::clone(&update_cmd);
            let toast_overlay = toast_overlay.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let is_updating = Rc::clone(&is_updating);

            app_details_widget
                .borrow()
                .desktop_switch
                .connect_active_notify(move |sw| {
                    if is_updating.get() {
                        return;
                    }

                    let selected_id = { state.borrow().selected_id.clone() };

                    if let Some(id) = selected_id {
                        let active = sw.is_active();
                        if let Ok(updated) = update_cmd.set_desktop_integration(&id, active) {
                            let msg = if active {
                                format!("Added {} to desktop menus", updated.name)
                            } else {
                                format!("Removed {} from desktop menus", updated.name)
                            };
                            toast_overlay.add_toast(Toast::new(&msg));
                            r_fn();
                        }
                    }
                });
        }

        // Connect Details: Runtime Method Combo
        {
            let state = Rc::clone(&state);
            let update_cmd = Arc::clone(&update_cmd);
            let toast_overlay = toast_overlay.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let is_updating = Rc::clone(&is_updating);

            app_details_widget
                .borrow()
                .runtime_combo
                .connect_selected_notify(move |combo| {
                    if is_updating.get() {
                        return;
                    }

                    let selected_id = { state.borrow().selected_id.clone() };

                    if let Some(id) = selected_id {
                        let method = match combo.selected() {
                            0 => RuntimeMethod::Auto,
                            1 => RuntimeMethod::Extracted,
                            2 => RuntimeMethod::Native,
                            3 => RuntimeMethod::Fuse,
                            _ => RuntimeMethod::Auto,
                        };
                        if let Ok(updated) = update_cmd.set_runtime_method(&id, method) {
                            toast_overlay.add_toast(Toast::new(&format!(
                                "Execution method set to {}",
                                updated.runtime_method
                            )));
                            r_fn();
                        }
                    }
                });
        }

        // Connect Details: Uninstall Button
        {
            let state = Rc::clone(&state);
            let remove_cmd = Arc::clone(&remove_cmd);
            let toast_overlay = toast_overlay.clone();
            let window_clone = window.clone();
            let r_fn = Rc::clone(&refresh_list_fn);

            app_details_widget.borrow().uninstall_btn.connect_clicked(move |_| {
                let selected = {
                    let st = state.borrow();
                    st.selected_id.as_ref().and_then(|id| {
                        st.apps.iter().find(|a| &a.id == id).map(|a| (id.clone(), a.name.clone()))
                    })
                };

                if let Some((app_id, app_name)) = selected {
                    let dialog = AlertDialog::builder()
                        .heading(&format!("Uninstall {}?", app_name))
                        .body("This will remove the application, its desktop shortcuts, and extracted files.")
                        .build();

                    dialog.add_response("cancel", "Cancel");
                    dialog.add_response("remove", "Uninstall");
                    dialog.set_response_appearance("remove", libadwaita::ResponseAppearance::Destructive);

                    let remove_cmd_clone = Arc::clone(&remove_cmd);
                    let toast_overlay_clone = toast_overlay.clone();
                    let r_fn_clone = Rc::clone(&r_fn);
                    let state_clone = Rc::clone(&state);

                    dialog.choose(
                        Some(&window_clone),
                        None::<&gio::Cancellable>,
                        move |response| {
                            if response == "remove" {
                                match remove_cmd_clone.execute(&app_id, true) {
                                    Ok(_) => {
                                        toast_overlay_clone.add_toast(Toast::new(&format!(
                                            "Uninstalled {}",
                                            app_name
                                        )));
                                        state_clone.borrow_mut().selected_id = None;
                                        r_fn_clone();
                                    }
                                    Err(e) => {
                                        toast_overlay_clone.add_toast(Toast::new(&format!(
                                            "Failed to uninstall {}: {}",
                                            app_name, e
                                        )));
                                    }
                                }
                            }
                        },
                    );
                }
            });
        }

        // Shared Add AppImage logic (File picker with background import)
        let handle_add = {
            let window_clone = window.clone();
            let import_cmd = Arc::clone(&import_cmd);
            let config_store = Arc::clone(&config_store);
            let toast_overlay = toast_overlay.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let state = Rc::clone(&state);

            Rc::new(move || {
                let chooser = FileChooserNative::builder()
                    .title("Select an AppImage")
                    .action(FileChooserAction::Open)
                    .transient_for(&window_clone)
                    .build();

                let filter = FileFilter::new();
                filter.set_name(Some("AppImage Executables"));
                filter.add_pattern("*.AppImage");
                filter.add_pattern("*.appimage");
                chooser.add_filter(&filter);

                let import_cmd_c = Arc::clone(&import_cmd);
                let config_store_c = Arc::clone(&config_store);
                let toast_c = toast_overlay.clone();
                let r_fn_c = Rc::clone(&r_fn);
                let state_c = Rc::clone(&state);

                chooser.connect_response(move |dialog, response| {
                    if response == ResponseType::Accept {
                        if let Some(file) = dialog.file() {
                            if let Some(path) = file.path() {
                                toast_c
                                    .add_toast(Toast::new("Importing AppImage in background..."));

                                let import_cmd_inner = Arc::clone(&import_cmd_c);
                                let config_store_inner = Arc::clone(&config_store_c);
                                let toast_inner = toast_c.clone();
                                let r_fn_inner = Rc::clone(&r_fn_c);
                                let state_inner = Rc::clone(&state_c);

                                let (tx, rx) = async_channel::bounded(1);
                                std::thread::spawn(move || {
                                    let settings = config_store_inner.load();
                                    let res = import_cmd_inner.execute(&path, &settings);
                                    let _ = tx.send_blocking(res);
                                });

                                glib::spawn_future_local(async move {
                                    if let Ok(res) = rx.recv().await {
                                        match res {
                                            Ok(app) => {
                                                toast_inner.add_toast(Toast::new(&format!(
                                                    "Successfully imported {}",
                                                    app.name
                                                )));
                                                state_inner.borrow_mut().selected_id = Some(app.id);
                                                r_fn_inner();
                                            }
                                            Err(e) => {
                                                toast_inner.add_toast(Toast::new(&format!(
                                                    "Import error: {}",
                                                    e
                                                )));
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                    dialog.destroy();
                });

                chooser.show();
            })
        };

        // Wire Add buttons
        let h1 = Rc::clone(&handle_add);
        add_btn.connect_clicked(move |_| {
            h1();
        });

        let h2 = Rc::clone(&handle_add);
        app_list_widget
            .borrow()
            .add_first_btn
            .connect_clicked(move |_| {
                h2();
            });

        // Register Global Keyboard Actions
        // win.add
        let add_action = SimpleAction::new("add", None);
        let h_add_act = Rc::clone(&handle_add);
        add_action.connect_activate(move |_, _| {
            h_add_act();
        });
        window.add_action(&add_action);

        // win.refresh
        let refresh_action = SimpleAction::new("refresh", None);
        let r_fn_act = Rc::clone(&refresh_list_fn);
        refresh_action.connect_activate(move |_, _| {
            r_fn_act();
        });
        window.add_action(&refresh_action);

        // win.search
        let search_action = SimpleAction::new("search", None);
        let app_list_w_search = Rc::clone(&app_list_widget);
        search_action.connect_activate(move |_, _| {
            app_list_w_search.borrow().search_entry.grab_focus();
        });
        window.add_action(&search_action);

        // win.close
        let close_action = SimpleAction::new("close", None);
        let win_weak_close = win_weak.clone();
        close_action.connect_activate(move |_, _| {
            if let Some(w) = win_weak_close.upgrade() {
                w.close();
            }
        });
        window.add_action(&close_action);

        // Drag and Drop support
        {
            let import_cmd = Arc::clone(&import_cmd);
            let config_store = Arc::clone(&config_store);
            let toast_overlay = toast_overlay.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let state = Rc::clone(&state);

            let drop_target = DropTarget::new(glib::types::Type::INVALID, gdk::DragAction::COPY);
            drop_target.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);

            drop_target.connect_drop(move |_, value, _x, _y| {
                let mut paths = Vec::new();
                if let Ok(file_list) = value.get::<gdk::FileList>() {
                    for f in file_list.files() {
                        if let Some(p) = f.path() {
                            paths.push(p);
                        }
                    }
                } else if let Ok(f) = value.get::<gio::File>() {
                    if let Some(p) = f.path() {
                        paths.push(p);
                    }
                }

                paths.retain(|p| {
                    p.is_file()
                        && (p
                            .extension()
                            .map_or(false, |ext| ext.eq_ignore_ascii_case("appimage"))
                            || AppImageValidator::validate(p).is_ok())
                });

                if paths.is_empty() {
                    return false;
                }

                let import_cmd_c = Arc::clone(&import_cmd);
                let config_store_c = Arc::clone(&config_store);
                let toast_c = toast_overlay.clone();
                let r_fn_c = Rc::clone(&r_fn);
                let state_c = Rc::clone(&state);

                toast_c.add_toast(Toast::new("Importing dropped AppImage(s)..."));

                let (tx, rx) = async_channel::bounded(1);
                std::thread::spawn(move || {
                    let settings = config_store_c.load();
                    let mut imported_names = Vec::new();
                    let mut errors = Vec::new();
                    let mut last_id = None;

                    for p in paths {
                        match import_cmd_c.execute(&p, &settings) {
                            Ok(app) => {
                                last_id = Some(app.id.clone());
                                imported_names.push(app.name);
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "{}: {}",
                                    p.file_name().unwrap_or_default().to_string_lossy(),
                                    e
                                ));
                            }
                        }
                    }

                    let _ = tx.send_blocking((imported_names, errors, last_id));
                });

                glib::spawn_future_local(async move {
                    if let Ok((imported_names, errors, last_id)) = rx.recv().await {
                        if !imported_names.is_empty() {
                            if imported_names.len() == 1 {
                                toast_c.add_toast(Toast::new(&format!(
                                    "Imported {}",
                                    imported_names[0]
                                )));
                            } else {
                                toast_c.add_toast(Toast::new(&format!(
                                    "Imported {} applications",
                                    imported_names.len()
                                )));
                            }
                            if let Some(id) = last_id {
                                state_c.borrow_mut().selected_id = Some(id);
                            }
                            r_fn_c();
                        }
                        if !errors.is_empty() {
                            toast_c.add_toast(Toast::new(&format!(
                                "Import failed: {}",
                                errors.join("; ")
                            )));
                        }
                    }
                });

                true
            });

            window.add_controller(drop_target);
        }

        // Initial populate
        refresh_list_fn();

        Self { window }
    }
}
