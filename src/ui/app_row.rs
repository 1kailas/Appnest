use crate::domain::appimage::AppImage;
use gtk4::prelude::*;
use gtk4::{Align, Box, Button, Image, Label, ListBoxRow, Orientation};
use std::cell::Cell;

pub struct AppRowWidget {
    pub row: ListBoxRow,
    pub launch_button: Button,
    pub running_badge: Label,
    pub app_id: String,
    pub is_running: Cell<bool>,
}

impl AppRowWidget {
    pub fn new(app: &AppImage) -> Self {
        let row = ListBoxRow::builder()
            .activatable(false)
            .selectable(true)
            .build();

        let main_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(10)
            .margin_end(10)
            .valign(Align::Center)
            .build();

        // App Icon
        let icon_img = if let Some(ref icon_path) = app.icon_path {
            if icon_path.exists() {
                Image::from_file(icon_path)
            } else if let Some(ref icon_name) = app.metadata.icon_name {
                Image::from_icon_name(icon_name)
            } else {
                Image::from_icon_name("application-x-executable")
            }
        } else if let Some(ref icon_name) = app.metadata.icon_name {
            Image::from_icon_name(icon_name)
        } else {
            Image::from_icon_name("application-x-executable")
        };

        icon_img.set_pixel_size(36);
        icon_img.add_css_class("app-icon-small");
        main_box.append(&icon_img);

        // Center: Title + Subtitle
        let text_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(Align::Center)
            .build();

        let title_label = Label::builder()
            .label(&app.name)
            .xalign(0.0)
            .single_line_mode(true)
            .lines(1)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        title_label.add_css_class("heading");

        let subtitle = match &app.version {
            Some(ver) => format!("v{} • {}", ver, app.formatted_size()),
            None => app.formatted_size(),
        };
        let subtitle_label = Label::builder()
            .label(&subtitle)
            .xalign(0.0)
            .single_line_mode(true)
            .lines(1)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        subtitle_label.add_css_class("dim-label");
        subtitle_label.add_css_class("caption");

        text_box.append(&title_label);
        text_box.append(&subtitle_label);
        main_box.append(&text_box);

        // Trailing: Running badge + Launch button
        let trailing_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(6)
            .valign(Align::Center)
            .build();

        let running_badge = Label::builder().label("Running").build();
        running_badge.add_css_class("pill-badge");
        running_badge.add_css_class("badge-running");
        running_badge.set_visible(false);
        trailing_box.append(&running_badge);

        let launch_button = Button::builder()
            .icon_name("media-playback-start-symbolic")
            .valign(Align::Center)
            .tooltip_text("Launch Application")
            .build();
        launch_button.add_css_class("flat");
        launch_button.add_css_class("circular");
        trailing_box.append(&launch_button);

        main_box.append(&trailing_box);
        row.set_child(Some(&main_box));

        Self {
            row,
            launch_button,
            running_badge,
            app_id: app.id.clone(),
            is_running: Cell::new(false),
        }
    }

    pub fn set_running_state(&self, running: bool) {
        if self.is_running.get() == running {
            return;
        }
        self.is_running.set(running);
        self.running_badge.set_visible(running);
        if running {
            self.launch_button.set_icon_name("window-close-symbolic");
            self.launch_button
                .set_tooltip_text(Some("Close Application"));
            self.launch_button.remove_css_class("flat");
            self.launch_button.add_css_class("destructive-action");
        } else {
            self.launch_button
                .set_icon_name("media-playback-start-symbolic");
            self.launch_button
                .set_tooltip_text(Some("Launch Application"));
            self.launch_button.remove_css_class("destructive-action");
            self.launch_button.add_css_class("flat");
        }
    }
}
