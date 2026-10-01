use gtk4::prelude::*;
use gtk4::{Align, Box, Button, ListBox, Orientation, ScrolledWindow, SearchEntry};
use libadwaita::StatusPage;

pub struct AppListWidget {
    pub container: Box,
    pub search_entry: SearchEntry,
    pub installed_list: ListBox,
    pub empty_status_page: StatusPage,
    pub scrolled_window: ScrolledWindow,
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

        let scrolled_window = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .child(&installed_list)
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
        }
    }

    pub fn set_empty_state(&self, has_apps: bool, is_searching: bool) {
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

    pub fn set_empty(&self, is_empty: bool) {
        self.set_empty_state(!is_empty, false);
    }
}
