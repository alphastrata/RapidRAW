//! Pure helpers for stamped AI results: source fingerprints, AI-mask result
//! validation and effect-map reconciliation. The job queue is gone; this logic
//! serves the sidecar persistence of the single-image editor.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde_json::{Value, json};

use crate::effect_maps::{EffectMapKind, is_map_reference, is_map_valid_for};
use crate::paths::parse_virtual_path;

const SOURCE_ID_SAMPLE_BYTES: u64 = 64 * 1024;
const SOURCE_ID_TAIL_OFFSET: i64 = 64 * 1024;
const SOURCE_ID_TAIL_THRESHOLD: u64 = 128 * 1024;
const SOURCE_ID_LEN: usize = 32;
const MASK_DATA_KEYS: [&str; 2] = ["maskDataBase64", "mask_data_base64"];
const AI_SOURCE_KEY: &str = "aiSource";

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
        let ids = SOURCE_IDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(source_path.to_path_buf(), (len, mtime, id.clone()));
    Some(id)
}

#[must_use]
pub fn source_id_for(path: &str) -> Option<String> {
    source_id(&parse_virtual_path(path).0)
}

fn mask_data(params: &Value) -> Option<&str> {
    MASK_DATA_KEYS
        .iter()
        .find_map(|key| params.get(*key).and_then(Value::as_str))
        .filter(|data| !data.is_empty())
}

fn is_mask_valid_for(params: &Value, source_id: &str, trust_unstamped: bool) -> bool {
    mask_data(params).is_some()
        && match params.get(AI_SOURCE_KEY).and_then(Value::as_str) {
            Some(stamp) => stamp == source_id,
            None => trust_unstamped,
        }
}

const LEGACY_AI_MASK_TYPES: [&str; 6] = [
    "ai-foreground",
    "ai-sky",
    "ai-text",
    "ai-face",
    "ai-subject",
    "ai-depth",
];

fn ai_masks_mut(adjustments: &mut Value) -> impl Iterator<Item = &mut Value> {
    adjustments
        .get_mut("masks")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|container| container.get_mut("subMasks").and_then(Value::as_array_mut))
        .flatten()
        .filter_map(|sub_mask| {
            (sub_mask
                .get("type")?
                .as_str()
                .is_some_and(|t| LEGACY_AI_MASK_TYPES.contains(&t)))
            .then_some(sub_mask)
        })
}

#[must_use]
pub fn has_ai_results(adjustments: &Value) -> bool {
    adjustments["masks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|container| container["subMasks"].as_array().into_iter().flatten())
        .any(|sub_mask| {
            sub_mask["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("ai-"))
        })
        || adjustments["aiDenoiseEnabled"].as_bool().unwrap_or(false)
        || EffectMapKind::ALL
            .iter()
            .flat_map(|map| map.adjustment_keys())
            .any(|(_, enabled_key, _)| adjustments[*enabled_key].as_bool().unwrap_or(false))
}

fn strip_mask_result(params: &mut Value) {
    if let Some(object) = params.as_object_mut() {
        MASK_DATA_KEYS
            .iter()
            .chain([&AI_SOURCE_KEY])
            .for_each(|key| {
                object.remove(*key);
            });
    }
}

pub fn reconcile_ai_results(
    incoming: &mut Value,
    existing: &Value,
    source_id: &str,
    trust_unstamped: bool,
) -> bool {
    let masks_stripped = ai_masks_mut(incoming).any(|sub_mask| {
        let params = &mut sub_mask["parameters"];
        if is_mask_valid_for(params, source_id, trust_unstamped) {
            params[AI_SOURCE_KEY] = json!(source_id);
            false
        } else {
            strip_mask_result(params);
            true
        }
    });

    EffectMapKind::ALL
        .iter()
        .flat_map(|map| map.adjustment_keys().iter().map(move |keys| (*map, *keys)))
        .fold(masks_stripped, |stripped, (map, (_, _, map_key))| {
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
