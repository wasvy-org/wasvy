//! A mod's native systems are replaced by a wasm build of the same code, which hot reloads, and then restored.
//!
//! The native Bevy systems are converted into a [ModAsset] and saved like any other asset. The mod running them
//! switches to the wasm asset with [Mod::set_asset], which takes over once it has loaded, and back to the saved
//! native asset when the wasm build is no longer wanted.
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{Duration, Instant},
};

use bevy_app::{App, Last, TaskPoolPlugin};
use bevy_asset::{
    AssetApp, AssetEvent, AssetLoadFailedEvent, AssetPlugin, AssetServer, Assets, Handle,
    io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    },
};
use bevy_ecs::prelude::*;
use wasvy_runtime::{
    access::ModAccess,
    asset::{ModAsset, ModDiscovery},
    cleanup::DespawnModEntity,
    commands::RuntimeCommands,
    mods::Mod,
    plugin::ModRuntimePlugin,
    schedule::ModScheduleName,
    system::ModSystemDefinition,
};
use wasvy_wasm::WasmLoaderPlugin;

/// Where the game loads the wasm build of `mods_hot_reloaded` from
const WASM: &str = "mods_hot_reloaded.wasm";

#[derive(Component)]
struct First;
#[derive(Component)]
struct NativeOnly;

fn native_first(mut commands: RuntimeCommands) {
    commands.spawn(First);
}

/// Has no counterpart in either wasm revision
fn native_only(mut commands: RuntimeCommands) {
    commands.spawn(NativeOnly);
}

/// The systems of the `mods_hot_reloaded` crate compiled into the game, converted into a mod asset
fn native_asset() -> ModAsset {
    ModAsset::new(ModDiscovery {
        systems: vec![
            // named like the wasm system on purpose: a mod's systems are replaced as a whole, not by name
            ModSystemDefinition::new(native_first).in_schedules([ModScheduleName::Update]),
            ModSystemDefinition::new(native_only).in_schedules([ModScheduleName::Update]),
        ],
        ..Default::default()
    })
}

/// The in-memory folder the asset server loads mods from
#[derive(Resource)]
struct ModFiles(Dir);

/// Loads and reloads that finished, successfully or not
#[derive(Resource, Default)]
struct FinishedLoads(usize);

fn count_finished_loads(
    mut events: MessageReader<AssetEvent<ModAsset>>,
    mut failures: MessageReader<AssetLoadFailedEvent<ModAsset>>,
    mut finished: ResMut<FinishedLoads>,
) {
    finished.0 += events
        .read()
        .filter(|event| matches!(event, AssetEvent::LoadedWithDependencies { .. }))
        .count();
    finished.0 += failures.read().count();
}

/// A game that runs the native systems as a mod with access to the world.
///
/// Returns the mod, and the native asset saved so that the mod can switch back to it.
fn app() -> (App, Entity, Handle<ModAsset>) {
    let files = Dir::default();
    let reader = files.clone();
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || {
            Box::new(MemoryAssetReader {
                root: reader.clone(),
            })
        }),
    )
    .add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            watch_for_changes_override: Some(false),
            ..Default::default()
        },
        ModRuntimePlugin::default(),
        WasmLoaderPlugin::default(),
    ))
    .insert_resource(ModFiles(files))
    .init_resource::<FinishedLoads>()
    .add_systems(Last, count_finished_loads);
    app.finish();
    app.cleanup();

    let native = app
        .world_mut()
        .resource_mut::<Assets<ModAsset>>()
        .add(native_asset());
    let mut mod_component = Mod::new(native.clone());
    mod_component.enable_access(ModAccess::World);
    let mod_id = app
        .world_mut()
        .spawn((mod_component, Name::new("mods_hot_reloaded")))
        .id();
    (app, mod_id, native)
}

/// (native `first`, native `native_only`, wasm systems) spawned by one more frame
fn per_frame(app: &mut App) -> (usize, usize, usize) {
    let count = |app: &mut App| {
        let world = app.world_mut();
        (
            world.query::<&First>().iter(world).count(),
            world.query::<&NativeOnly>().iter(world).count(),
            // the wasm systems spawn empty entities, owned by the mod like the native ones
            world
                .query_filtered::<(), (With<DespawnModEntity>, Without<First>, Without<NativeOnly>)>()
                .iter(world)
                .count(),
        )
    };
    let before = count(app);
    app.update();
    let after = count(app);
    (after.0 - before.0, after.1 - before.1, after.2 - before.2)
}

fn set_asset(app: &mut App, mod_id: Entity, asset: Handle<ModAsset>) {
    app.world_mut()
        .get_mut::<Mod>(mod_id)
        .unwrap()
        .set_asset(asset);
}

