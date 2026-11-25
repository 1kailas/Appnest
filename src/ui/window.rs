use crate::application::commands::{
    CloseAppImageCommand, ImportAppImageCommand, LaunchAppImageCommand, RemoveAppImageCommand,
    UpdateAppImageCommand,
};
use crate::application::queries::{AppImagesListing, ListAppImagesQuery};
use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, RuntimeMethod};
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::config_store::ConfigStore;
use crate::infrastructure::hash::sha256_file;
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
    Box, Button, DropTarget, FileChooserAction, FileChooserNative, FileFilter, MenuButton,
    Orientation, Paned, ResponseType, Stack,
};
use libadwaita::prelude::*;
use libadwaita::{
    AboutDialog, AlertDialog, Application, ApplicationWindow, HeaderBar, StatusPage, Toast,
    ToastOverlay, WindowTitle,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

struct ControllerState {
    apps: Vec<AppImage>,
    filtered_apps: Vec<AppImage>,
    unmanaged: Vec<PathBuf>,
    selected_id: Option<String>,
    row_widgets: HashMap<String, Rc<AppRowWidget>>,
    running: HashSet<String>,
}

type RefreshHandle = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

struct Snapshot {
    listing: AppImagesListing,
    running: HashSet<String>,
}

fn subtitle_text(total: usize, running: usize) -> String {
    let plural = if total == 1 { "" } else { "s" };
    match (total, running) {
        (0, _) => "No Applications".to_string(),
        (_, 0) => format!("{} Application{}", total, plural),
        _ => format!("{} Application{} • {} Running", total, plural, running),
    }
}

pub struct MainWindow {
    pub window: ApplicationWindow,
    prompt_open_fn: Rc<dyn Fn(PathBuf)>,
}

impl MainWindow {
    pub fn prompt_open_file(&self, path: PathBuf) {
        (self.prompt_open_fn)(path);
    }

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
            running: HashSet::new(),
        }));

        // Prevents signal handler loops when programmatically updating widgets
        let is_updating = Rc::new(Cell::new(false));

        let toast_overlay = ToastOverlay::new();

        // Main vertical layout
        let main_box = Box::builder().orientation(Orientation::Vertical).build();

        // HeaderBar
        let header_bar = HeaderBar::builder().build();
        let title_widget = WindowTitle::new("AppNest", "No Applications");
        header_bar.set_title_widget(Some(&title_widget));

        // Add button
        let add_btn = Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add AppImage (Ctrl+O)")
            .build();
        header_bar.pack_start(&add_btn);

        // Refresh button
        let refresh_btn = Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Refresh Application List (Ctrl+R)")
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
            .resize_start_child(false)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(true)
            .hexpand(true)
            .vexpand(true)
            .build();
        paned.set_position(300);

        let app_list_widget = Rc::new(RefCell::new(AppListWidget::new()));
        app_list_widget.borrow().container.set_size_request(280, -1);
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
        window.set_size_request(760, 480);

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
                    .version(env!("CARGO_PKG_VERSION"))
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

        // Starts or stops an application on a worker thread (both can take a moment) and
        // reflects the result in the list and details views.
        let toggle_app_fn: Rc<dyn Fn(String, String)> = {
            let state = Rc::clone(&state);
            let launch_cmd = Arc::clone(&launch_cmd);
            let close_cmd = Arc::clone(&close_cmd);
            let toast_overlay = toast_overlay.clone();
            let app_details_widget = Rc::clone(&app_details_widget);
            let busy: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));

            Rc::new(move |app_id: String, app_name: String| {
                if !busy.borrow_mut().insert(app_id.clone()) {
                    return;
                }
                let closing = state.borrow().running.contains(&app_id);

                let (tx, rx) = async_channel::bounded(1);
                {
                    let launch_cmd = Arc::clone(&launch_cmd);
                    let close_cmd = Arc::clone(&close_cmd);
                    let id = app_id.clone();
                    std::thread::spawn(move || {
                        let res = if closing {
                            close_cmd.execute(&id).map(|_| 0)
                        } else {
                            launch_cmd.execute(&id, &[])
                        };
                        let _ = tx.send_blocking(res);
                    });
                }

                let state = Rc::clone(&state);
                let toast_overlay = toast_overlay.clone();
                let details = Rc::clone(&app_details_widget);
                let busy = Rc::clone(&busy);
                glib::spawn_future_local(async move {
                    let Ok(res) = rx.recv().await else { return };
                    busy.borrow_mut().remove(&app_id);

                    let set_running = |running: bool| {
                        let mut st = state.borrow_mut();
                        if running {
                            st.running.insert(app_id.clone());
                        } else {
                            st.running.remove(&app_id);
                        }
                        if let Some(row) = st.row_widgets.get(&app_id) {
                            row.set_running_state(running);
                        }
                        if st.selected_id.as_deref() == Some(app_id.as_str()) {
                            details.borrow().set_running_state(running);
                        }
                    };

                    let message = match (closing, res) {
                        (true, Ok(_)) => {
                            set_running(false);
                            format!("Closed {}", app_name)
                        }
                        (true, Err(e)) => format!("Failed to close {}: {}", app_name, e),
                        (false, Ok(_)) => {
                            set_running(true);
                            format!("Launched {}", app_name)
                        }
                        (false, Err(e)) => format!("Failed to launch {}: {}", app_name, e),
                    };
                    toast_overlay.add_toast(Toast::new(&message));
                });
            })
        };

        // Rebuilds the list and details from a snapshot gathered on a worker thread
        let render_fn: Rc<dyn Fn(Snapshot)> = {
            let state = Rc::clone(&state);
            let app_list_widget = Rc::clone(&app_list_widget);
            let app_details_widget = Rc::clone(&app_details_widget);
            let right_stack = right_stack.clone();
            let is_updating = Rc::clone(&is_updating);
            let title_widget_clone = title_widget.clone();
            let toggle_app_fn = Rc::clone(&toggle_app_fn);

            Rc::new(move |snapshot: Snapshot| {
                let Snapshot { listing, running } = snapshot;

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
                    st.running = running.clone();
                }

                // Clear installed listbox safely
                let listbox = &app_list_widget.borrow().installed_list;
                while let Some(row) = listbox.row_at_index(0) {
                    listbox.remove(&row);
                }

                let mut row_map = HashMap::new();
                for app in &filtered_apps {
                    let row_widget = Rc::new(AppRowWidget::new(app));
                    row_widget.set_running_state(running.contains(&app.id));

                    let toggle = Rc::clone(&toggle_app_fn);
                    let (app_id, app_name) = (app.id.clone(), app.name.clone());
                    let listbox_weak = listbox.downgrade();
                    let row_weak = row_widget.row.downgrade();
                    row_widget.launch_button.connect_clicked(move |_| {
                        if let (Some(lb), Some(rw)) = (listbox_weak.upgrade(), row_weak.upgrade()) {
                            lb.select_row(Some(&rw));
                        }
                        toggle(app_id.clone(), app_name.clone());
                    });

                    row_map.insert(app.id.clone(), Rc::clone(&row_widget));
                    listbox.append(&row_widget.row);
                }
                state.borrow_mut().row_widgets = row_map;

                app_list_widget
                    .borrow()
                    .set_empty_state(!filtered_apps.is_empty(), !query.is_empty());

                let running_count = filtered_apps
                    .iter()
                    .filter(|a| running.contains(&a.id))
                    .count();
                title_widget_clone.set_subtitle(&subtitle_text(filtered_apps.len(), running_count));

                // If the selected app is still present, show it again
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
                        .set_running_state(running.contains(&found.id));
                    is_updating.set(false);
                    right_stack.set_visible_child_name("details");

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

        // Gathers the library listing, scans for running applications and extracts missing
        // icons on one worker thread. Requests made while a refresh is running are merged
        // into a single follow-up refresh.
        let refresh_self: RefreshHandle = Rc::new(RefCell::new(None));
        let refresh_list_fn: Rc<dyn Fn()> = {
            let list_query = Arc::clone(&list_query);
            let launcher = Arc::clone(&launcher);
            let repo = Arc::clone(&repo);
            let paths = paths.clone();
            let render_fn = Rc::clone(&render_fn);
            let refresh_self = Rc::clone(&refresh_self);
            let in_flight = Rc::new(Cell::new(false));
            let pending = Rc::new(Cell::new(false));
            let icon_attempted: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

            Rc::new(move || {
                if in_flight.replace(true) {
                    pending.set(true);
                    return;
                }

                let (tx, rx) = async_channel::unbounded::<Snapshot>();
                {
                    let list_query = Arc::clone(&list_query);
                    let launcher = Arc::clone(&launcher);
                    let repo = Arc::clone(&repo);
                    let paths = paths.clone();
                    let attempted = Arc::clone(&icon_attempted);

                    std::thread::spawn(move || {
                        // Discovery of unmanaged AppImages is not shown in the UI, so the
                        // refresh does not scan the disk for them.
                        let listing = list_query.execute(&[]).unwrap_or(AppImagesListing {
                            managed: Vec::new(),
                            unmanaged_discovered: Vec::new(),
                        });
                        let running = launcher.refresh_running(&listing.managed);
                        let managed = listing.managed.clone();
                        let _ = tx.send_blocking(Snapshot {
                            listing,
                            running: running.clone(),
                        });

                        // Missing icons are extracted once per session, after the list is shown
                        let mut changed = false;
                        for mut app in managed {
                            let missing = app.icon_path.as_ref().is_none_or(|p| !p.exists());
                            let first_try = attempted
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(app.id.clone());
                            if !(missing && first_try && app.exists()) {
                                continue;
                            }
                            if let Some(icon) = AppImageExtractor::extract_icon_only(
                                &app.path,
                                &paths.icons_dir,
                                &app.id,
                                app.metadata.icon_name.as_deref(),
                            ) {
                                app.icon_path = Some(icon);
                                let _ = repo.save(&app);
                                if app.desktop_integrated {
                                    let _ = DesktopEntryService::create_entry(
                                        &app,
                                        &paths.desktop_applications_dir,
                                    );
                                }
                                changed = true;
                            }
                        }
                        if changed {
                            if let Ok(listing) = list_query.execute(&[]) {
                                let _ = tx.send_blocking(Snapshot { listing, running });
                            }
                        }
                    });
                }

                let render_fn = Rc::clone(&render_fn);
                let in_flight = Rc::clone(&in_flight);
                let pending = Rc::clone(&pending);
                let refresh_self = Rc::clone(&refresh_self);
                glib::spawn_future_local(async move {
                    while let Ok(snapshot) = rx.recv().await {
                        render_fn(snapshot);
                    }
                    in_flight.set(false);
                    if pending.replace(false) {
                        let again = refresh_self.borrow().clone();
                        if let Some(f) = again {
                            f();
                        }
                    }
                });
            })
        };
        *refresh_self.borrow_mut() = Some(Rc::clone(&refresh_list_fn));

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

            app_list_widget
                .borrow()
                .installed_list
                .connect_row_selected(move |_, row_opt| {
                    let Some(row) = row_opt else { return };
                    let (app_opt, running) = {
                        let st = state.borrow();
                        let app = st.filtered_apps.get(row.index() as usize).cloned();
                        let running = app.as_ref().is_some_and(|a| st.running.contains(&a.id));
                        (app, running)
                    };
                    if let Some(app) = app_opt {
                        state.borrow_mut().selected_id = Some(app.id.clone());
                        is_updating.set(true);
                        app_details_widget.borrow_mut().update(&app);
                        app_details_widget.borrow().set_running_state(running);
                        is_updating.set(false);
                        right_stack.set_visible_child_name("details");
                    }
                });
        }

        // Connect Details: Launch / Close Button
        {
            let state = Rc::clone(&state);
            let toggle = Rc::clone(&toggle_app_fn);
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
                    toggle(app_id, app_name);
                }
            });
        }

        // Periodically sync running state; /proc is scanned on a worker thread
        {
            let state = Rc::clone(&state);
            let launcher = Arc::clone(&launcher);
            let app_details_widget = Rc::clone(&app_details_widget);
            let title_widget_watcher = title_widget.clone();
            let scanning = Rc::new(Cell::new(false));

            glib::timeout_add_local(std::time::Duration::from_millis(2000), move || {
                if scanning.get() {
                    return glib::ControlFlow::Continue;
                }
                let apps = state.borrow().apps.clone();
                if apps.is_empty() {
                    return glib::ControlFlow::Continue;
                }
                scanning.set(true);

                let (tx, rx) = async_channel::bounded(1);
                let launcher = Arc::clone(&launcher);
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(launcher.refresh_running(&apps));
                });

                let state = Rc::clone(&state);
                let details = Rc::clone(&app_details_widget);
                let title = title_widget_watcher.clone();
                let scanning = Rc::clone(&scanning);
                glib::spawn_future_local(async move {
                    if let Ok(running) = rx.recv().await {
                        let mut st = state.borrow_mut();
                        for (id, row) in &st.row_widgets {
                            row.set_running_state(running.contains(id));
                        }
                        if let Some(id) = &st.selected_id {
                            details.borrow().set_running_state(running.contains(id));
                        }
                        let running_count = st
                            .row_widgets
                            .keys()
                            .filter(|id| running.contains(*id))
                            .count();
                        title.set_subtitle(&subtitle_text(st.row_widgets.len(), running_count));
                        st.running = running;
                    }
                    scanning.set(false);
                });

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
                                                .is_some_and(|a| a.is_extracted())
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
                        let hash_opt = sha256_file(&app_clone.path).ok();
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
                                        stored.set_sha256(hash.clone());
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
                                .and_then(|a| a.cached_sha256().map(str::to_string))
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
                            3 => RuntimeMethod::ExtractAndRun,
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
                        .heading(format!("Uninstall {}?", app_name))
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

        // Define integration prompt closures
        let prompt_open_fn = {
            let window_clone = window.clone();
            let toast_overlay = toast_overlay.clone();
            let import_cmd = Arc::clone(&import_cmd);
            let launcher = Arc::clone(&launcher);
            let config_store = Arc::clone(&config_store);
            let repo = Arc::clone(&repo);
            let paths = paths.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let state = Rc::clone(&state);

            Rc::new(move |path: PathBuf| {
                if !path.is_file() {
                    toast_overlay
                        .add_toast(Toast::new(&format!("File not found: {}", path.display())));
                    return;
                }

                if let Err(e) = AppImageValidator::validate(&path) {
                    toast_overlay.add_toast(Toast::new(&format!(
                        "Cannot use {}: {}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        e
                    )));
                    return;
                }

                // Check if already in AppNest
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                let existing = match repo.list() {
                    Ok(list) => list.into_iter().find(|a| {
                        a.path == path
                            || a.path.canonicalize().unwrap_or_else(|_| a.path.clone()) == canonical
                    }),
                    Err(_) => None,
                };

                // If already in AppNest, just select it and present the window — no dialog, no launch
                if let Some(existing_app) = existing {
                    let has_desktop_shortcut = paths.user_desktop_dir.as_ref().is_some_and(|d| {
                        DesktopEntryService::entry_path(&existing_app, d).is_ok_and(|p| p.exists())
                    });

                    if !has_desktop_shortcut || !existing_app.desktop_integrated {
                        // Missing desktop shortcut — offer to create one (but don't auto-launch)
                        let win_dlg = window_clone.clone();
                        let toast_dlg = toast_overlay.clone();
                        let paths_dlg = paths.clone();
                        let repo_dlg = Arc::clone(&repo);
                        let r_fn_dlg = Rc::clone(&r_fn);
                        let state_dlg = Rc::clone(&state);

                        let dialog = AlertDialog::builder()
                            .heading(format!("Create Shortcut for {}?", existing_app.name))
                            .body(format!(
                                "\"{}\" is already in AppNest but doesn't have a desktop shortcut.",
                                existing_app.name
                            ))
                            .build();

                        dialog.add_response("cancel", "No Thanks");
                        dialog.add_response("shortcut", "Create Desktop Shortcut");
                        dialog.set_response_appearance(
                            "shortcut",
                            libadwaita::ResponseAppearance::Suggested,
                        );
                        dialog.set_default_response(Some("shortcut"));
                        dialog.set_close_response("cancel");

                        let app_c = existing_app.clone();
                        dialog.choose(Some(&win_dlg), None::<&gio::Cancellable>, move |resp| {
                            if resp.as_str() == "shortcut" {
                                let mut app = app_c.clone();
                                if let Ok(entry_path) = DesktopEntryService::create_entry(
                                    &app,
                                    &paths_dlg.desktop_applications_dir,
                                ) {
                                    app.desktop_entry_path = Some(entry_path);
                                    app.desktop_integrated = true;
                                    let _ = repo_dlg.save(&app);
                                }
                                let _ = DesktopEntryService::create_user_desktop_shortcut(
                                    &app,
                                    paths_dlg.user_desktop_dir.as_deref(),
                                );
                                toast_dlg.add_toast(Toast::new(&format!(
                                    "Created desktop shortcut for {}",
                                    app.name
                                )));
                            }
                            // Always select the app in AppNest regardless of choice
                            state_dlg.borrow_mut().selected_id = Some(app_c.id.clone());
                            r_fn_dlg();
                        });
                    } else {
                        // Already fully integrated — just select and show, no dialog needed
                        state.borrow_mut().selected_id = Some(existing_app.id.clone());
                        r_fn();
                        toast_overlay.add_toast(Toast::new(&format!(
                            "{} is already in AppNest",
                            existing_app.name
                        )));
                    }
                    return;
                }

                // New AppImage — show dialog to add to AppNest
                let app_name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "AppImage".to_string());

                let dialog = AlertDialog::builder()
                    .heading(format!("Add {} to AppNest?", app_name))
                    .body(format!(
                        "Would you like to add \"{}\" to AppNest and create a desktop shortcut?",
                        app_name
                    ))
                    .build();

                dialog.add_response("cancel", "Cancel");
                dialog.add_response("run_once", "Run Once");
                dialog.add_response("add_only", "Add without Shortcut");
                dialog.add_response("integrate", "Add & Create Shortcut");
                dialog.set_response_appearance(
                    "integrate",
                    libadwaita::ResponseAppearance::Suggested,
                );
                dialog.set_default_response(Some("integrate"));
                dialog.set_close_response("cancel");

                let win_dlg = window_clone.clone();
                let toast_dlg = toast_overlay.clone();
                let import_cmd_dlg = Arc::clone(&import_cmd);
                let launcher_dlg = Arc::clone(&launcher);
                let config_store_dlg = Arc::clone(&config_store);
                let repo_dlg = Arc::clone(&repo);
                let paths_dlg = paths.clone();
                let r_fn_dlg = Rc::clone(&r_fn);
                let state_dlg = Rc::clone(&state);
                let path_dlg = path.clone();
                let app_name_dlg = app_name.clone();

                dialog.choose(Some(&win_dlg), None::<&gio::Cancellable>, move |response| {
                    match response.as_str() {
                        "integrate" | "add_only" => {
                            let create_shortcut = response.as_str() == "integrate";
                            toast_dlg.add_toast(Toast::new(&format!(
                                "Adding {} to AppNest...",
                                app_name_dlg
                            )));

                            let (tx, rx) = async_channel::bounded(1);
                            let p = path_dlg.clone();
                            let import_cmd = Arc::clone(&import_cmd_dlg);
                            let config_store = Arc::clone(&config_store_dlg);

                            std::thread::spawn(move || {
                                let mut settings = config_store.load();
                                settings.auto_integrate_desktop = create_shortcut;
                                let res = import_cmd.execute(&p, &settings);
                                let _ = tx.send_blocking(res);
                            });

                            let toast_res = toast_dlg.clone();
                            let r_fn_res = Rc::clone(&r_fn_dlg);
                            let state_res = Rc::clone(&state_dlg);
                            let repo_res = Arc::clone(&repo_dlg);
                            let paths_res = paths_dlg.clone();
                            let name_res = app_name_dlg.clone();

                            glib::spawn_future_local(async move {
                                if let Ok(res) = rx.recv().await {
                                    match res {
                                        Ok(mut app) => {
                                            if create_shortcut {
                                                if !app.desktop_integrated {
                                                    if let Ok(entry_path) =
                                                        DesktopEntryService::create_entry(
                                                            &app,
                                                            &paths_res.desktop_applications_dir,
                                                        )
                                                    {
                                                        app.desktop_integrated = true;
                                                        app.desktop_entry_path = Some(entry_path);
                                                        let _ = repo_res.save(&app);
                                                    }
                                                }
                                                let _ =
                                                    DesktopEntryService::create_user_desktop_shortcut(
                                                        &app,
                                                        paths_res.user_desktop_dir.as_deref(),
                                                    );
                                            }

                                            // Auto-select the newly added app (no auto-launch)
                                            state_res.borrow_mut().selected_id =
                                                Some(app.id.clone());
                                            r_fn_res();

                                            let msg = if create_shortcut {
                                                format!(
                                                    "Added {} to AppNest with desktop shortcut",
                                                    app.name
                                                )
                                            } else {
                                                format!("Added {} to AppNest", app.name)
                                            };
                                            toast_res.add_toast(Toast::new(&msg));
                                        }
                                        Err(e) => {
                                            toast_res.add_toast(Toast::new(&format!(
                                                "Failed to add {}: {}",
                                                name_res, e
                                            )));
                                        }
                                    }
                                }
                            });
                        }
                        "run_once" => {
                            let name = path_dlg
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "AppImage".to_string());
                            let app_id = format!("run-once-{}", AppImage::sanitize_id(&name));
                            let launcher = Arc::clone(&launcher_dlg);
                            let path_run = path_dlg.clone();
                            let name_run = name.clone();

                            let (tx, rx) = async_channel::bounded(1);
                            std::thread::spawn(move || {
                                let mut temp_app = AppImage::new(&app_id, &name_run, path_run);
                                let _ = tx.send_blocking(launcher.launch(&mut temp_app, &[]));
                            });

                            let toast = toast_dlg.clone();
                            glib::spawn_future_local(async move {
                                if let Ok(res) = rx.recv().await {
                                    let message = match res {
                                        Ok(_) => format!("Running {} once", name),
                                        Err(e) => format!("Failed to run {}: {}", name, e),
                                    };
                                    toast.add_toast(Toast::new(&message));
                                }
                            });
                        }
                        _ => {}
                    }
                });
            })
        };

        let prompt_import_fn = {
            let window_clone = window.clone();
            let toast_overlay = toast_overlay.clone();
            let import_cmd = Arc::clone(&import_cmd);
            let config_store = Arc::clone(&config_store);
            let repo = Arc::clone(&repo);
            let paths = paths.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let state = Rc::clone(&state);

            Rc::new(move |path: PathBuf| {
                if !path.is_file() {
                    toast_overlay
                        .add_toast(Toast::new(&format!("File not found: {}", path.display())));
                    return;
                }

                if let Err(e) = AppImageValidator::validate(&path) {
                    toast_overlay.add_toast(Toast::new(&format!(
                        "Cannot use {}: {}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        e
                    )));
                    return;
                }

                // Check if already in AppNest
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                let existing = match repo.list() {
                    Ok(list) => list.into_iter().find(|a| {
                        a.path == path
                            || a.path.canonicalize().unwrap_or_else(|_| a.path.clone()) == canonical
                    }),
                    Err(_) => None,
                };

                if let Some(existing_app) = existing {
                    state.borrow_mut().selected_id = Some(existing_app.id);
                    r_fn();
                    toast_overlay.add_toast(Toast::new("Application is already in AppNest"));
                    return;
                }

                let app_name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "AppImage".to_string());

                let dialog = AlertDialog::builder()
                    .heading(format!("Add {} to AppNest?", app_name))
                    .body(format!(
                        "Would you like to add \"{}\" to AppNest and create a desktop shortcut?",
                        app_name
                    ))
                    .build();

                dialog.add_response("cancel", "Cancel");
                dialog.add_response("add_only", "Add without Shortcut");
                dialog.add_response("integrate", "Add & Create Shortcut");
                dialog.set_response_appearance(
                    "integrate",
                    libadwaita::ResponseAppearance::Suggested,
                );
                dialog.set_default_response(Some("integrate"));
                dialog.set_close_response("cancel");

                let win_dlg = window_clone.clone();
                let toast_dlg = toast_overlay.clone();
                let import_cmd_dlg = Arc::clone(&import_cmd);
                let config_store_dlg = Arc::clone(&config_store);
                let repo_dlg = Arc::clone(&repo);
                let paths_dlg = paths.clone();
                let r_fn_dlg = Rc::clone(&r_fn);
                let state_dlg = Rc::clone(&state);
                let path_dlg = path.clone();
                let app_name_dlg = app_name.clone();

                dialog.choose(Some(&win_dlg), None::<&gio::Cancellable>, move |response| {
                    match response.as_str() {
                        "integrate" | "add_only" => {
                            let create_shortcut = response.as_str() == "integrate";
                            toast_dlg.add_toast(Toast::new(&format!(
                                "Adding {} to AppNest...",
                                app_name_dlg
                            )));

                            let (tx, rx) = async_channel::bounded(1);
                            let p = path_dlg.clone();
                            let import_cmd = Arc::clone(&import_cmd_dlg);
                            let config_store = Arc::clone(&config_store_dlg);

                            std::thread::spawn(move || {
                                let mut settings = config_store.load();
                                settings.auto_integrate_desktop = create_shortcut;
                                let res = import_cmd.execute(&p, &settings);
                                let _ = tx.send_blocking(res);
                            });

                            let toast_res = toast_dlg.clone();
                            let r_fn_res = Rc::clone(&r_fn_dlg);
                            let state_res = Rc::clone(&state_dlg);
                            let repo_res = Arc::clone(&repo_dlg);
                            let paths_res = paths_dlg.clone();
                            let name_res = app_name_dlg.clone();

                            glib::spawn_future_local(async move {
                                if let Ok(res) = rx.recv().await {
                                    match res {
                                        Ok(mut app) => {
                                            if create_shortcut {
                                                if !app.desktop_integrated {
                                                    if let Ok(entry_path) =
                                                        DesktopEntryService::create_entry(
                                                            &app,
                                                            &paths_res.desktop_applications_dir,
                                                        )
                                                    {
                                                        app.desktop_integrated = true;
                                                        app.desktop_entry_path = Some(entry_path);
                                                        let _ = repo_res.save(&app);
                                                    }
                                                }
                                                let _ = DesktopEntryService::create_user_desktop_shortcut(
                                                    &app,
                                                    paths_res.user_desktop_dir.as_deref(),
                                                );
                                            }

                                            state_res.borrow_mut().selected_id =
                                                Some(app.id.clone());
                                            r_fn_res();

                                            let msg = if create_shortcut {
                                                format!("Added {} to AppNest and desktop", app.name)
                                            } else {
                                                format!("Added {} to AppNest", app.name)
                                            };
                                            toast_res.add_toast(Toast::new(&msg));
                                        }
                                        Err(e) => {
                                            toast_res.add_toast(Toast::new(&format!(
                                                "Failed to add {}: {}",
                                                name_res, e
                                            )));
                                        }
                                    }
                                }
                            });
                        }
                        _ => {}
                    }
                });
            })
        };

        let prompt_import_multiple_fn = {
            let window_clone = window.clone();
            let toast_overlay = toast_overlay.clone();
            let import_cmd = Arc::clone(&import_cmd);
            let config_store = Arc::clone(&config_store);
            let repo = Arc::clone(&repo);
            let paths = paths.clone();
            let r_fn = Rc::clone(&refresh_list_fn);
            let state = Rc::clone(&state);

            Rc::new(move |paths_to_add: Vec<PathBuf>| {
                let count = paths_to_add.len();
                let dialog = AlertDialog::builder()
                    .heading(format!("Add {} Applications to AppNest?", count))
                    .body("Would you like to add these applications to AppNest and create desktop shortcuts?")
                    .build();

                dialog.add_response("cancel", "Cancel");
                dialog.add_response("add_only", "Add without Shortcuts");
                dialog.add_response("integrate", "Add & Create Shortcuts");
                dialog.set_response_appearance(
                    "integrate",
                    libadwaita::ResponseAppearance::Suggested,
                );
                dialog.set_default_response(Some("integrate"));
                dialog.set_close_response("cancel");

                let win_dlg = window_clone.clone();
                let toast_dlg = toast_overlay.clone();
                let import_cmd_dlg = Arc::clone(&import_cmd);
                let config_store_dlg = Arc::clone(&config_store);
                let repo_dlg = Arc::clone(&repo);
                let paths_dlg = paths.clone();
                let r_fn_dlg = Rc::clone(&r_fn);
                let state_dlg = Rc::clone(&state);

                dialog.choose(
                    Some(&win_dlg),
                    None::<&gio::Cancellable>,
                    move |response| match response.as_str() {
                        "integrate" | "add_only" => {
                            let create_shortcuts = response.as_str() == "integrate";
                            toast_dlg.add_toast(Toast::new("Adding applications to AppNest..."));

                            let (tx, rx) = async_channel::bounded(1);
                            let import_cmd = Arc::clone(&import_cmd_dlg);
                            let config_store = Arc::clone(&config_store_dlg);
                            let paths_batch = paths_to_add.clone();

                            std::thread::spawn(move || {
                                let mut settings = config_store.load();
                                settings.auto_integrate_desktop = create_shortcuts;
                                let mut imported = Vec::new();
                                let mut errors = Vec::new();

                                for p in paths_batch {
                                    match import_cmd.execute(&p, &settings) {
                                        Ok(app) => imported.push(app),
                                        Err(e) => errors.push(format!(
                                            "{}: {}",
                                            p.file_name().unwrap_or_default().to_string_lossy(),
                                            e
                                        )),
                                    }
                                }
                                let _ = tx.send_blocking((imported, errors));
                            });

                            let toast_res = toast_dlg.clone();
                            let r_fn_res = Rc::clone(&r_fn_dlg);
                            let state_res = Rc::clone(&state_dlg);
                            let repo_res = Arc::clone(&repo_dlg);
                            let paths_res = paths_dlg.clone();

                            glib::spawn_future_local(async move {
                                if let Ok((mut imported, errors)) = rx.recv().await {
                                    if !imported.is_empty() {
                                        if create_shortcuts {
                                            for app in &mut imported {
                                                if !app.desktop_integrated {
                                                    if let Ok(entry_path) = DesktopEntryService::create_entry(
                                                        app,
                                                        &paths_res.desktop_applications_dir,
                                                    ) {
                                                        app.desktop_integrated = true;
                                                        app.desktop_entry_path = Some(entry_path);
                                                        let _ = repo_res.save(app);
                                                    }
                                                }
                                            }
                                        }

                                        if let Some(last) = imported.last() {
                                            state_res.borrow_mut().selected_id = Some(last.id.clone());
                                        }
                                        r_fn_res();

                                        let msg = if create_shortcuts {
                                            format!("Added {} applications to AppNest with desktop shortcuts", imported.len())
                                        } else {
                                            format!("Added {} applications to AppNest", imported.len())
                                        };
                                        toast_res.add_toast(Toast::new(&msg));
                                    }
                                    if !errors.is_empty() {
                                        toast_res.add_toast(Toast::new(&format!(
                                            "Some applications failed to add: {}",
                                            errors.join("; ")
                                        )));
                                    }
                                }
                            });
                        }
                        _ => {}
                    },
                );
            })
        };

        // Shared Add AppImage logic (File picker leading to integration prompt)
        let handle_add = {
            let window_clone = window.clone();
            let prompt_import = Rc::clone(&prompt_import_fn);

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

                let prompt_import_c = Rc::clone(&prompt_import);

                chooser.connect_response(move |dialog, response| {
                    if response == ResponseType::Accept {
                        if let Some(file) = dialog.file() {
                            if let Some(path) = file.path() {
                                prompt_import_c(path);
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
            let prompt_import_drop = Rc::clone(&prompt_import_fn);
            let prompt_import_mult_drop = Rc::clone(&prompt_import_multiple_fn);
            let toast_drop = toast_overlay.clone();

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

                let mut rejected = Vec::new();
                paths.retain(|p| match AppImageValidator::validate(p) {
                    Ok(_) => true,
                    Err(e) => {
                        rejected.push(format!(
                            "{}: {}",
                            p.file_name().unwrap_or_default().to_string_lossy(),
                            e
                        ));
                        false
                    }
                });
                if !rejected.is_empty() {
                    toast_drop.add_toast(Toast::new(&format!(
                        "Skipped invalid files: {}",
                        rejected.join("; ")
                    )));
                }

                if paths.is_empty() {
                    return false;
                }

                if paths.len() == 1 {
                    prompt_import_drop(paths.remove(0));
                } else {
                    prompt_import_mult_drop(paths);
                }

                true
            });

            window.add_controller(drop_target);
        }

        // Initial populate
        refresh_list_fn();

        Self {
            window,
            prompt_open_fn,
        }
    }
}
