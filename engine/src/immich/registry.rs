use std::sync::LazyLock;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone)]
pub struct RemoteImage {
    pub asset_id: String,
    pub listed_asset_id: String,
    pub album_id: Option<String>,
}

static ENTRIES: LazyLock<RwLock<HashMap<PathBuf, RemoteImage>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn insert(path: PathBuf, image: RemoteImage) {
    ENTRIES.write().unwrap_or_else(std::sync::PoisonError::into_inner).insert(path, image);
}

pub fn get(path: &Path) -> Option<RemoteImage> {
    ENTRIES.read().unwrap_or_else(std::sync::PoisonError::into_inner).get(path).cloned()
}

pub fn set_listed_asset(path: &Path, listed_asset_id: &str) {
    if let Some(entry) = ENTRIES.write().unwrap_or_else(std::sync::PoisonError::into_inner).get_mut(path) {
        entry.listed_asset_id = listed_asset_id.to_string();
    }
}

pub fn is_placeholder(path: &Path) -> bool {
    let known = ENTRIES
        .read()
        .is_ok_and(|entries| entries.contains_key(path));
    known && !path.exists()
}

pub fn remove(path: &Path) {
    ENTRIES.write().unwrap_or_else(std::sync::PoisonError::into_inner).remove(path);
}

static TRASHING: LazyLock<RwLock<HashSet<String>>> = LazyLock::new(|| RwLock::new(HashSet::new()));

pub fn mark_trashing(asset_ids: &[String]) {
    TRASHING.write().unwrap_or_else(std::sync::PoisonError::into_inner).extend(asset_ids.iter().cloned());
}

pub fn done_trashing(asset_ids: &[String]) {
    let mut trashing = TRASHING.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    asset_ids.iter().for_each(|id| {
        trashing.remove(id);
    });
}

pub fn is_trashing(asset_id: &str) -> bool {
    TRASHING.read().unwrap_or_else(std::sync::PoisonError::into_inner).contains(asset_id)
}

pub fn clear() {
    ENTRIES.write().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}
