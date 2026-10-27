use std::sync::Mutex;

use crate::app_state::AppState;
use crate::settings::AppSettings;

fn no_op() {}
fn no_str_op(_: &str) {}

static REGISTER_EXIT: Mutex<Option<fn()>> = Mutex::new(None);

pub fn register_exit_handler() {
    let f = REGISTER_EXIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(f) = *f {
        f();
    }
}

pub fn set_exit_handler(f: fn()) {
    *REGISTER_EXIT.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
}
static LOAD_SETTINGS: Mutex<fn(&tauri::AppHandle) -> AppSettings> = Mutex::new(no_load);
static REGENERATE_THUMBNAILS: Mutex<ThumbJob> = Mutex::new(no_thumbs);
static THUMBNAIL_QUEUE: Mutex<ThumbQueueFn> = Mutex::new(no_queue);
static RELEASE_DEFERRED: Mutex<fn(&str)> = Mutex::new(no_str_op);

type ThumbJob = fn(Vec<String>, &tauri::AppHandle, &AppSettings, bool);
type ThumbQueueFn = fn(&AppState, usize, &tauri::AppHandle);

fn no_load(_app_handle: &tauri::AppHandle) -> AppSettings {
    AppSettings::default()
}

fn no_thumbs(_paths: Vec<String>, _app_handle: &tauri::AppHandle, _settings: &AppSettings, _force: bool) {}

fn no_queue(_state: &AppState, _count: usize, _app_handle: &tauri::AppHandle) {}

pub fn load_settings(app_handle: &tauri::AppHandle) -> AppSettings {
    let f = LOAD_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    f(app_handle)
}

pub fn set_load_settings(f: fn(&tauri::AppHandle) -> AppSettings) {
    *LOAD_SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = f;
}

pub fn regenerate_thumbnails(paths: Vec<String>, app_handle: &tauri::AppHandle, settings: &AppSettings, force: bool) {
    let f = REGENERATE_THUMBNAILS.lock().unwrap_or_else(|e| e.into_inner());
    f(paths, app_handle, settings, force)
}

pub fn set_regenerate_thumbnails(f: ThumbJob) {
    *REGENERATE_THUMBNAILS.lock().unwrap_or_else(|e| e.into_inner()) = f;
}

pub fn add_to_thumbnail_queue(state: &AppState, count: usize, app_handle: &tauri::AppHandle) {
    let f = THUMBNAIL_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    f(state, count, app_handle)
}

pub fn set_add_to_thumbnail_queue(f: ThumbQueueFn) {
    *THUMBNAIL_QUEUE.lock().unwrap_or_else(|e| e.into_inner()) = f;
}

pub fn release_deferred(path: &str) {
    let f = RELEASE_DEFERRED.lock().unwrap_or_else(|e| e.into_inner());
    f(path)
}

pub fn set_release_deferred(f: fn(&str)) {
    *RELEASE_DEFERRED.lock().unwrap_or_else(|e| e.into_inner()) = f;
}
