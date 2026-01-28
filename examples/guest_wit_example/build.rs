use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");

    let generated = repo_root.join("target/wasvy/components.wit");
    let dest = manifest_dir.join("wit/deps/game/components.wit");
    let wasvy_src = manifest_dir.join("wit/deps/wasvy/ecs.wit");
    let wasvy_dest = manifest_dir.join("wit/deps/game/deps/wasvy/ecs.wit");

    println!("cargo:rerun-if-changed={}", generated.display());
    println!("cargo:rerun-if-changed={}", wasvy_src.display());

    if generated.exists() {
        let should_copy = match fs::read_to_string(&generated) {
            Ok(contents) => contents.contains("wasvy:type-path="),
            Err(_) => false,
        };
        if should_copy {
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(err) = fs::copy(&generated, &dest) {
                println!("cargo:warning=failed to copy generated WIT: {err}");
            }
        } else {
            println!(
                "cargo:warning=generated WIT missing wasvy:type-path, using checked-in copy"
            );
        }
    } else {
        println!(
            "cargo:warning=generated WIT not found at {}, using checked-in copy",
            generated.display()
        );
    }

    if wasvy_src.exists() {
        if let Some(parent) = wasvy_dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(err) = fs::copy(&wasvy_src, &wasvy_dest) {
            println!("cargo:warning=failed to copy wasvy WIT: {err}");
        }
    } else {
        println!(
            "cargo:warning=wasvy WIT not found at {}, using checked-in copy",
            wasvy_src.display()
        );
    }
}
