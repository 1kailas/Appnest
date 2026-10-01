use crate::domain::appimage::{AppImage, RuntimeMethod};
use gtk4::prelude::*;
use gtk4::{Align, Box, Button, Image, Label, Orientation, ScrolledWindow, StringList};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, ComboRow, PreferencesGroup, SwitchRow};

pub struct AppDetailsWidget {
    pub container: ScrolledWindow,
    pub icon_image: Image,
    pub name_label: Label,
    pub version_label: Label,
    pub description_label: Label,
    pub status_badge: Label,
    pub arch_badge: Label,
    pub type_badge: Label,
    pub extracted_badge: Label,
    pub launch_btn: Button,
    pub launch_content: libadwaita::ButtonContent,
    pub reveal_btn: Button,
    pub extract_btn: Button,
    pub uninstall_btn: Button,
    pub path_row: ActionRow,
    pub size_row: ActionRow,
    pub date_row: ActionRow,
    pub categories_row: ActionRow,
    pub cmd_preview_row: ActionRow,
    pub hash_row: ActionRow,
    pub runtime_combo: ComboRow,
    pub desktop_switch: SwitchRow,
    pub copy_path_btn: Button,
    pub compute_hash_btn: Button,
    pub copy_hash_btn: Button,
    pub current_app_id: Option<String>,
    pub is_running: std::cell::Cell<bool>,
}

