use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MyLens {
    pub maker: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CustomAspectRatio {
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PasteMode {
    Merge,
    Replace,
}

#[must_use]
pub fn all_available_adjustments() -> HashSet<String> {
    [
        "exposure",
        "brightness",
        "contrast",
        "curves",
        "pointCurves",
        "parametricCurve",
        "curveMode",
        "highlights",
        "shadows",
        "whites",
        "blacks",
        "toneMapper",
        "temperature",
        "tint",
        "whiteBalance",
        "saturation",
        "vibrance",
        "hsl",
        "hue",
        "colorGrading",
        "colorCalibration",
        "clarity",
        "structure",
        "dehaze",
        "sharpness",
        "sharpnessThreshold",
        "centré",
        "centréX",
        "centréY",
        "lumaNoiseReduction",
        "colorNoiseReduction",
        "aiDenoiseEnabled",
        "aiDenoiseMethod",
        "aiDenoiseAmount",
        "aiDenoiseDetail",
        "chromaticAberrationRedCyan",
        "chromaticAberrationBlueYellow",
        "negativeConversionEnabled",
        "negativeRedWeight",
        "negativeGreenWeight",
        "negativeBlueWeight",
        "negativeExposure",
        "negativeContrast",
        "vignetteAmount",
        "vignetteFeather",
        "vignetteMidpoint",
        "vignetteRoundness",
        "vignetteCenterX",
        "vignetteCenterY",
        "grainAmount",
        "grainRoughness",
        "grainSize",
        "lutIntensity",
        "lutName",
        "lutPath",
        "lutSize",
        "lutData",
        "lutIsSceneReferred",
        "glowAmount",
        "halationAmount",
        "flareAmount",
        "lensBlurEnabled",
        "lensBlurAmount",
        "lensBlurDiffusion",
        "lensBlurShape",
        "lensBlurMinDepth",
        "lensBlurMaxDepth",
        "lensBlurMinFade",
        "lensBlurMaxFade",
        "fogEnabled",
        "fogAmount",
        "fogStart",
        "fogDensity",
        "fogHeight",
        "fogVariation",
        "fogGlow",
        "fogTemperature",
        "fogTint",
        "relightEnabled",
        "relightLights",
        "relightAmbient",
        "relightSoftness",
        "relightShine",
        "relightShadows",
        "relightShadowSoftness",
        "crop",
        "aspectRatio",
        "rotation",
        "flipHorizontal",
        "flipVertical",
        "orientationSteps",
        "transformDistortion",
        "transformVertical",
        "transformHorizontal",
        "transformRotate",
        "transformAspect",
        "transformScale",
        "transformXOffset",
        "transformYOffset",
        "masks",
        "lensCorrectionMode",
        "lensMaker",
        "lensModel",
        "lensDistortionAmount",
        "lensVignetteAmount",
        "lensTcaAmount",
        "lensDistortionEnabled",
        "lensTcaEnabled",
        "lensVignetteEnabled",
        "guidedPerspective",
    ]
    .iter()
    .map(std::string::ToString::to_string)
    .collect()
}

#[must_use]
pub fn default_included_adjustments() -> HashSet<String> {
    let mut defaults = all_available_adjustments();

    let off_by_default = [
        "crop",
        "aspectRatio",
        "rotation",
        "flipHorizontal",
        "flipVertical",
        "orientationSteps",
        "transformDistortion",
        "transformVertical",
        "transformHorizontal",
        "transformRotate",
        "transformAspect",
        "transformScale",
        "transformXOffset",
        "transformYOffset",
        "masks",
        "lensCorrectionMode",
        "lensMaker",
        "lensModel",
        "lensDistortionAmount",
        "lensVignetteAmount",
        "lensTcaAmount",
        "lensDistortionEnabled",
        "lensTcaEnabled",
        "lensVignetteEnabled",
        "guidedPerspective",
    ];

    off_by_default.iter().for_each(|item| {
        defaults.remove(*item);
    });

    defaults
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CopyPasteSettings {
    pub mode: PasteMode,
    #[serde(default = "default_included_adjustments")]
    pub included_adjustments: HashSet<String>,
    #[serde(default)]
    pub known_adjustments: HashSet<String>,
    #[serde(default)]
    pub auto_sync: bool,
}

impl Default for CopyPasteSettings {
    fn default() -> Self {
        Self {
            mode: PasteMode::Merge,
            included_adjustments: default_included_adjustments(),
            known_adjustments: all_available_adjustments(),
            auto_sync: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdjustmentLayout {
    pub section_order: Vec<String>,
    pub hidden_sections: Vec<String>,
    pub open_sections: BTreeMap<String, bool>,
    pub tool_order: HashMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_tools: Option<Vec<String>>,
    pub collapsed_tools: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    pub left_panel_width: u32,
    pub right_panel_width: u32,
    pub left_top_height: u32,
    pub right_top_height: u32,
    pub panel_layout: HashMap<String, Vec<String>>,
    pub active_panels: HashMap<String, Option<String>>,
    pub panel_switcher_placement: HashMap<String, String>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        let mut panel_layout = HashMap::new();
        let left_top = vec!["export".to_string()];

        panel_layout.insert("leftTop".to_string(), left_top);
        panel_layout.insert("leftBottom".to_string(), vec![]);

        panel_layout.insert(
            "rightTop".to_string(),
            vec![
                "adjustments".to_string(),
                "crop".to_string(),
                "masks".to_string(),
                "ai".to_string(),
                "presets".to_string(),
            ],
        );
        panel_layout.insert("rightBottom".to_string(), vec![]);

        let mut active_panels = HashMap::new();
        active_panels.insert("leftTop".to_string(), Some("export".to_string()));
        active_panels.insert("leftBottom".to_string(), None);
        active_panels.insert("rightTop".to_string(), Some("adjustments".to_string()));
        active_panels.insert("rightBottom".to_string(), None);

        let mut panel_switcher_placement = HashMap::new();
        panel_switcher_placement.insert("leftTop".to_string(), "bottom".to_string());
        panel_switcher_placement.insert("leftBottom".to_string(), "bottom".to_string());
        panel_switcher_placement.insert("rightTop".to_string(), "right".to_string());
        panel_switcher_placement.insert("rightBottom".to_string(), "right".to_string());

        Self {
            left_panel_width: 320,
            right_panel_width: 320,
            left_top_height: 450,
            right_top_height: 450,
            panel_layout,
            active_panels,
            panel_switcher_placement,
        }
    }
}

#[must_use]
pub fn default_linear_raw_mode() -> String {
    "auto".to_string()
}

#[must_use]
pub fn default_baseline_exposure() -> f32 {
    0.7
}

#[must_use]
pub fn default_compensate_highlight_preservation() -> bool {
    true
}

#[must_use]
pub fn default_ai_gpu_acceleration() -> bool {
    true
}

#[must_use]
pub fn default_ai_cache_limit_gb() -> u32 {
    10
}

#[must_use]
pub fn default_ai_maps_cache_limit_gb() -> u32 {
    4
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub editor_preview_resolution: Option<u32>,
    #[serde(default)]
    pub enable_zoom_hifi: Option<bool>,
    #[serde(default)]
    pub use_full_dpi_rendering: Option<bool>,
    #[serde(default)]
    pub high_res_zoom_multiplier: Option<f32>,
    #[serde(default)]
    pub enable_live_previews: Option<bool>,
    #[serde(default)]
    pub live_preview_quality: Option<String>,
    pub theme: Option<String>,
    #[serde(default)]
    pub font_family: Option<String>,
    pub decorations: Option<bool>,
    #[serde(alias = "comfyuiAddress")]
    pub ai_connector_address: Option<String>,
    pub ui_visibility: Option<Value>,
    pub ai_provider: Option<String>,
    #[serde(default = "default_ai_gpu_acceleration")]
    pub ai_gpu_acceleration: bool,
    #[serde(default)]
    pub copy_paste_settings: CopyPasteSettings,
    #[serde(default)]
    pub raw_highlight_compression: Option<f32>,
    #[serde(default)]
    pub processing_backend: Option<String>,
    #[serde(default)]
    pub linux_gpu_optimization: Option<bool>,
    #[serde(default)]
    pub linux_gpu_optimization_migrated_v1: Option<bool>,
    #[serde(default)]
    pub my_lenses: Option<Vec<MyLens>>,
    #[serde(default = "default_linear_raw_mode")]
    pub linear_raw_mode: String,
    #[serde(default)]
    pub enable_xmp_sync: Option<bool>,
    #[serde(default)]
    pub create_xmp_if_missing: Option<bool>,
    #[serde(default)]
    pub is_waveform_visible: Option<bool>,
    #[serde(default)]
    pub waveform_height: Option<u32>,
    #[serde(default)]
    pub active_waveform_channel: Option<String>,
    #[serde(default)]
    pub use_wgpu_renderer: Option<bool>,
    #[serde(default)]
    pub editor_neutral_grey_bg: Option<bool>,
    #[serde(default)]
    pub canvas_input_mode: Option<String>,
    #[serde(default)]
    pub compare_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_balance_mode: Option<String>,
    #[serde(default)]
    pub zoom_speed_multiplier: Option<f32>,
    #[serde(default)]
    pub zoom_photo_to_pixel_click: Option<bool>,
    #[serde(default)]
    pub keybinds: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub image_cache_size: Option<u32>,
    #[serde(default)]
    pub ai_cache_limit_gb: Option<u32>,
    #[serde(default)]
    pub ai_maps_cache_limit_gb: Option<u32>,
    #[serde(default)]
    pub export_worker_threads: Option<u32>,
    #[serde(default)]
    pub export_memory_limit: Option<u32>,
    #[serde(default)]
    pub enable_baseline_exposure: bool,
    #[serde(default = "default_baseline_exposure")]
    pub baseline_exposure: f32,
    #[serde(default = "default_compensate_highlight_preservation")]
    pub compensate_highlight_preservation: bool,
    #[serde(default)]
    pub tonemapper_override_enabled: Option<bool>,
    #[serde(default)]
    pub default_raw_tonemapper: Option<String>,
    #[serde(default)]
    pub default_non_raw_tonemapper: Option<String>,
    #[serde(default)]
    pub enable_focus_mode: Option<bool>,
    #[serde(default)]
    pub enable_tool_focus_mode: Option<bool>,
    #[serde(default)]
    pub raw_preprocessing_color_nr: Option<f32>,
    #[serde(default)]
    pub raw_preprocessing_sharpening: Option<f32>,
    #[serde(default)]
    pub apply_preprocessing_to_non_raws: Option<bool>,
    #[serde(default)]
    pub use_apple_raw9: Option<bool>,
    #[serde(default)]
    pub exif_overlay: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub custom_aspect_ratios: Vec<CustomAspectRatio>,
    #[serde(default)]
    pub adjustment_layout: AdjustmentLayout,
    #[serde(default)]
    pub workspace: WorkspaceState,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            editor_preview_resolution: Some(1920),
            enable_zoom_hifi: Some(true),
            use_full_dpi_rendering: Some(false),
            enable_live_previews: Some(true),
            live_preview_quality: Some("high".to_string()),
            theme: Some("dark".to_string()),
            font_family: None,
            decorations: Some(false),
            ai_connector_address: None,
            ui_visibility: None,
            ai_provider: Some("cpu".to_string()),
            ai_gpu_acceleration: default_ai_gpu_acceleration(),
            copy_paste_settings: CopyPasteSettings::default(),
            raw_highlight_compression: Some(2.5),
            processing_backend: Some("auto".to_string()),
            linux_gpu_optimization: Some(false),
            linux_gpu_optimization_migrated_v1: Some(true),
            my_lenses: Some(Vec::new()),
            high_res_zoom_multiplier: Some(1.0),
            linear_raw_mode: default_linear_raw_mode(),
            enable_xmp_sync: Some(true),
            create_xmp_if_missing: Some(false),
            is_waveform_visible: Some(false),
            waveform_height: Some(220),
            active_waveform_channel: Some("luma".to_string()),
            #[cfg(target_os = "linux")]
            use_wgpu_renderer: Some(false),
            #[cfg(not(target_os = "linux"))]
            use_wgpu_renderer: Some(true),
            editor_neutral_grey_bg: Some(false),
            canvas_input_mode: Some("mouse".to_string()),
            compare_mode: None,
            white_balance_mode: None,
            zoom_speed_multiplier: Some(1.0),
            zoom_photo_to_pixel_click: Some(false),
            keybinds: HashMap::new(),
            image_cache_size: Some(5),
            ai_cache_limit_gb: Some(default_ai_cache_limit_gb()),
            ai_maps_cache_limit_gb: Some(default_ai_maps_cache_limit_gb()),
            export_worker_threads: None,
            export_memory_limit: None,
            enable_baseline_exposure: false,
            baseline_exposure: default_baseline_exposure(),
            compensate_highlight_preservation: default_compensate_highlight_preservation(),
            tonemapper_override_enabled: Some(false),
            default_raw_tonemapper: Some("agx".to_string()),
            default_non_raw_tonemapper: Some("basic".to_string()),
            enable_focus_mode: Some(false),
            enable_tool_focus_mode: Some(false),
            raw_preprocessing_color_nr: Some(0.5),
            raw_preprocessing_sharpening: Some(0.35),
            apply_preprocessing_to_non_raws: Some(false),
            use_apple_raw9: Some(false),
            exif_overlay: Some("off".to_string()),
            language: Some("en".to_string()),
            custom_aspect_ratios: Vec::new(),
            adjustment_layout: AdjustmentLayout::default(),
            workspace: WorkspaceState::default(),
        }
    }
}
