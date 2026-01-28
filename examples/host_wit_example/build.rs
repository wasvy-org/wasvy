use std::{env, fs, path::PathBuf};

#[allow(dead_code)]
mod components {
    include!("src/components.rs");
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");

    let settings = wasvy::witgen::WitGeneratorSettings::default();
    let output = wasvy::witgen::generate_wit(&settings);

    write_wit(&manifest_dir.join("wit/components.wit"), &output);
    write_wit(&repo_root.join("target/wasvy/components.wit"), &output);
    write_wit(&manifest_dir.join("target/wasvy/components.wit"), &output);

    println!("cargo:rerun-if-changed=src/components.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wit");
    println!("cargo:rerun-if-changed=wit/components.wit");
}

fn write_wit(path: &PathBuf, output: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create wit directory");
    }
    fs::write(path, output).expect("write wit file");
}
