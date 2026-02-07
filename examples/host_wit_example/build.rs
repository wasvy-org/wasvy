use std::{env, fs, path::PathBuf};

use bevy_app::App;
use bevy_ecs::prelude::AppTypeRegistry;
use bevy_ecs::reflect::AppFunctionRegistry;

wasvy::include_wasvy_components!("src");

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");

    let mut app = App::new();
    app.init_resource::<AppFunctionRegistry>();
    wasvy::authoring::register_all(&mut app);
    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry");

    let settings = wasvy::witgen::WitGeneratorSettings::default();
    let output = wasvy::witgen::generate_wit(&settings, type_registry, function_registry);

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
