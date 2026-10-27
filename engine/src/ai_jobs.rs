use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde_json::{Value, json};

use crate::ai_denoise::{self, DenoiseMethod};
use crate::effect_maps::{EffectMapKind, is_map_reference, is_map_valid_for};
use crate::image_processing::is_effect_enabled;
use crate::paths::parse_virtual_path;

const SOURCE_ID_SAMPLE_BYTES: u64 = 64 * 1024;
const SOURCE_ID_TAIL_OFFSET: i64 = 64 * 1024;
const SOURCE_ID_TAIL_THRESHOLD: u64 = 128 * 1024;
const SOURCE_ID_LEN: usize = 32;
const SUBJECT_BOX_COORDS: usize = 4;
const SUBJECT_BOX_SENTINEL: f64 = -9999.0;
const SUBJECT_BOX_MATCH_TOLERANCE: f64 = 0.5;
const MASK_DATA_KEYS: [&str; 2] = ["maskDataBase64", "mask_data_base64"];
const AI_SOURCE_KEY: &str = "aiSource";
const TEXT_PROMPT_KEY: &str = "prompt";
const FACE_PARTS_KEY: &str = "parts";
const SUBJECT_BOX_KEYS: [&str; 4] = ["startX", "startY", "endX", "endY"];
const MASK_RESULT_KEYS: [&str; 6] = [
    "maskDataBase64",
    "aiSource",
    "rotation",
    "flipHorizontal",
    "flipVertical",
    "orientationSteps",
];
const SUBJECT_RESULT_KEYS: [&str; 6] = [
    "startX",
    "startY",
    "endX",
    "endY",
    "boxSpaceWidth",
    "boxSpaceHeight",
];

type SourceIdCache = HashMap<PathBuf, (u64, u128, String)>;

static SOURCE_IDS: LazyLock<Mutex<SourceIdCache>> = LazyLock::new(Default::default);

pub fn source_id(source_path: &Path) -> Option<String> {
    let meta = fs::metadata(source_path).ok()?;
    let len = meta.len();
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();

    {
        let ids = SOURCE_IDS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_len, cached_mtime, id)) = ids.get(source_path)
            && (*cached_len, *cached_mtime) == (len, mtime)
        {
            return Some(id.clone());
        }
    }

    let mut file = fs::File::open(source_path).ok()?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&len.to_le_bytes());
    let mut head = Vec::new();
    (&mut file)
        .take(SOURCE_ID_SAMPLE_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    hasher.update(&head);
    if len > SOURCE_ID_TAIL_THRESHOLD {
        let mut tail = Vec::new();
        file.seek(SeekFrom::End(SOURCE_ID_TAIL_OFFSET)).ok()?;
        file.read_to_end(&mut tail).ok()?;
        hasher.update(&tail);
    }
    let hex = hasher.finalize().to_hex().to_string();
    let id = hex.get(..SOURCE_ID_LEN).unwrap_or(&hex).to_string();

    SOURCE_IDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(source_path.to_path_buf(), (len, mtime, id.clone()));
    Some(id)
}

