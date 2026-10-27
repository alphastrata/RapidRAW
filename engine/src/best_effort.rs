use std::fmt::Display;

#[cfg(target_os = "android")]
pub fn jni_check(result: jni::errors::Result<()>) {
    if let Err(e) = result {
        log::debug!("jni best-effort call failed: {e}");
    }
}

pub fn best_effort<T, E: Display>(result: Result<T, E>) {
    if let Err(e) = result {
        log::debug!("{e}");
    }
}