/// Runs `start`, then frames until the load it started has finished (successfully or not) and the runtime had a
/// frame to set up the result. Loading compiles wasm on the IO task pool, so this can take a moment.
fn finish_loading(app: &mut App, start: impl FnOnce(&mut App)) {
    let finished = app.world().resource::<FinishedLoads>().0;
    start(app);
    let deadline = Instant::now() + Duration::from_secs(60);
    while app.world().resource::<FinishedLoads>().0 == finished {
        assert!(
            Instant::now() < deadline,
            "the asset never finished loading"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }
    // mods are set up in the frame after their asset has loaded
    app.update();
}

fn write_wasm(app: &App, bytes: &[u8]) {
    app.world()
        .resource::<ModFiles>()
        .0
        .insert_asset(Path::new(WASM), bytes.to_vec());
}

/// Switches the mod to the wasm build, as a game would to try it out in place of the native systems
fn switch_to_wasm(app: &mut App, mod_id: Entity, bytes: &[u8]) {
    write_wasm(app, bytes);
    let wasm = app.world().resource::<AssetServer>().load(WASM);
    finish_loading(app, |app| set_asset(app, mod_id, wasm));
}

/// Overwrites the wasm build and reloads it, like Bevy's file watcher does when a new build replaces it
fn reload(app: &mut App, bytes: &[u8]) {
    write_wasm(app, bytes);
    finish_loading(app, |app| {
        app.world().resource::<AssetServer>().reload(WASM)
    });
}

/// Revisions of the `loader_discovery` fixture guest, built once:
/// - revision 1 has the systems `first` (startup and update) and `second` (update)
/// - revision 2 (feature `rev2`) only has `first`
fn revisions() -> (&'static [u8], &'static [u8]) {
    static REVISIONS: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();
    let (rev1, rev2) = REVISIONS.get_or_init(|| (build_guest(None), build_guest(Some("rev2"))));
    (rev1, rev2)
}

fn build_guest(feature: Option<&str>) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target");
    let mut command = Command::new(env!("CARGO"));
    command
        .current_dir(&root)
        .args([
            "build",
            "--offline",
            "--target",
            "wasm32-wasip2",
            "-p",
            "wasvy_loader_fixture",
            "--target-dir",
        ])
        .arg(&target);
    if let Some(feature) = feature {
        command.args(["--features", feature]);
    }
    let output = command.output().expect("cargo builds guest fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(target.join("wasm32-wasip2/debug/wasvy_loader_fixture.wasm")).unwrap()
}

#[test]
fn wasm_replaces_native_systems_and_switching_back_restores_them() {
    let (rev1, rev2) = revisions();
    let (mut app, mod_id, native) = app();
    assert_eq!(
        per_frame(&mut app),
        (1, 1, 0),
        "native systems run on their own"
    );
    assert_eq!(per_frame(&mut app), (1, 1, 0));

    switch_to_wasm(&mut app, mod_id, rev1);
    // the wasm build replaced the entities the native systems spawned
    let world = app.world_mut();
    assert_eq!(world.query::<&First>().iter(world).count(), 0);
    assert_eq!(world.query::<&NativeOnly>().iter(world).count(), 0);
    // rev1 provides `first` and `second`; no native system runs anymore, including `native_only`
    assert_eq!(per_frame(&mut app), (0, 0, 2));

    // hot reload to rev2 (no `second`): still no native systems, no duplicates
    reload(&mut app, rev2);
    assert_eq!(per_frame(&mut app), (0, 0, 1));
    reload(&mut app, rev1);
    assert_eq!(per_frame(&mut app), (0, 0, 2));

    // switching back to the saved native asset: wasm is gone and the native systems are back, exactly once
    set_asset(&mut app, mod_id, native);
    // the native asset is loaded already, so the next frame sets the mod up again (despawning what wasm spawned)
    app.update();
    assert_eq!(per_frame(&mut app), (1, 1, 0));
    assert_eq!(per_frame(&mut app), (1, 1, 0));
}

#[test]
fn broken_wasm_keeps_native_running() {
    let (rev1, _) = revisions();
    let (mut app, mod_id, _) = app();
    assert_eq!(per_frame(&mut app), (1, 1, 0));

    // a broken build never loads
    switch_to_wasm(&mut app, mod_id, b"definitely not wasm");
    assert_eq!(per_frame(&mut app), (1, 1, 0), "native still runs");

    // fixing the build hot reloads it, and it takes over
    reload(&mut app, rev1);
    assert_eq!(per_frame(&mut app), (0, 0, 2));

    // breaking it again keeps the last working revision, native stays away
    reload(&mut app, b"broken again");
    assert_eq!(per_frame(&mut app), (0, 0, 2));
}