pub fn source_id_for(path: &str) -> Option<String> {
    source_id(&parse_virtual_path(path).0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskKind {
    Foreground,
    Sky,
    Text,
    Face,
    Depth,
    Subject,
}

impl MaskKind {
    pub fn from_type(mask_type: &str) -> Option<Self> {
        match mask_type {
            "ai-foreground" => Some(MaskKind::Foreground),
            "ai-sky" => Some(MaskKind::Sky),
            "ai-text" => Some(MaskKind::Text),
            "ai-face" => Some(MaskKind::Face),
            "ai-depth" => Some(MaskKind::Depth),
            "ai-subject" => Some(MaskKind::Subject),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub enum Job {
    Denoise(DenoiseMethod),
    EffectMap(EffectMapKind),
    Mask {
        sub_mask_id: String,
        kind: MaskKind,
        name: String,
    },
}

impl Job {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Job::Denoise(_) => "denoise",
            Job::EffectMap(EffectMapKind::Depth) => "depthMap",
            Job::EffectMap(EffectMapKind::Normal) => "normalMap",
            Job::Mask { kind, .. } => kind_label(*kind),
        }
    }
}

fn kind_label(kind: MaskKind) -> &'static str {
    match kind {
        MaskKind::Foreground => "foregroundMask",
        MaskKind::Sky => "skyMask",
        MaskKind::Text => "textMask",
        MaskKind::Face => "faceMask",
        MaskKind::Depth => "depthMask",
        MaskKind::Subject => "subjectMask",
    }
}

fn mask_data(params: &Value) -> Option<&str> {
    MASK_DATA_KEYS
        .iter()
        .find_map(|key| params.get(*key).and_then(Value::as_str))
        .filter(|data| !data.is_empty())
}

pub fn text_prompt(params: &Value) -> Option<&str> {
    params
        .get(TEXT_PROMPT_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
}

pub fn face_parts(params: &Value) -> Vec<String> {
    params
        .get(FACE_PARTS_KEY)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.as_str().map(str::to_string))
        .collect()
}

fn has_subject_box(params: &Value) -> bool {
    let coords: Vec<f64> = SUBJECT_BOX_KEYS
        .iter()
        .filter_map(|key| params.get(*key).and_then(Value::as_f64))
        .collect();
    coords.len() == SUBJECT_BOX_COORDS
        && coords.iter().all(|c| *c > SUBJECT_BOX_SENTINEL)
        && coords[..2] != coords[2..]
}

pub fn is_mask_valid_for(params: &Value, source_id: &str, trust_unstamped: bool) -> bool {
    mask_data(params).is_some()
        && match params.get(AI_SOURCE_KEY).and_then(Value::as_str) {
            Some(stamp) => stamp == source_id,
            None => trust_unstamped,
        }
}

pub fn needs_mask_generation(kind: MaskKind, params: &Value, source_id: &str) -> bool {
    !is_mask_valid_for(params, source_id, true)
        && match kind {
            MaskKind::Subject => has_subject_box(params),
            MaskKind::Text => text_prompt(params).is_some(),
            MaskKind::Face => !face_parts(params).is_empty(),
            _ => true,
        }
}

pub fn ai_sub_masks_mut(adjustments: &mut Value) -> impl Iterator<Item = (MaskKind, &mut Value)> {
    adjustments
        .get_mut("masks")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|container| container.get_mut("subMasks").and_then(Value::as_array_mut))
        .flatten()
        .filter_map(|sub_mask| {
            let kind = MaskKind::from_type(sub_mask.get("type")?.as_str()?)?;
            Some((kind, sub_mask))
        })
}

pub fn ai_sub_masks(adjustments: &Value) -> impl Iterator<Item = (MaskKind, &Value)> {
    adjustments["masks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|container| container["subMasks"].as_array().into_iter().flatten())
        .filter_map(|sub_mask| Some((MaskKind::from_type(sub_mask["type"].as_str()?)?, sub_mask)))
}

pub fn has_ai_results(adjustments: &Value) -> bool {
    ai_sub_masks(adjustments).next().is_some()
        || adjustments["aiDenoiseEnabled"].as_bool().unwrap_or(false)
        || EffectMapKind::ALL
            .iter()
            .flat_map(|map| map.adjustment_keys())
            .any(|(_, enabled_key, _)| adjustments[*enabled_key].as_bool().unwrap_or(false))
}

pub fn sub_mask_id(sub_mask: &Value) -> Option<&str> {
    sub_mask.get("id").and_then(Value::as_str)
}

pub fn plan(adjustments: &Value, path: &str, source_id: &str) -> Vec<Job> {
    let denoise_method = DenoiseMethod::from_adjustments(adjustments);
    let denoise = (ai_denoise::is_enabled(adjustments)
        && !ai_denoise::has_cached_entry(path, denoise_method))
    .then_some(Job::Denoise(denoise_method));

    let maps = EffectMapKind::ALL
        .iter()
        .copied()
        .filter(|map| {
            map.adjustment_keys()
                .iter()
                .any(|(tool, enabled_key, map_key)| {
                    is_effect_enabled(adjustments, tool, enabled_key)
                        && !is_map_valid_for(&adjustments[*map_key], *map, source_id)
                })
        })
        .map(Job::EffectMap);

    let masks = adjustments["masks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|container| {
            let name = container["name"].as_str().unwrap_or_default();
            container["subMasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |sub_mask| {
                    let kind = sub_mask["type"].as_str().and_then(MaskKind::from_type)?;
                    let id = sub_mask_id(sub_mask)?;
                    needs_mask_generation(kind, &sub_mask["parameters"], source_id).then(|| {
                        Job::Mask {
                            sub_mask_id: id.to_string(),
                            kind,
                            name: name.to_string(),
                        }
                    })
                })
        });

    denoise.into_iter().chain(maps).chain(masks).collect()
}

pub fn strip_mask_result(params: &mut Value) {
    if let Some(object) = params.as_object_mut() {
        MASK_DATA_KEYS
            .iter()
            .chain([&AI_SOURCE_KEY])
            .for_each(|key| {
                object.remove(*key);
            });
    }
}

