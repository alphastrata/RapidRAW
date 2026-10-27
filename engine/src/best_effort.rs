use std::fmt::Display;

pub fn best_effort<T, E: Display>(result: Result<T, E>) {
    if let Err(e) = result {
        log::debug!("{e}");
    }
}
