use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let dest_dir = manifest_dir.join("resources");

    fs::create_dir_all(&dest_dir).unwrap();

    println!("cargo:rustc-env=ORT_LIB_LOCATION={}", dest_dir.display());
    println!("cargo:rustc-env=ORT_STRATEGY=manual");
    println!("cargo:rustc-link-search=native={}", dest_dir.display());

    println!("cargo:rerun-if-changed=build.rs");

    tauri_build::build()
}