pub fn same_mask_inputs(a: &Value, b: &Value) -> bool {
    let (mut a, mut b) = (a.clone(), b.clone());
    strip_mask_result(&mut a);
    strip_mask_result(&mut b);
    a == b
}

fn box_space(params: &Value) -> Option<(f64, f64)> {
    Some((
        params.get("boxSpaceWidth")?.as_f64()?,
        params.get("boxSpaceHeight")?.as_f64()?,
    ))
}

pub fn scaled_subject_box(
    params: &Value,
    target_space: Option<(f64, f64)>,
) -> Option<[f64; SUBJECT_BOX_COORDS]> {
    let coords: Option<[f64; SUBJECT_BOX_COORDS]> = {
        let mut coords = [0.0; SUBJECT_BOX_COORDS];
        coords
            .iter_mut()
            .zip(SUBJECT_BOX_KEYS)
            .try_fold((), |(), (coord, key)| {
                Some(*coord = params.get(key)?.as_f64()?)
            })
            .map(|()| coords)
    };
    let mut coords = coords?;
    if let (Some((from_w, from_h)), Some((to_w, to_h))) = (box_space(params), target_space)
        && from_w > 0.0
        && from_h > 0.0
    {
        let (sx, sy) = (to_w / from_w, to_h / from_h);
        coords = [
            coords[0] * sx,
            coords[1] * sy,
            coords[2] * sx,
            coords[3] * sy,
        ];
    }
    Some(coords)
}

pub fn same_subject_box(incoming: &Value, existing: &Value) -> bool {
    match (
        scaled_subject_box(incoming, box_space(existing)),
        scaled_subject_box(existing, None),
    ) {
        (Some(a), Some(b)) => a
            .iter()
            .zip(b)
            .all(|(a, b)| (a - b).abs() < SUBJECT_BOX_MATCH_TOLERANCE),
        _ => false,
    }
}

pub fn copy_mask_result(kind: MaskKind, target: &mut Value, result: &Value) {
    let keys = MASK_RESULT_KEYS.iter().chain(if kind == MaskKind::Subject {
        SUBJECT_RESULT_KEYS.iter()
    } else {
        [].iter()
    });
    keys.for_each(|key| {
        result.get(*key).map(|value| {
            target[*key] = value.clone();
        });
    });
}

pub fn reconcile_ai_results(
    incoming: &mut Value,
    existing: &Value,
    source_id: &str,
    trust_unstamped: bool,
) -> bool {
    let existing_sub_masks: HashMap<&str, (MaskKind, &Value)> = ai_sub_masks(existing)
        .filter_map(|(kind, sub_mask)| {
            Some((sub_mask_id(sub_mask)?, (kind, &sub_mask["parameters"])))
        })
        .collect();

    let stripped = ai_sub_masks_mut(incoming).fold(false, |stripped, (kind, sub_mask)| {
        let existing = sub_mask_id(sub_mask).and_then(|id| existing_sub_masks.get(id));
        let params = &mut sub_mask["parameters"];
        if is_mask_valid_for(params, source_id, trust_unstamped) {
            params[AI_SOURCE_KEY] = json!(source_id);
            return stripped;
        }

        match existing.filter(|(existing_kind, existing)| {
            *existing_kind == kind
                && is_mask_valid_for(existing, source_id, true)
                && match kind {
                    MaskKind::Subject => same_subject_box(params, existing),
                    MaskKind::Text => text_prompt(params) == text_prompt(existing),
                    MaskKind::Face => face_parts(params) == face_parts(existing),
                    _ => true,
                }
        }) {
            Some((_, existing)) => {
                copy_mask_result(kind, params, existing);
                params[AI_SOURCE_KEY] = json!(source_id);
                stripped
            }
            None if mask_data(params).is_some() => {
                strip_mask_result(params);
                true
            }
            None => stripped,
        }
    });

    EffectMapKind::ALL
        .iter()
        .flat_map(|map| map.adjustment_keys().iter().map(move |keys| (*map, *keys)))
        .fold(stripped, |stripped, (map, (_, _, map_key))| {
            let Some((trusted, was_null)) = incoming
                .get(map_key)
                .map(|value| (trust_unstamped || is_map_reference(value), value.is_null()))
            else {
                return stripped;
            };
            if trusted && is_map_valid_for(&incoming[map_key], map, source_id) {
                return stripped;
            }
            if is_map_valid_for(&existing[map_key], map, source_id) {
                incoming[map_key] = existing[map_key].clone();
            } else if !was_null {
                incoming[map_key] = Value::Null;
                return true;
            }
            stripped
        })
}
