use crate::domain::appimage::AppImage;
use gtk4::prelude::*;
use gtk4::{Box, Button, Image, Label, Orientation};
use libadwaita::prelude::*;
use libadwaita::ActionRow;
use std::cell::Cell;

pub struct AppRowWidget {
    pub row: ActionRow,
    pub launch_button: Button,
    pub app_id: String,
    pub is_running: Cell<bool>,
}

impl AppRowWidget {
    pub fn new(app: &AppImage) -> Self {
        let row = ActionRow::builder()
            .title(&app.name)
            .activatable(true)
            .build();

        let subtitle = match &app.version {
            Some(ver) => format!("v{} • {}", ver, app.formatted_size()),
            None => app.formatted_size(),
        };
        row.set_subtitle(&subtitle);

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
        row.add_prefix(&icon_img);

        // Suffix container
        let suffix_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .valign(gtk4::Align::Center)
            .build();

        // Architecture badge
        let arch_label = Label::builder()
            .label(&app.architecture.to_string())
            .build();
        arch_label.add_css_class("pill-badge");
        arch_label.add_css_class("badge-info");
        suffix_box.append(&arch_label);

        // Quick launch / close button
        let launch_button = Button::builder()
            .icon_name("media-playback-start-symbolic")
            .valign(gtk4::Align::Center)
            .tooltip_text("Launch Application")
            .build();
        launch_button.add_css_class("flat");
        suffix_box.append(&launch_button);

        row.add_suffix(&suffix_box);

        Self {
            row,
            launch_button,
            app_id: app.id.clone(),
            is_running: Cell::new(false),
        }
    }

    pub fn set_running_state(&self, running: bool) {
        if self.is_running.get() == running {
            return;
        }
        self.is_running.set(running);
        if running {
            self.launch_button.set_icon_name("window-close-symbolic");
            self.launch_button.set_tooltip_text(Some("Close Application"));
            self.launch_button.remove_css_class("flat");
            self.launch_button.add_css_class("destructive-action");
        } else {
            self.launch_button.set_icon_name("media-playback-start-symbolic");
            self.launch_button.set_tooltip_text(Some("Launch Application"));
            self.launch_button.remove_css_class("destructive-action");
            self.launch_button.add_css_class("flat");
        }
    }
}

