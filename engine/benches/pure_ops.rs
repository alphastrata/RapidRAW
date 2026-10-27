use divan::Bencher;

use engine::cache_utils::calculate_transform_hash;
use engine::paths::parse_virtual_path;

fn main() {
    divan::main();
}

const ADJUSTMENTS: &str = r#"{"exposure": 0.4, "contrast": 12, "vibrance": 30, "clarity": 8}"#;

#[divan::bench]
fn transform_hash(bencher: Bencher) {
    let value: serde_json::Value = serde_json::from_str(ADJUSTMENTS).expect("valid adjustments");
    bencher.bench(|| calculate_transform_hash(&value));
}

#[divan::bench]
fn parse_virtual_bench(bencher: Bencher) {
    bencher.bench(|| parse_virtual_path("virtual://albums/holiday/raw_0001.jpg"));
}
