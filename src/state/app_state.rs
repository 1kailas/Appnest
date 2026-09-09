use crate::config::settings::AppSettings;
use crate::domain::appimage::AppImage;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub apps: Vec<AppImage>,
    pub unmanaged_discovered: Vec<PathBuf>,
    pub selected_app_id: Option<String>,
    pub search_query: String,
    pub loading: bool,
    pub status_message: Option<String>,
    pub settings: AppSettings,
}

impl AppState {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            ..Default::default()
        }
    }

    pub fn filtered_apps(&self) -> Vec<&AppImage> {
        let q = self.search_query.trim().to_lowercase();
        if q.is_empty() {
            self.apps.iter().collect()
        } else {
            self.apps
                .iter()
                .filter(|a| {
                    a.name.to_lowercase().contains(&q)
                        || a.id.to_lowercase().contains(&q)
                        || a.version
                            .as_ref()
                            .map_or(false, |v| v.to_lowercase().contains(&q))
                        || a.metadata
                            .categories
                            .iter()
                            .any(|c| c.to_lowercase().contains(&q))
                })
                .collect()
        }
    }

    pub fn selected_app(&self) -> Option<&AppImage> {
        self.selected_app_id
            .as_ref()
            .and_then(|id| self.apps.iter().find(|a| &a.id == id))
    }
}
