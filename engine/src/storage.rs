use serde::Serialize;
use crate::best_effort::best_effort;
use std::collections::HashMap;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::settings::{
    AppSettings, default_ai_cache_limit_gb, default_ai_maps_cache_limit_gb,
    default_thumbnail_cache_limit_gb,
};


struct ThumbnailCache {
    dir: Mutex<Option<PathBuf>>,
    session_start: SystemTime,
    limit: AtomicU64,
    estimated: AtomicU64,
    written_since_prune: AtomicU64,
    pruning: AtomicBool,
    prune_requested: AtomicBool,
}

static THUMBNAIL_CACHE: LazyLock<ThumbnailCache> = LazyLock::new(|| ThumbnailCache {
    dir: Mutex::new(None),
    session_start: SystemTime::now() - mark_used_interval(),
    limit: AtomicU64::new(u64::MAX),
    estimated: AtomicU64::new(0),
    written_since_prune: AtomicU64::new(0),
    pruning: AtomicBool::new(false),
    prune_requested: AtomicBool::new(false),
});

fn mark_used_interval() -> Duration {
    Duration::from_mins(10)
}

pub fn apply_cache_limits(settings: &AppSettings) {
    set_thumbnail_limit_gb(
        settings
            .thumbnail_cache_limit_gb
            .unwrap_or_else(default_thumbnail_cache_limit_gb),
    );
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

use std::fs;

pub fn mark_used(path: &Path) {
    let stale = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > mark_used_interval());
    if stale {
        best_effort(filetime::set_file_mtime(path, filetime::FileTime::now()));
    }
}

pub fn initialize_thumbnail_cache(cache_dir: PathBuf) {
    *THUMBNAIL_CACHE
        .dir
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cache_dir.join("thumbnails"));
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        prune_thumbnails_in_background();
    });
}

pub fn reset_thumbnail_estimate() {
    THUMBNAIL_CACHE.estimated.store(0, Ordering::SeqCst);
    THUMBNAIL_CACHE
        .written_since_prune
        .store(0, Ordering::SeqCst);
}

fn set_thumbnail_limit_gb(limit_gb: u32) {
    let cache = &*THUMBNAIL_CACHE;
    let limit = u64::from(limit_gb.max(1)) * 1024 * 1024 * 1024;
    if cache.limit.swap(limit, Ordering::SeqCst) > limit
        && cache.estimated.load(Ordering::SeqCst) > limit
    {
        prune_thumbnails_in_background();
    }
}

pub fn record_thumbnail_write(bytes: u64) {
    let cache = &*THUMBNAIL_CACHE;
    let estimated = cache.estimated.fetch_add(bytes, Ordering::SeqCst) + bytes;
    let written = cache
        .written_since_prune
        .fetch_add(bytes, Ordering::SeqCst)
        + bytes;
    if estimated > cache.limit.load(Ordering::SeqCst) && written >= 64 * 1024 * 1024 {
        prune_thumbnails_in_background();
    }
}

fn prune_thumbnails_in_background() {
    let cache = &*THUMBNAIL_CACHE;
    cache.prune_requested.store(true, Ordering::Release);
    if cache.pruning.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            while cache.prune_requested.swap(false, Ordering::AcqRel) {
                cache.written_since_prune.store(0, Ordering::SeqCst);
                let dir = cache.dir.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                if let Some(dir) = dir {
                    let before = cache.estimated.load(Ordering::SeqCst);
                    let remaining = prune_thumbnails(&dir, cache.limit.load(Ordering::SeqCst));
                    let written_during = cache
                        .estimated
                        .load(Ordering::SeqCst)
                        .saturating_sub(before);
                    cache
                        .estimated
                        .store(remaining + written_during, Ordering::SeqCst);
                }
            }
            cache.pruning.store(false, Ordering::Release);
            if !cache.prune_requested.load(Ordering::Acquire)
                || cache.pruning.swap(true, Ordering::AcqRel)
            {
                break;
            }
        }
    });
}

struct ThumbnailEntry {
    files: Vec<PathBuf>,
    size: u64,
    last_used: SystemTime,
}

fn prune_thumbnails(dir: &Path, limit_bytes: u64) -> u64 {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return 0;
    };

    let mut entries: HashMap<String, ThumbnailEntry> = HashMap::new();
    let mut total = 0u64;
    read_dir
        .filter_map(std::result::Result::ok)
        .filter_map(|file| {
            let path = file.path();
            let stem = path.file_stem().and_then(|s| s.to_str())?.to_string();
            let meta = file.metadata().ok()?;
            meta.is_file().then(|| (path, stem, meta))
        })
        .for_each(|(path, stem, meta)| {
            let key = stem
                .strip_suffix("_small")
                .or_else(|| stem.strip_suffix("_medium"))
                .unwrap_or(&stem)
                .to_string();
            let entry = entries.entry(key).or_insert(ThumbnailEntry {
                files: Vec::new(),
                size: 0,
                last_used: UNIX_EPOCH,
            });
            entry.files.push(path);
            entry.size += meta.len();
            entry.last_used = entry.last_used.max(meta.modified().unwrap_or(UNIX_EPOCH));
            total += meta.len();
        });
    if total <= limit_bytes {
        return total;
    }

    let target = limit_bytes / 10 * 9;
    let mut entries: Vec<ThumbnailEntry> = entries.into_values().collect();
    entries.sort_by_key(|entry| entry.last_used);
    entries.into_iter().map_while(|entry| {
        (total > target && entry.last_used < THUMBNAIL_CACHE.session_start).then(|| {
            entry
                .files
                .iter()
                .for_each(|file| best_effort(fs::remove_file(file)));
            total = total.saturating_sub(entry.size);
            entry.size
        })
    }).count();
    total
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub thumbnails: u64,
    pub ai_denoise: u64,
    pub ai_maps: u64,
    pub metadata: u64,
    pub immich: u64,
    pub models: u64,
    pub logs: u64,
}



