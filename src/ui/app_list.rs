use gtk4::prelude::*;
use gtk4::{Align, Box, Button, Label, ListBox, Orientation, ScrolledWindow, SearchEntry};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, StatusPage};
use std::cell::Cell;
use std::path::PathBuf;

pub struct AppListWidget {
    pub container: Box,
    pub search_entry: SearchEntry,
    pub installed_list: ListBox,
    pub empty_status_page: StatusPage,
    pub scrolled_window: ScrolledWindow,
    pub add_first_btn: Button,
    discovered_box: Box,
    discovered_list: ListBox,
    discovered_count: Cell<usize>,
}

impl Default for AppListWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl AppListWidget {
    pub fn new() -> Self {
        let container = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        // Search Entry
        let search_entry = SearchEntry::builder()
            .placeholder_text("Search AppImages...")
            .build();
        container.append(&search_entry);

        // List box for installed apps
        let installed_list = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();
        installed_list.add_css_class("boxed-list");

        // AppImages found on disk that are not in the library yet
        let discovered_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(6)
            .visible(false)
            .build();
        let discovered_heading = Label::builder()
            .label("Found on this computer")
            .xalign(0.0)
            .build();
        discovered_heading.add_css_class("heading");
        let discovered_list = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .build();
        discovered_list.add_css_class("boxed-list");
        discovered_box.append(&discovered_heading);
        discovered_box.append(&discovered_list);

        let list_content = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(18)
            .build();
        list_content.append(&installed_list);
        list_content.append(&discovered_box);

        let scrolled_window = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&list_content)
            .vexpand(true)
            .build();

        // Empty state page
        let add_first_btn = Button::builder()
            .label("Add AppImage")
            .icon_name("list-add-symbolic")
            .halign(Align::Center)
            .build();
        add_first_btn.add_css_class("suggested-action");
        add_first_btn.add_css_class("pill");

        let empty_status_page = StatusPage::builder()
            .icon_name("application-x-executable-symbolic")
            .title("No AppImages")
            .description("Add an AppImage to organize, integrate, and launch it.")
            .child(&add_first_btn)
            .vexpand(true)
            .build();

        container.append(&empty_status_page);
        container.append(&scrolled_window);

        Self {
            container,
            search_entry,
            installed_list,
            empty_status_page,
            scrolled_window,
            add_first_btn,
            discovered_box,
            discovered_list,
            discovered_count: Cell::new(0),
        }
    }

    /// Shows AppImages found in the scan folders, each with an Add button.
    pub fn set_discovered(&self, files: &[PathBuf], on_add: impl Fn(PathBuf) + Clone + 'static) {
        while let Some(row) = self.discovered_list.row_at_index(0) {
            self.discovered_list.remove(&row);
        }
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let row = ActionRow::builder()
                .title(gtk4::glib::markup_escape_text(&name))
                .subtitle(gtk4::glib::markup_escape_text(&path.to_string_lossy()))
                .subtitle_lines(1)
                .build();
            let add = Button::builder()
                .label("Add")
                .valign(Align::Center)
                .tooltip_text("Add this AppImage to AppNest")
                .build();
            add.add_css_class("pill");
            let (path, on_add) = (path.clone(), on_add.clone());
            add.connect_clicked(move |_| on_add(path.clone()));
            row.add_suffix(&add);
            self.discovered_list.append(&row);
        }
        self.discovered_count.set(files.len());
        self.discovered_box.set_visible(!files.is_empty());
    }

    pub fn set_empty_state(&self, has_apps: bool, is_searching: bool) {
        let has_discovered = self.discovered_count.get() > 0 && !is_searching;
        self.discovered_box.set_visible(has_discovered);
        if !has_apps && has_discovered {
            self.empty_status_page.set_visible(false);
            self.scrolled_window.set_visible(true);
            self.installed_list.set_visible(false);
            return;
        }
        self.installed_list.set_visible(true);
        if !has_apps {
            if is_searching {
                self.empty_status_page.set_title("No Results Found");
                self.empty_status_page
                    .set_description(Some("No applications match your search query."));
                self.add_first_btn.set_visible(false);
            } else {
                self.empty_status_page.set_title("No AppImages");
                self.empty_status_page
                    .set_description(Some("Add or drag and drop an AppImage to get started."));
                self.add_first_btn.set_visible(true);
            }
            self.empty_status_page.set_visible(true);
            self.scrolled_window.set_visible(false);
        } else {
            self.empty_status_page.set_visible(false);
            self.scrolled_window.set_visible(true);
        }
    }
}
