use std::fs;

use bevy_app::App;
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::World;
use wasvy::module_plugin::WasvyWorkspacePlugin;
use wasvy::modules::{ModuleGeneration, ModuleId, ModuleSystemSet};
use wasvy::workspace::{WorkspaceConfigPath, WorkspaceInventory, WorldComposition};

#[test]
fn module_id_is_hashable_and_stable() {
    let a = ModuleId::new("combat");
    let b = ModuleId::new("combat");
    let c = ModuleId::new("ai");

    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.to_string(), "combat");
    assert_eq!(a.as_str(), "combat");
}

#[test]
fn module_system_set_distinguishes_generations() {
    let a = ModuleSystemSet::generation("combat", ModuleGeneration(1));
    let b = ModuleSystemSet::generation("combat", ModuleGeneration(2));
    let c = ModuleSystemSet::module("combat");

    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
}

#[test]
fn workspace_plugin_builds_and_seeds_resources() {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(WasvyWorkspacePlugin::new("wasvy.toml").with_modules(["combat", "ai"]));

    let world: &World = app.world();

    assert!(world.get_resource::<WorkspaceInventory>().is_some());

    let config = world
        .get_resource::<WorkspaceConfigPath>()
        .expect("config path installed");
    assert_eq!(config.0.to_string_lossy(), "wasvy.toml");

    let composition = world
        .get_resource::<WorldComposition>()
        .expect("world composition installed");
    assert!(composition.includes(&ModuleId::new("combat")));
    assert!(composition.includes(&ModuleId::new("ai")));
}

#[test]
fn workspace_plugin_reads_manifest_inventory_and_default_world() {
    let dir = std::env::temp_dir().join(format!("wasvy-modules-runtime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let manifest = dir.join("wasvy.toml");
    fs::write(
        &manifest,
        r#"
[[module]]
name = "combat"
path = "crates/modules/combat"

[[module]]
name = "ai"
path = "crates/modules/ai"

[world]
modules = ["ai"]
"#,
    )
    .unwrap();

    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(WasvyWorkspacePlugin::new(&manifest));

    let world: &World = app.world();
    let inventory = world
        .get_resource::<WorkspaceInventory>()
        .expect("inventory installed from manifest");
    assert_eq!(inventory.modules.len(), 2);

    let composition = world
        .get_resource::<WorldComposition>()
        .expect("composition installed from manifest");
    assert!(composition.includes(&ModuleId::new("ai")));
    assert!(!composition.includes(&ModuleId::new("combat")));
}
