use engine::guided_perspective::calculate_guided_perspective as engine_calculate;
pub use engine::guided_perspective::*;
use engine::guided_perspective::{GuideLine, GuidedResultJson};

#[tauri::command]
pub fn calculate_guided_perspective(
    lines: Vec<GuideLine>,
    width: f64,
    height: f64,
) -> GuidedResultJson {
    engine_calculate(lines, width, height)
}
