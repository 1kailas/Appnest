use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppImageMetadata {
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub comment: Option<String>,
    pub categories: Vec<String>,
    pub exec_args: Option<String>,
    pub icon_name: Option<String>,
    pub terminal: bool,
    pub mime_types: Vec<String>,
    pub startup_wm_class: Option<String>,
    pub keywords: Vec<String>,
}
