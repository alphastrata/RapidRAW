use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use uuid::Uuid;

use crate::white_balance::{MIRED_PER_RELATIVE_UNIT, TINT_PER_RELATIVE_UNIT};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub adjustments: Value,
    #[serde(rename = "includeMasks", skip_serializing_if = "Option::is_none")]
    pub include_masks: Option<bool>,
    #[serde(
        rename = "includeCropTransform",
        skip_serializing_if = "Option::is_none"
    )]
    pub include_crop_transform: Option<bool>,
    #[serde(rename = "presetType", skip_serializing_if = "Option::is_none")]
    pub preset_type: Option<String>,
}

#[derive(Copy, Clone, Debug)]
enum Num {
    I(i64),
    F(f64),
}

fn parse_num(s: &str) -> Option<Num> {
    s.parse::<i64>()
        .map(Num::I)
        .or_else(|_| s.parse::<f64>().map(Num::F))
        .ok()
}

fn num_to_json(num: Num) -> Option<Value> {
    match num {
        Num::I(i) => Some(Value::Number(i.into())),
        Num::F(f) => serde_json::Number::from_f64(f).map(Value::Number),
    }
}

fn get_attr_as_f64(attrs: &HashMap<String, String>, key: &str) -> Option<f64> {
    attrs
        .get(key)
        .and_then(|s| s.trim_start_matches('+').parse::<f64>().ok())
}

fn compiled(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).context("invalid XMP pattern")
}