impl AppDetailsWidget {
    pub fn new() -> Self {
        let root_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();

        // Header section (Icon + Title + Badges)
        let header_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(18)
            .valign(Align::Center)
            .build();

        let icon_image = Image::builder()
            .icon_name("application-x-executable")
            .pixel_size(72)
            .build();
        icon_image.add_css_class("app-icon-large");
        header_box.append(&icon_image);

        let title_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(Align::Center)
            .build();

        let name_label = Label::builder()
            .label("Select an Application")
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .build();
        name_label.add_css_class("title-1");

        let version_label = Label::builder()
            .label("")
            .xalign(0.0)
            .single_line_mode(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        version_label.add_css_class("dim-label");

        let badges_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .margin_top(4)
            .build();

        let status_badge = Label::builder().label("Ready").build();
        status_badge.add_css_class("pill-badge");
        status_badge.add_css_class("badge-neutral");

        let arch_badge = Label::builder().build();
        arch_badge.add_css_class("pill-badge");
        arch_badge.add_css_class("badge-info");

        let type_badge = Label::builder().build();
        type_badge.add_css_class("pill-badge");
        type_badge.add_css_class("badge-success");

        let extracted_badge = Label::builder().build();
        extracted_badge.add_css_class("pill-badge");
        extracted_badge.add_css_class("badge-warning");
        extracted_badge.set_visible(false);

        badges_box.append(&status_badge);
        badges_box.append(&arch_badge);
        badges_box.append(&type_badge);
        badges_box.append(&extracted_badge);

        let description_label = Label::builder()
            .label("")
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .margin_top(4)
            .visible(false)
            .build();
        description_label.add_css_class("dim-label");

        title_box.append(&name_label);
        title_box.append(&version_label);
        title_box.append(&badges_box);
        title_box.append(&description_label);
        header_box.append(&title_box);
        root_box.append(&header_box);

        // Actions Row (Launch, Reveal in Files, Uninstall)
        let actions_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(10)
            .margin_top(8)
            .build();

        let launch_content = libadwaita::ButtonContent::builder()
            .label("Launch")
            .icon_name("media-playback-start-symbolic")
            .build();
        let launch_btn = Button::builder()
            .child(&launch_content)
            .hexpand(true)
            .build();
        launch_btn.add_css_class("suggested-action");
        launch_btn.add_css_class("pill");

        let reveal_content = libadwaita::ButtonContent::builder()
            .label("Reveal in Files")
            .icon_name("folder-open-symbolic")
            .build();
        let reveal_btn = Button::builder()
            .child(&reveal_content)
            .tooltip_text("Open folder in file manager")
            .build();
        reveal_btn.add_css_class("pill");

        let uninstall_content = libadwaita::ButtonContent::builder()
            .label("Remove")
            .icon_name("user-trash-symbolic")
            .build();
        let uninstall_btn = Button::builder()
            .child(&uninstall_content)
            .tooltip_text("Remove application and shortcuts")
            .build();
        uninstall_btn.add_css_class("destructive-action");
        uninstall_btn.add_css_class("pill");

        actions_box.append(&launch_btn);
        actions_box.append(&reveal_btn);
        actions_box.append(&uninstall_btn);
        root_box.append(&actions_box);

        // Group 1: Details & Storage
        let details_group = PreferencesGroup::builder()
            .title("Application Details &amp; Storage")
            .margin_top(12)
            .build();

        // Path row
        let path_row = ActionRow::builder()
            .title("File Location")
            .subtitle_lines(2)
            .build();
        let copy_path_btn = Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(Align::Center)
            .tooltip_text("Copy file path to clipboard")
            .build();
        copy_path_btn.add_css_class("flat");
        path_row.add_suffix(&copy_path_btn);
        details_group.add(&path_row);

        // File size row
        let size_row = ActionRow::builder().title("Size on Disk").build();
        details_group.add(&size_row);

        // Installation date row
        let date_row = ActionRow::builder().title("Installed").build();
        details_group.add(&date_row);

        // Categories row
        let categories_row = ActionRow::builder().title("Categories").build();
        details_group.add(&categories_row);

        root_box.append(&details_group);

        // Group 2: Execution & Desktop
        let exec_group = PreferencesGroup::builder()
            .title("Execution &amp; Desktop Integration")
            .margin_top(12)
            .build();

        // Runtime method combo
        let runtime_methods = StringList::new(&[
            "Auto (Adaptive)",
            "Extracted AppRun",
            "Direct Native",
            "FUSE / Extract-and-run",
        ]);
        let runtime_combo = ComboRow::builder()
            .title("Launch Method")
            .subtitle("Execution strategy for running the AppImage")
            .model(&runtime_methods)
            .build();
        exec_group.add(&runtime_combo);

        // Desktop integration switch
        let desktop_switch = SwitchRow::builder()
            .title("Desktop Integration")
            .subtitle("Create desktop shortcut and menu launcher")
            .build();
        exec_group.add(&desktop_switch);

        // AppDir extraction row
        let extract_row = ActionRow::builder()
            .title("Extracted AppDir")
            .subtitle("Extract contents for FUSE-free execution")
            .subtitle_lines(2)
            .build();
        let extract_btn = Button::builder()
            .label("Extract AppDir")
            .icon_name("system-run-symbolic")
            .valign(Align::Center)
            .build();
        extract_btn.add_css_class("pill");
        extract_row.add_suffix(&extract_btn);
        exec_group.add(&extract_row);

        // Command Preview row
        let cmd_preview_row = ActionRow::builder()
            .title("Execution Command")
            .subtitle_lines(2)
            .build();
        cmd_preview_row.add_css_class("code-row");
        exec_group.add(&cmd_preview_row);

        root_box.append(&exec_group);

        // Group 3: Integrity & Security
        let sec_group = PreferencesGroup::builder()
            .title("Integrity &amp; Security")
            .margin_top(12)
            .build();

        // SHA-256 Checksum row
        let hash_row = ActionRow::builder()
            .title("SHA-256 Checksum")
            .subtitle("Click compute to verify file integrity")
            .subtitle_lines(2)
            .build();
        hash_row.add_css_class("code-row");
        let compute_hash_btn = Button::builder()
            .icon_name("system-search-symbolic")
            .valign(Align::Center)
            .tooltip_text("Compute SHA-256 hash")
            .build();
        compute_hash_btn.add_css_class("flat");

        let copy_hash_btn = Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(Align::Center)
            .tooltip_text("Copy SHA-256 to clipboard")
            .visible(false)
            .build();
        copy_hash_btn.add_css_class("flat");

        hash_row.add_suffix(&compute_hash_btn);
        hash_row.add_suffix(&copy_hash_btn);
        sec_group.add(&hash_row);

        root_box.append(&sec_group);

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&root_box)
            .hexpand(true)
            .vexpand(true)
            .build();

        Self {
            container: scrolled,
            icon_image,
            name_label,
            version_label,
            description_label,
            status_badge,
            arch_badge,
            type_badge,
            extracted_badge,
            launch_btn,
            launch_content,
            reveal_btn,
            extract_btn,
            uninstall_btn,
            path_row,
            size_row,
            date_row,
            categories_row,
            cmd_preview_row,
            hash_row,
            runtime_combo,
            desktop_switch,
            copy_path_btn,
            compute_hash_btn,
            copy_hash_btn,
            current_app_id: None,
            is_running: std::cell::Cell::new(false),
        }
    }

