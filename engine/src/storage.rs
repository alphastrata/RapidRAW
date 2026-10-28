use crate::best_effort::best_effort;
use std::path::Path;
use std::time::Duration;

use crate::settings::{
    AppSettings, default_ai_cache_limit_gb, default_ai_maps_cache_limit_gb,
};

fn mark_used_interval() -> Duration {
    Duration::from_mins(10)
}

pub fn apply_cache_limits(settings: &AppSettings) {
    crate::ai_denoise::set_disk_cache_limit_gb(
        settings
            .ai_cache_limit_gb
            .unwrap_or_else(default_ai_cache_limit_gb),
    );
    crate::effect_maps::set_disk_cache_limit_gb(
        settings
            .ai_maps_cache_limit_gb
            .unwrap_or_else(default_ai_maps_cache_limit_gb),
    );
}

pub fn mark_used(path: &Path) {
    let stale = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > mark_used_interval());
    if stale {
        best_effort(filetime::set_file_mtime(path, filetime::FileTime::now()));
    }
}

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub ai_denoise: u64,
    pub ai_maps: u64,
    pub metadata: u64,
    pub models: u64,
    pub logs: u64,
}