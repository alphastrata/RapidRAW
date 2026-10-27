pub use engine::guided_perspective::*;
use engine::guided_perspective::calculate_guided_perspective as engine_calculate;

#[tauri::command]
pub fn calculate_guided_perspective(
    lines: Vec<engine::guided_perspective::GuideLine>,
    width: f64,
    height: f64,
) -> engine::guided_perspective::GuidedResultJson {
    engine_calculate(lines, width, height)
}