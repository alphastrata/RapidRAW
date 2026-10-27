pub const COLOR_TAG_PREFIX: &str = "color:";
pub const USER_TAG_PREFIX: &str = "user:";

mod candidates;
mod hierarchy;

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use image::{DynamicImage, imageops::FilterType};
use ndarray::{Array, Array2, Array4, Axis};
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

pub use self::candidates::TAG_CANDIDATES;
pub use self::hierarchy::TAG_HIERARCHY;

use crate::ai_processing::gpu_inference;

fn preprocess_clip_image(image: &DynamicImage) -> Result<Array4<f32>> {
    let input_size: usize = 224;
    let resized = image.resize_to_fill(
        u32::try_from(input_size)?,
        u32::try_from(input_size)?,
        FilterType::Triangle,
    );
    let rgb_image = resized.to_rgb8();

    let mean = [0.481_454_66, 0.457_827_5, 0.408_210_73];
    let std = [0.268_629_54, 0.261_302_6, 0.275_777_1];

    let channel = |channel: usize| {
        rgb_image
            .pixels()
            .map(move |pixel| (f32::from(pixel[channel]) / 255.0 - mean[channel]) / std[channel])
    };

    let values: Vec<f32> = (0..3).flat_map(channel).collect();
    Ok(Array::from_shape_vec(
        (1, 3, input_size, input_size),
        values,
    )?)
}

fn softmax(array: &Array2<f32>) -> Array2<f32> {
    let mut new_array = array.clone();
    new_array.axis_iter_mut(Axis(0)).for_each(|mut row| {
        let max_val = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        row.mapv_inplace(|x| (x - max_val).exp());
        let sum = row.sum();
        if sum > 0.0 {
            row.mapv_inplace(|x| x / sum);
        }
    });
    new_array
}

fn rgb_to_hsv(rgb: (u8, u8, u8)) -> (f32, f32, f32) {
    let red = f32::from(rgb.0) / 255.0;
    let green = f32::from(rgb.1) / 255.0;
    let blue = f32::from(rgb.2) / 255.0;

    let value = red.max(green).max(blue);
    let delta = value - red.min(green).min(blue);

    let hue = if delta.abs() < f32::EPSILON {
        0.0
    } else if (value - red).abs() < f32::EPSILON {
        60.0 * (((green - blue) / delta) % 6.0)
    } else if (value - green).abs() < f32::EPSILON {
        60.0 * (((blue - red) / delta) + 2.0)
    } else {
        60.0 * (((red - green) / delta) + 4.0)
    };
    let hue = if hue < 0.0 { hue + 360.0 } else { hue };

    let saturation = if value.abs() < f32::EPSILON {
        0.0
    } else {
        delta / value
    };

    (hue, saturation, value)
}

fn classify_color(rgb: (u8, u8, u8)) -> String {
    let (h, s, v) = rgb_to_hsv(rgb);

    let color_name = if v < 0.2 {
        "black".to_string()
    } else if s < 0.1 {
        if v > 0.8 {
            "white".to_string()
        } else {
            "gray".to_string()
        }
    } else {
        match h {
            _ if !(20.0..340.0).contains(&h) => "red".to_string(),
            _ if (20.0..45.0).contains(&h) => "orange".to_string(),
            _ if (45.0..70.0).contains(&h) => "yellow".to_string(),
            _ if (70.0..160.0).contains(&h) => "green".to_string(),
            _ if (160.0..260.0).contains(&h) => "blue".to_string(),
            _ if (260.0..340.0).contains(&h) => "purple".to_string(),
            _ => "unknown".to_string(),
        }
    };

    if (color_name == "orange" || color_name == "red") && v < 0.6 && s < 0.7 {
        "brown".to_string()
    } else {
        color_name
    }
}

pub fn extract_color_tags(image: &DynamicImage) -> Vec<String> {
    let resized = image.resize(100, 100, FilterType::Triangle);
    let rgb_image = resized.to_rgb8();

    let color_counts: HashMap<String, u32> = rgb_image
        .pixels()
        .map(|pixel| (pixel[0], pixel[1], pixel[2]))
        .map(classify_color)
        .fold(HashMap::new(), |mut counts, name| {
            *counts.entry(name).or_default() += 1;
            counts
        });

    let mut vivid_tags: Vec<(String, u32)> = color_counts
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "black" | "white" | "gray"))
        .map(|(name, count)| (name.clone(), *count))
        .collect();

    vivid_tags.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    if vivid_tags.is_empty() {
        color_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map_or_default(|(name, _)| vec![name])
    } else {
        vivid_tags
            .into_iter()
            .take(2)
            .map(|(name, _)| name)
            .collect()
    }
}

