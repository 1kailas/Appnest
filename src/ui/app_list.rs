use gtk4::prelude::*;
use gtk4::{Align, Box, Button, Label, ListBox, Orientation, ScrolledWindow, SearchEntry};
use libadwaita::StatusPage;

pub struct AppListWidget {
    pub container: Box,
    pub search_entry: SearchEntry,
    pub installed_list: ListBox,
    pub discovered_box: Box,
    pub discovered_list: ListBox,
    pub empty_status_page: StatusPage,
    pub add_first_btn: Button,
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

        // Discovered unmanaged section
        let discovered_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .margin_top(16)
            .visible(false)
            .build();

        let disc_header = Label::builder()
            .label("Discovered on System")
            .xalign(0.0)
            .build();
        disc_header.add_css_class("heading");

        let disc_sub = Label::builder()
            .label("Unmanaged AppImages found in scan folders")
            .xalign(0.0)
            .build();
        disc_sub.add_css_class("dim-label");

        let discovered_list = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .build();
        discovered_list.add_css_class("boxed-list");

        discovered_box.append(&disc_header);
        discovered_box.append(&disc_sub);
        discovered_box.append(&discovered_list);

        let list_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(16)
            .build();
        list_box.append(&installed_list);
        list_box.append(&discovered_box);

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&list_box)
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
        container.append(&scrolled);

        Self {
            container,
            search_entry,
            installed_list,
            discovered_box,
            discovered_list,
            empty_status_page,
            add_first_btn,
        }
    }

    pub fn set_empty_state(&self, has_apps: bool, is_searching: bool, has_discovered: bool) {
        if !has_apps && !has_discovered {
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
            if let Some(scrolled) = self.empty_status_page.next_sibling() {
                scrolled.set_visible(false);
            }
        } else {
            self.empty_status_page.set_visible(false);
            if let Some(scrolled) = self.empty_status_page.next_sibling() {
                scrolled.set_visible(true);
            }
        }
    }

    pub fn set_empty(&self, is_empty: bool) {
        self.set_empty_state(!is_empty, false, false);
    }
}