    pub fn update(&mut self, app: &AppImage) {
        self.current_app_id = Some(app.id.clone());

        // Update Icon (file first, then metadata icon name, then default)
        self.icon_image.clear();
        let mut loaded = false;
        if let Some(ref icon_path) = app.icon_path {
            if icon_path.exists() {
                self.icon_image.set_from_file(Some(icon_path));
                self.icon_image.set_pixel_size(72);
                loaded = true;
            }
        }
        if !loaded {
            if let Some(ref icon_name) = app.metadata.icon_name {
                self.icon_image.set_icon_name(Some(icon_name));
                self.icon_image.set_pixel_size(72);
                loaded = true;
            }
        }
        if !loaded {
            self.icon_image
                .set_icon_name(Some("application-x-executable"));
            self.icon_image.set_pixel_size(72);
        }

        // Update Labels
        self.name_label.set_label(&app.name);

        let ver_str = match &app.version {
            Some(v) => format!("v{} • {}", v, app.formatted_size()),
            None => app.formatted_size(),
        };
        self.version_label.set_label(&ver_str);

        if let Some(ref comment) = app.metadata.comment {
            let trimmed = comment.trim();
            if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(&app.name) {
                self.description_label.set_label(trimmed);
                self.description_label.set_visible(true);
            } else {
                self.description_label.set_visible(false);
            }
        } else {
            self.description_label.set_visible(false);
        }

        // Update Badges
        self.arch_badge.set_label(&app.architecture.to_string());
        self.type_badge.set_label(&app.appimage_type.to_string());

        if app.is_extracted() {
            self.extracted_badge.set_label("Extracted (AppRun)");
            self.extracted_badge.set_visible(true);
        } else {
            self.extracted_badge.set_visible(false);
        }

        // Update Details Rows
        self.path_row.set_subtitle(&*app.path.to_string_lossy());
        self.size_row.set_subtitle(&app.formatted_size());
        self.date_row.set_subtitle(&app.formatted_date());

        let cats = if app.metadata.categories.is_empty() {
            "Utility".to_string()
        } else {
            app.metadata.categories.join(", ")
        };
        self.categories_row.set_subtitle(&cats);

        self.cmd_preview_row.set_subtitle(&app.command_preview());

        // Update Checksum
        if let Some(ref hash) = app.sha256 {
            self.hash_row.set_subtitle(hash);
            self.copy_hash_btn.set_visible(true);
            self.compute_hash_btn
                .set_tooltip_text(Some("Recompute SHA-256 hash"));
        } else {
            self.hash_row
                .set_subtitle("Click compute to verify file integrity");
            self.copy_hash_btn.set_visible(false);
            self.compute_hash_btn
                .set_tooltip_text(Some("Compute SHA-256 hash"));
        }

        // Update Combo selection
        let idx = match app.runtime_method {
            RuntimeMethod::Auto => 0,
            RuntimeMethod::Extracted => 1,
            RuntimeMethod::Native => 2,
            RuntimeMethod::Fuse => 3,
        };
        self.runtime_combo.set_selected(idx);

        // Update Switch
        self.desktop_switch.set_active(app.desktop_integrated);

        // Update Extract button text
        if app.is_extracted() {
            self.extract_btn.set_label("Re-extract AppDir");
        } else {
            self.extract_btn.set_label("Extract AppDir");
        }
    }

    pub fn set_running_state(&self, running: bool) {
        self.is_running.set(running);
        if running {
            self.status_badge.set_label("Running");
            self.status_badge.remove_css_class("badge-neutral");
            self.status_badge.add_css_class("badge-running");

            self.launch_content.set_label("Close Application");
            self.launch_content.set_icon_name("window-close-symbolic");
            self.launch_btn.remove_css_class("suggested-action");
            self.launch_btn.add_css_class("destructive-action");
            self.launch_btn
                .set_tooltip_text(Some("Close running application"));
        } else {
            self.status_badge.set_label("Ready");
            self.status_badge.remove_css_class("badge-running");
            self.status_badge.add_css_class("badge-neutral");

            self.launch_content.set_label("Launch");
            self.launch_content
                .set_icon_name("media-playback-start-symbolic");
            self.launch_btn.remove_css_class("destructive-action");
            self.launch_btn.add_css_class("suggested-action");
            self.launch_btn.set_tooltip_text(Some("Launch application"));
        }
    }
}