#[doc = "Generates AI tags for an image using an already initialised CLIP session."]
#[doc = "\n\n# Errors\n\nFails if the tokenizer or ONNX session rejects the inputs, or the outputs cannot be decoded."]
pub fn generate_tags_with_clip(
    image: &DynamicImage,
    clip_session_mutex: &Mutex<Session>,
    tokenizer: &Tokenizer,
    custom_tags: Option<Vec<String>>,
    max_tags: usize,
) -> Result<Vec<String>> {
    let image_input = preprocess_clip_image(image)?;

    let is_custom = custom_tags.as_ref().is_some_and(|t| !t.is_empty());
    let text_inputs: Vec<String> = if is_custom {
        custom_tags.unwrap_or_default()
    } else {
        TAG_CANDIDATES.iter().map(|&s| s.to_owned()).collect()
    };

    let encodings = tokenizer
        .encode_batch(text_inputs.clone(), true)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let max_len = encodings
        .iter()
        .map(|e| e.get_ids().len())
        .max()
        .unwrap_or(0);

    let (ids_data, mask_data): (Vec<i64>, Vec<i64>) = encodings.iter().fold(
        (Vec::new(), Vec::new()),
        |(mut ids, mut masks), encoding| {
            let mut id_row: Vec<i64> = encoding
                .get_ids()
                .iter()
                .map(|&i| i64::from(i))
                .collect::<Vec<_>>();
            let mut mask_row: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&m| i64::from(m))
                .collect::<Vec<_>>();
            id_row.resize(max_len, 0);
            mask_row.resize(max_len, 0);
            ids.extend(id_row);
            masks.extend(mask_row);
            (ids, masks)
        },
    );

    let ids_array = Array::from_shape_vec((text_inputs.len(), max_len), ids_data)?;
    let mask_array = Array::from_shape_vec((text_inputs.len(), max_len), mask_data)?;

    let image_input_dyn = image_input.into_dyn();
    let ids_array_dyn = ids_array.into_dyn();
    let mask_array_dyn = mask_array.into_dyn();

    let image_layout = image_input_dyn.as_standard_layout();
    let ids_layout = ids_array_dyn.as_standard_layout();
    let mask_layout = mask_array_dyn.as_standard_layout();

    let image_val = Tensor::from_array(image_layout.into_owned())?;
    let ids_val = Tensor::from_array(ids_layout.into_owned())?;
    let mask_val = Tensor::from_array(mask_layout.into_owned())?;

    let mut clip_session = clip_session_mutex
        .lock()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let outputs = gpu_inference(
        clip_session.run(ort::inputs![ids_val, image_val, mask_val]),
    )?;

    let logits = outputs[0]
        .try_extract_array::<f32>()?
        .to_owned()
        .into_dimensionality::<ndarray::Dim<[usize; 2]>>()?;
    let probs = softmax(&logits);

    let confidence_threshold = 0.005;
    let mut scored_tags: Vec<(String, f32)> = probs
        .row(0)
        .iter()
        .zip(&text_inputs)
        .filter(|(prob, _)| **prob > confidence_threshold)
        .map(|(prob, tag)| (tag.clone(), *prob))
        .collect();

    scored_tags.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let ranked_tags: Vec<String> = scored_tags.into_iter().map(|(tag, _)| tag).collect();

    let color_tags = if is_custom {
        Vec::new()
    } else {
        extract_color_tags(image)
    };

    Ok(limit_tags(&ranked_tags, !is_custom, &color_tags, max_tags))
}

fn limit_tags(
    ranked_tags: &[String],
    add_parents: bool,
    color_tags: &[String],
    max_tags: usize,
) -> Vec<String> {
    ranked_tags
        .iter()
        .flat_map(|tag| {
            let mut chain = vec![tag.as_str()];
            if add_parents && let Some(parents) = TAG_HIERARCHY.get(tag.as_str()) {
                chain.extend(parents.iter().copied());
            }
            chain
        })
        .chain(color_tags.iter().map(String::as_str))
        .fold(Vec::new(), |mut final_tags: Vec<String>, tag| {
            if final_tags.len() < max_tags && !final_tags.iter().any(|t| t == tag) {
                final_tags.push(tag.to_owned());
            }
            final_tags
        })
}