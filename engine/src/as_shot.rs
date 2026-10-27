use crate::baseline_exposure::RawExposure;
use crate::paths::{parse_virtual_path, read_file_mapped};
use crate::formats::is_raw_file;
use crate::white_balance::WhiteBalance;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug)]
pub struct AsShot {
    pub white_balance: WhiteBalance,
    pub exposure: RawExposure,
}

impl AsShot {
    #[must_use]
    pub fn reference() -> Self {
        Self {
            white_balance: WhiteBalance::reference(),
            exposure: RawExposure::default(),
        }
    }
}

fn as_shot_cache() -> &'static Mutex<HashMap<String, AsShot>> {
    static CACHE: OnceLock<Mutex<HashMap<String, AsShot>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn as_shot(path: &str) -> AsShot {
    let (source_path, _) = parse_virtual_path(path);
    if !is_raw_file(&source_path) {
        return AsShot::reference();
    }
    let key = source_path.to_string_lossy().into_owned();
    if let Some(cached) = as_shot_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return *cached;
    }

    let as_shot = read_file_mapped(&source_path)
        .ok()
        .and_then(|mmap| crate::raw_processing::read_as_shot(&mmap))
        .unwrap_or_else(AsShot::reference);

    as_shot_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, as_shot);
    as_shot
}
