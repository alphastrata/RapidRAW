use crate::file_management::Preset;

pub fn convert_xmp_to_preset(xmp_content: &str) -> Result<Preset, String> {
    engine::preset_converter::convert_xmp_to_preset(xmp_content)
        .map(|p| Preset {
            id: p.id,
            name: p.name,
            adjustments: p.adjustments,
            include_masks: p.include_masks,
            include_crop_transform: p.include_crop_transform,
            preset_type: p.preset_type,
        })
        .map_err(|e| e.to_string())
}
