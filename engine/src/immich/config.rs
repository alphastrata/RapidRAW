use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ImmichSettings {
    pub server_url: String,
    pub cache_limit_gb: u32,
    pub export_album: String,
    pub export_with_original: bool,
}

impl Default for ImmichSettings {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            cache_limit_gb: 20,
            export_album: "source".to_string(),
            export_with_original: true,
        }
    }
}

impl ImmichSettings {
    #[must_use]
    pub fn same_library(&self, other: &ImmichSettings) -> bool {
        self.server_url.trim() == other.server_url.trim()
    }
}
