use engine::cache_utils::calculate_transform_hash;
use engine::paths::parse_virtual_path;

#[test]
fn transform_hash_is_stable_and_order_sensitive() {
    let rotated: serde_json::Value =
        serde_json::from_str(r#"{"rotation": 90}"#).expect("valid json");
    let flipped: serde_json::Value =
        serde_json::from_str(r#"{"flipHorizontal": true}"#).expect("valid json");
    assert_ne!(
        calculate_transform_hash(&rotated),
        calculate_transform_hash(&flipped)
    );
    assert_eq!(
        calculate_transform_hash(&rotated),
        calculate_transform_hash(&rotated)
    );
}

#[test]
fn virtual_path_splits_base_and_copy_id() {
    let (base, sidecar) = parse_virtual_path("IMG_0001.CR2?vc=1");
    let (plain, plain_sidecar) = parse_virtual_path("IMG_0001.CR2");
    assert_eq!(base, plain);
    assert_ne!(sidecar, plain_sidecar);
}