fn extract_xmp_name(xmp_content: &str) -> Option<String> {
    compiled(r#"(?s)<crs:Name>.*?<rdf:Alt>.*?<rdf:li[^>]*>([^<]+)</rdf:li>.*?</crs:Name>"#)
        .ok()?
        .captures(xmp_content)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
}

fn shadow_dampened_y(x: f64, y: f64) -> f64 {
    const SHADOW_RANGE_END: f64 = 64.0;
    const SHADOW_DAMPEN_START: f64 = 0.8;
    const SHADOW_DAMPEN_END: f64 = 1.0;

    let lift_amount = y - x;
    let progress = x / SHADOW_RANGE_END;
    let dampening_factor =
        SHADOW_DAMPEN_START + (SHADOW_DAMPEN_END - SHADOW_DAMPEN_START) * progress;
    x + (lift_amount * dampening_factor)
}

fn curve_point(captures: &regex::Captures<'_>, curve_name: &str) -> Option<Value> {
    let x: u32 = captures.get(1)?.as_str().parse().ok()?;
    let y: u32 = captures.get(2)?.as_str().parse().ok()?;

    let x_f64 = f64::from(x);
    let y_f64 = f64::from(y);
    let final_y = if curve_name == "ToneCurvePV2012" && y_f64 > x_f64 && x_f64 < 64.0 {
        let damped = shadow_dampened_y(x_f64, y_f64).round().clamp(0.0, 255.0);
        damped as u32
    } else {
        y
    };

    let mut point = Map::new();
    point.insert("x".to_string(), Value::Number(x.into()));
    point.insert("y".to_string(), Value::Number(final_y.into()));
    Some(Value::Object(point))
}

fn extract_tone_curve_points(xmp_str: &str, curve_name: &str) -> Option<Vec<Value>> {
    let pattern = format!(
        r"(?s)<crs:{}>\s*<rdf:Seq>(.*?)</rdf:Seq>\s*</crs:{}>",
        curve_name, curve_name
    );
    let captures = compiled(&pattern).ok()?.captures(xmp_str)?;
    let seq_content = captures.get(1)?.as_str();

    let point_re = compiled(r"<rdf:li>(\d+),\s*(\d+)</rdf:li>").ok()?;
    let points: Option<Vec<Value>> = point_re
        .captures_iter(seq_content)
        .map(|c| curve_point(&c, curve_name))
        .collect();

    match points {
        Some(points) if !points.is_empty() => Some(points),
        _ => None,
    }
}

pub fn convert_xmp_to_preset(xmp_content: &str) -> Result<Preset> {
    let xmp_one_line = xmp_content.split('\n').collect::<Vec<_>>().join(" ");

    let attr_re = compiled(r#"crs:([A-Za-z0-9]+)="([^"]*)""#)?;
    let attrs: HashMap<String, String> = attr_re
        .captures_iter(&xmp_one_line)
        .map(|c| {
            c.get(1)
                .zip(c.get(2))
                .map(|(k, v)| (k.as_str().to_string(), v.as_str().to_string()))
        })
        .collect::<Option<_>>()
        .context("malformed XMP attribute capture")?;

    let mut adjustments = Map::new();
    let mut color_grading_map = Map::new();

    let mappings = [
        ("Exposure2012", "exposure"),
        ("Contrast2012", "contrast"),
        ("Highlights2012", "highlights"),
        ("Whites2012", "whites"),
        ("Blacks2012", "blacks"),
        ("Clarity2012", "clarity"),
        ("Dehaze", "dehaze"),
        ("Vibrance", "vibrance"),
        ("Saturation", "saturation"),
        ("Texture", "structure"),
        ("SharpenRadius", "sharpenRadius"),
        ("SharpenDetail", "sharpenDetail"),
        ("SharpenEdgeMasking", "sharpenMasking"),
        ("LuminanceSmoothing", "lumaNoiseReduction"),
        ("ColorNoiseReduction", "colorNoiseReduction"),
        ("ColorNoiseReductionDetail", "colorNoiseDetail"),
        ("ColorNoiseReductionSmoothness", "colorNoiseSmoothness"),
        ("ChromaticAberrationRedCyan", "chromaticAberrationRedCyan"),
        (
            "ChromaticAberrationBlueYellow",
            "chromaticAberrationBlueYellow",
        ),
        ("PostCropVignetteAmount", "vignetteAmount"),
        ("PostCropVignetteMidpoint", "vignetteMidpoint"),
        ("PostCropVignetteFeather", "vignetteFeather"),
        ("PostCropVignetteRoundness", "vignetteRoundness"),
        ("GrainAmount", "grainAmount"),
        ("GrainSize", "grainSize"),
        ("GrainFrequency", "grainRoughness"),
        ("ColorGradeBlending", "blending"),
    ];

    let (blend, plain): (Vec<(String, Value)>, Vec<(String, Value)>) = mappings
        .into_iter()
        .filter_map(|(xmp_key, rr_key)| {
            let val = attrs
                .get(xmp_key)
                .and_then(|raw| parse_num(raw.trim_start_matches('+')))
                .and_then(num_to_json)?;
            Some((rr_key, val))
        })
        .map(|(k, v)| (String::from(k), v))
        .partition(|(k, _)| k == "blending");
    color_grading_map.extend(blend);
    adjustments.extend(plain);

    if let Some(shadows_val) = get_attr_as_f64(&attrs, "Shadows2012") {
        let adjusted_shadows = (shadows_val * 1.5).min(100.0);
        adjustments.insert("shadows".to_string(), json!(adjusted_shadows));
    }

    if let Some(sharpness_val) = get_attr_as_f64(&attrs, "Sharpness") {
        let scaled_sharpness = (sharpness_val / 150.0) * 100.0;
        adjustments.insert(
            "sharpness".to_string(),
            json!(scaled_sharpness.clamp(0.0, 100.0)),
        );
    }

    if let Some(adjusted_k) = get_attr_as_f64(&attrs, "Temperature") {
        const AS_SHOT_DEFAULT: f64 = 5500.0;
        let as_shot_k = get_attr_as_f64(&attrs, "AsShotTemperature").unwrap_or(AS_SHOT_DEFAULT);
        let mired_adjusted = 1_000_000.0 / adjusted_k;
        let mired_as_shot = 1_000_000.0 / as_shot_k;
        let temp_value = -(mired_adjusted - mired_as_shot) / MIRED_PER_RELATIVE_UNIT;
        adjustments.insert(
            "temperature".to_string(),
            json!(temp_value.clamp(-100.0, 100.0)),
        );
    }

    if let Some(tint_val) = get_attr_as_f64(&attrs, "Tint") {
        let scaled_tint = tint_val / TINT_PER_RELATIVE_UNIT;
        adjustments.insert("tint".to_string(), json!(scaled_tint.clamp(-100.0, 100.0)));
    }

    let colors = [
        ("Red", "reds"),
        ("Orange", "oranges"),
        ("Yellow", "yellows"),
        ("Green", "greens"),
        ("Aqua", "aquas"),
        ("Blue", "blues"),
        ("Purple", "purples"),
        ("Magenta", "magentas"),
    ];
    let hsl_map: Map<String, Value> = colors
        .into_iter()
        .filter_map(|(src, dst)| {
            let color_map: Map<String, Value> = [
                ("hue", format!("HueAdjustment{src}"), 0.75),
                ("saturation", format!("SaturationAdjustment{src}"), 1.0),
                ("luminance", format!("LuminanceAdjustment{src}"), 1.0),
            ]
            .into_iter()
            .filter_map(|(field, key, scale)| {
                let raw = attrs.get(&key)?;
                let val_f64 = parse_num(raw.trim_start_matches('+'))
                    .and_then(num_to_json)?
                    .as_f64()?;
                scaled_field_value(field, val_f64 * scale)
            })
            .collect();
            (!color_map.is_empty()).then_some((String::from(dst), Value::Object(color_map)))
        })
        .collect();

    if !hsl_map.is_empty() {
        adjustments.insert("hsl".to_string(), Value::Object(hsl_map));
    }

    let color_grading = [
        (
            "shadows",
            [
                ("hue", "SplitToningShadowHue"),
                ("saturation", "SplitToningShadowSaturation"),
                ("luminance", "ColorGradeShadowLum"),
            ],
        ),
        (
            "midtones",
            [
                ("hue", "ColorGradeMidtoneHue"),
                ("saturation", "ColorGradeMidtoneSat"),
                ("luminance", "ColorGradeMidtoneLum"),
            ],
        ),
        (
            "highlights",
            [
                ("hue", "SplitToningHighlightHue"),
                ("saturation", "SplitToningHighlightSaturation"),
                ("luminance", "ColorGradeHighlightLum"),
            ],
        ),
        (
            "global",
            [
                ("hue", "ColorGradeGlobalHue"),
                ("saturation", "ColorGradeGlobalSat"),
                ("luminance", "ColorGradeGlobalLum"),
            ],
        ),
    ];

    let zone_maps: Map<String, Value> = color_grading
        .iter()
        .filter_map(|&(zone, fields)| {
            let zone_map: Map<String, Value> = fields
                .iter()
                .filter_map(|&(field, key)| {
                    let raw = attrs.get(key)?;
                    parse_num(raw)
                        .and_then(num_to_json)
                        .map(|v| (String::from(field), v))
                })
                .collect();
            (!zone_map.is_empty()).then_some((String::from(zone), Value::Object(zone_map)))
        })
        .collect();
    color_grading_map.extend(zone_maps);

    if let Some(json_val) = attrs
        .get("SplitToningBalance")
        .and_then(|raw| parse_num(raw))
        .and_then(num_to_json)
    {
        color_grading_map.insert("balance".to_string(), json_val);
    }

    if !color_grading_map.is_empty() {
        adjustments.insert("colorGrading".to_string(), Value::Object(color_grading_map));
    }

    let curve_mappings = [
        ("ToneCurvePV2012", "luma"),
        ("ToneCurvePV2012Red", "red"),
        ("ToneCurvePV2012Green", "green"),
        ("ToneCurvePV2012Blue", "blue"),
    ];
    let curve_map: Map<String, Value> = curve_mappings
        .into_iter()
        .filter_map(|(xmp_curve, rr_curve)| {
            let points = extract_tone_curve_points(xmp_content, xmp_curve)?;
            Some((String::from(rr_curve), Value::Array(points)))
        })
        .collect();
    if !curve_map.is_empty() {
        adjustments.insert("curves".to_string(), Value::Object(curve_map));
    }

    let preset_name =
        extract_xmp_name(xmp_content).unwrap_or_else(|| "Imported Preset".to_string());

    Ok(Preset {
        id: Uuid::new_v4().to_string(),
        name: preset_name,
        adjustments: Value::Object(adjustments),
        include_masks: Some(false),
        include_crop_transform: Some(false),
        preset_type: Some("style".to_string()),
    })
}

fn scaled_field_value(field: &str, value: f64) -> Option<(String, Value)> {
    serde_json::Number::from_f64(value).map(|n| (field.to_string(), Value::Number(n)))
}
