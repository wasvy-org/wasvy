use bevy_app::Update;
use bevy_asset::{AssetId, Handle};
use bevy_platform::collections::{HashMap, HashSet};

use super::*;
use crate::{
    access::ModAccess,
    asset::{ModDiscovery, ModReplacement},
    mods::ModDespawnBehaviour,
    sandbox::Sandboxed,
    schedule::{ModScheduleName, ModSchedules},
    system::ModSystem,
};

#[derive(Resource, Default)]
struct Runs(Vec<u32>);

fn definition(name: &str, value: u32) -> ModSystem {
    let mut definition = ModSystem::new(move |mut runs: ResMut<Runs>| runs.0.push(value))
        .in_schedules([ModScheduleName::Update]);
    definition.name = name.to_owned().into();
    definition
}

fn asset(namespace: &str, systems: &[(&str, u32)], incomplete: bool) -> ModAsset {
    let mut discovery = ModDiscovery {
        systems: systems
            .iter()
            .map(|(name, value)| definition(name, *value))
            .collect(),
        ..Default::default()
    };
    if incomplete {
        discovery.errors.push(anyhow::anyhow!("setup trapped"));
    }
    ModAsset::new(namespace.to_owned(), discovery).with_source_retention()
}

fn world() -> World {
    let mut world = World::new();
    world.init_resource::<Assets<ModAsset>>();
    world.init_resource::<Assets<ModSystemAsset>>();
    world.init_resource::<Schedules>();
    world.init_resource::<ModSchedules>();
    world.init_resource::<Runs>();
    world.init_resource::<Messages<AssetEvent<ModAsset>>>();
    world.init_resource::<TestRevisions>();
    world.insert_resource(ModDespawnBehaviour::None);
    world.register_component::<Sandboxed>();
    world
        .resource_mut::<Schedules>()
        .insert(Schedule::new(Update));
    world
        .resource_mut::<Schedules>()
        .insert(ModStartup::new_schedule());
    world
}

fn load(world: &mut World, asset: ModAsset, policy: ModReplacement) -> (Entity, Handle<ModAsset>) {
    let handle = world
        .resource_mut::<Assets<ModAsset>>()
        .add(asset.with_settings(policy));
    let mut module = Mod::new(handle.clone());
    module.enable_access(ModAccess::World);
    (world.spawn(module).id(), handle)
}

/// Publishes a new revision with the same settings, as the AssetServer reapplies loader settings on reload.
fn reload(world: &mut World, handle: &Handle<ModAsset>, asset: ModAsset) {
    let mut assets = world.resource_mut::<Assets<ModAsset>>();
    let settings = assets.get(handle).unwrap().settings().clone();
    assets
        .insert(handle.id(), asset.with_settings(settings))
        .unwrap();
}

#[derive(Resource, Default)]
struct TestRevisions(HashMap<AssetId<ModAsset>, u64>);

fn tick(world: &mut World) -> Vec<u32> {
    // These unit worlds have no AssetPlugin; simulate its publication events.
    let mut changes: Vec<_> = world
        .resource::<Assets<ModAsset>>()
        .iter()
        .filter(|(id, asset)| world.resource::<TestRevisions>().0.get(id) != Some(&asset.revision))
        .map(|(id, asset)| (asset.revision, id))
        .collect();
    changes.sort_by_key(|(revision, _)| *revision);
    for (revision, id) in changes {
        world.resource_mut::<TestRevisions>().0.insert(id, revision);
        world.write_message(AssetEvent::Modified { id });
    }
    world.run_system_cached(run_setup).unwrap();
    world.run_schedule(Update);
    let mut runs = std::mem::take(&mut world.resource_mut::<Runs>().0);
    runs.sort();
    runs
}

#[test]
fn provider_updates_replacements_after_its_mod_is_removed() {
    let mut world = world();
    load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    let (provider, handle) = load(
        &mut world,
        asset("patch", &[("game::a", 2)], false),
        ModReplacement::Only(HashSet::from(["game::a".into()])),
    );
    assert_eq!(tick(&mut world), [2]);
    world.entity_mut(provider).remove::<Mod>();
    assert_eq!(tick(&mut world), [2]);
    reload(
        &mut world,
        &handle,
        asset("patch", &[("game::a", 3)], false),
    );
    assert_eq!(tick(&mut world), [3]);
}

#[test]
fn publication_order_is_independent_of_construction_order() {
    let mut world = world();
    let patch = asset("patch", &[("game::a", 2)], false);
    let native = asset("game", &[("game::a", 1)], false);
    let (_, native) = load(&mut world, native, ModReplacement::Namespace);
    let (_, patch) = load(&mut world, patch, ModReplacement::Any);
    world.write_message(AssetEvent::Added { id: native.id() });
    world.write_message(AssetEvent::Added { id: patch.id() });
    world.run_system_cached(run_setup).unwrap();
    world.run_schedule(Update);
    assert_eq!(world.resource::<Runs>().0, [2]);
}

#[test]
fn setup_pipeline_preserves_local_state_without_spawning_entities() {
    let mut world = world();
    let mut setup = Schedule::default();
    setup.add_systems(run_setup);
    let (_, native) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    let (_, patch) = load(
        &mut world,
        asset("patch", &[("game::a", 2)], false),
        ModReplacement::default(),
    );
    let entities = world.entities().len();
    world.write_message(AssetEvent::Added { id: native.id() });
    world.write_message(AssetEvent::Added { id: patch.id() });
    setup.run(&mut world);
    world.run_schedule(Update);
    assert_eq!(world.resource::<Runs>().0, [2]);
    assert_eq!(world.entities().len(), entities);

    world.resource_mut::<Runs>().0.clear();
    reload(&mut world, &patch, asset("patch", &[], false));
    world.write_message(AssetEvent::Modified { id: patch.id() });
    setup.run(&mut world);
    world.run_schedule(Update);
    assert!(world.resource::<Runs>().0.is_empty());
    setup.run(&mut world);
    world.run_schedule(Update);
    assert!(world.resource::<Runs>().0.is_empty());
    assert_eq!(world.entities().len(), entities);
}

#[test]
fn removing_last_consumer_releases_namespace_for_waiting_asset() {
    let mut world = world();
    let (original, _) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    load(
        &mut world,
        asset("game", &[("game::a", 2)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);

    world.entity_mut(original).remove::<Mod>();
    assert_eq!(tick(&mut world), [2]);
    assert_eq!(tick(&mut world), [2]);
}

#[test]
fn pending_asset_switch_retains_copies_but_respects_access_removal() {
    let mut world = world();
    let (entity, _) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    let pending = world.resource::<Assets<ModAsset>>().reserve_handle();
    world.get_mut::<Mod>(entity).unwrap().set_asset(pending);
    assert_eq!(tick(&mut world), [1]);
    world
        .get_mut::<Mod>(entity)
        .unwrap()
        .disable_access(&ModAccess::World);
    assert!(tick(&mut world).is_empty());
}

#[test]
fn failed_factory_preserves_the_old_revision() {
    struct Fail;
    // Safety: this factory never returns a system.
    unsafe impl crate::system::ModSystemBuild for Fail {
        fn build(
            &self,
            _: crate::system::ModSystemContext<'_>,
        ) -> anyhow::Result<bevy_ecs::system::BoxedSystem> {
            anyhow::bail!("cannot construct candidate")
        }
    }
    let mut world = world();
    let (_, handle) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    let failure = ModSystem::custom("game::a", Fail).in_schedules([ModScheduleName::Update]);
    reload(
        &mut world,
        &handle,
        ModAsset::new(
            "game",
            ModDiscovery {
                systems: vec![failure],
                ..Default::default()
            },
        ),
    );
    assert_eq!(tick(&mut world), [1]);
    assert_eq!(tick(&mut world), [1]);
}

#[test]
fn partial_schedule_build_preserves_all_old_copies() {
    use std::sync::atomic::{AtomicBool, Ordering};

    use bevy_app::PostUpdate;

    struct FailSecondBuild(AtomicBool);

    // Safety: successful builds delegate to the native builder's access restrictions.
    unsafe impl crate::system::ModSystemBuild for FailSecondBuild {
        fn build(
            &self,
            context: crate::system::ModSystemContext<'_>,
        ) -> anyhow::Result<bevy_ecs::system::BoxedSystem> {
            if self.0.swap(true, Ordering::Relaxed) {
                anyhow::bail!("cannot construct second schedule");
            }
            crate::system::ModSystemBuild::build(&definition("game::a", 2), context)
        }
    }

    let mut world = world();
    let original = definition("game::a", 1)
        .in_schedules([ModScheduleName::Update, ModScheduleName::PostUpdate]);
    let (_, handle) = load(
        &mut world,
        ModAsset::new(
            "game",
            ModDiscovery {
                systems: vec![original],
                ..Default::default()
            },
        ),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    let replacement = ModSystem::custom("game::a", FailSecondBuild(AtomicBool::new(false)))
        .in_schedules([ModScheduleName::Update, ModScheduleName::PostUpdate]);
    reload(
        &mut world,
        &handle,
        ModAsset::new(
            "game",
            ModDiscovery {
                systems: vec![replacement],
                ..Default::default()
            },
        ),
    );

    assert_eq!(tick(&mut world), [1]);
    world.run_schedule(PostUpdate);
    assert_eq!(world.resource::<Runs>().0, [1]);
}

#[test]
fn accesses_reload_and_uninstall_independently() {
    use crate::sandbox::Sandbox;

    let mut world = world();
    let sandbox = Sandbox::new(&mut world, ModSchedules::default());
    let access = ModAccess::Sandbox(world.spawn(sandbox).id());
    let (entity, handle) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    world.get_mut::<Mod>(entity).unwrap().enable_access(access);
    assert_eq!(tick(&mut world), [1, 1]);
    world
        .get_mut::<Mod>(entity)
        .unwrap()
        .disable_access(&access);
    assert_eq!(tick(&mut world), [1]);
    world.get_mut::<Mod>(entity).unwrap().enable_access(access);
    assert_eq!(tick(&mut world), [1, 1]);

    reload(&mut world, &handle, asset("game", &[("game::a", 2)], false));
    assert_eq!(tick(&mut world), [2, 2]);
    world.entity_mut(entity).remove::<Mod>();
    assert!(tick(&mut world).is_empty());
}

#[test]
fn revision_assets_release_factories_after_replacement() {
    use bevy_ecs::system::RunSystemOnce;
    use std::sync::Arc;
    let mut app = bevy_app::App::new();
    app.add_plugins((
        bevy_app::TaskPoolPlugin::default(),
        bevy_asset::AssetPlugin::default(),
    ));
    let mut world = world();
    world.insert_resource(app.world().resource::<bevy_asset::AssetServer>().clone());
    let token = Arc::new(());
    let weak = Arc::downgrade(&token);
    let mut definition = ModSystem::new(move || {
        let _ = Arc::strong_count(&token);
    })
    .in_schedules([ModScheduleName::Update]);
    definition.name = "game::a".into();
    let (_, handle) = load(
        &mut world,
        ModAsset::new(
            "game",
            ModDiscovery {
                systems: vec![definition],
                ..Default::default()
            },
        ),
        ModReplacement::Namespace,
    );
    tick(&mut world);
    assert!(weak.upgrade().is_some());
    reload(&mut world, &handle, asset("game", &[("game::a", 2)], false));
    assert!(
        weak.upgrade().is_some(),
        "scheduled copies retain the previous executable"
    );
    assert_eq!(tick(&mut world), [2]);
    world
        .run_system_once(Assets::<ModSystemAsset>::track_assets)
        .unwrap();
    assert!(
        weak.upgrade().is_none(),
        "the old system asset and executable must be released"
    );
}

#[test]
fn native_assets_have_no_source_dependency() {
    let mut world = world();
    let native = ModAsset::new(
        "game",
        ModDiscovery {
            systems: vec![definition("game::a", 1)],
            ..Default::default()
        },
    );
    load(&mut world, native, ModReplacement::Namespace);
    assert_eq!(tick(&mut world), [1]);
    assert!(
        world
            .resource::<Assets<ModSystemAsset>>()
            .iter()
            .all(|(_, system)| system.source.is_none())
    );
}

#[test]
fn replacement_deletion_and_no_fallback() {
    let mut world = world();
    load(
        &mut world,
        asset("game", &[("game::move", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    let (_, replacement) = load(
        &mut world,
        asset("patch", &[("game::move", 2), ("patch::post", 3)], false),
        ModReplacement::Only(HashSet::from(["game::move".into(), "patch::post".into()])),
    );
    assert_eq!(tick(&mut world), [2, 3]);
    assert_eq!(tick(&mut world), [2, 3]);
    reload(&mut world, &replacement, asset("patch", &[], false));
    assert!(tick(&mut world).is_empty());
    assert!(tick(&mut world).is_empty());
}

#[test]
fn partial_discovery_keeps_missing_systems() {
    let mut world = world();
    let (_, handle) = load(
        &mut world,
        asset("game", &[("game::a", 1), ("b", 2)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1, 2]);
    reload(&mut world, &handle, asset("game", &[("game::a", 3)], true));
    assert_eq!(tick(&mut world), [2, 3]);
    reload(&mut world, &handle, asset("game", &[("game::a", 4)], false));
    assert_eq!(tick(&mut world), [4]);
}

#[test]
fn namespace_collision_is_rejected_even_with_unrestricted_replacements() {
    let mut world = world();
    load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    load(
        &mut world,
        asset("game", &[("game::a", 2)], false),
        ModReplacement::Any,
    );
    assert_eq!(tick(&mut world), [1]);
}

#[test]
fn foreign_replacements_require_permission() {
    for (policy, expected) in [
        (ModReplacement::None, 1),
        (ModReplacement::Namespace, 1),
        (ModReplacement::Only(HashSet::new()), 1),
        (ModReplacement::Only(HashSet::from(["game::a".into()])), 2),
        (ModReplacement::default(), 2),
    ] {
        let mut world = world();
        load(
            &mut world,
            asset("game", &[("game::a", 1)], false),
            ModReplacement::Namespace,
        );
        assert_eq!(tick(&mut world), [1]);
        load(&mut world, asset("patch", &[("game::a", 2)], false), policy);
        assert_eq!(tick(&mut world), [expected]);
    }
}

#[test]
fn denied_replacements_keep_existing_systems_but_allow_new_names() {
    for policy in [ModReplacement::None, ModReplacement::Only(HashSet::new())] {
        let mut world = world();
        let (_, handle) = load(&mut world, asset("game", &[("a", 1)], false), policy);
        assert_eq!(tick(&mut world), [1]);
        reload(
            &mut world,
            &handle,
            asset("game", &[("a", 2), ("b", 3)], false),
        );
        assert_eq!(tick(&mut world), [1, 3]);
        reload(&mut world, &handle, asset("game", &[], false));
        assert_eq!(tick(&mut world), [1, 3]);
    }
}

#[test]
fn revisions_use_their_own_policy_without_revocation() {
    let mut world = world();
    load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    let (_, handle) = load(
        &mut world,
        asset("patch", &[("game::a", 2)], false),
        ModReplacement::Any,
    );
    assert_eq!(tick(&mut world), [2]);

    for (value, policy, expected) in [(3, ModReplacement::None, 2), (4, ModReplacement::Any, 4)] {
        world
            .resource_mut::<Assets<ModAsset>>()
            .insert(
                handle.id(),
                asset("patch", &[("game::a", value)], false).with_settings(policy),
            )
            .unwrap();
        assert_eq!(tick(&mut world), [expected]);
    }
}

#[test]
fn bare_names_in_different_namespaces_and_qualified_names_are_distinct() {
    let mut world = world();
    let (_, game) = load(
        &mut world,
        asset("game", &[("tick", 1), ("game::tick", 2)], false),
        ModReplacement::Namespace,
    );
    load(
        &mut world,
        asset("other", &[("tick", 3)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1, 2, 3]);
    reload(&mut world, &game, asset("game", &[("tick", 4)], false));
    assert_eq!(tick(&mut world), [3, 4]);
}

#[test]
fn shared_asset_reloads_every_mod_and_access_removal_is_local() {
    let mut world = world();
    let (first, handle) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    let mut module = Mod::new(handle.clone());
    module.enable_access(ModAccess::World);
    world.spawn(module);
    assert_eq!(tick(&mut world), [1, 1]);
    reload(&mut world, &handle, asset("game", &[("game::a", 2)], false));
    assert_eq!(tick(&mut world), [2, 2]);
    world
        .get_mut::<Mod>(first)
        .unwrap()
        .disable_access(&ModAccess::World);
    assert_eq!(tick(&mut world), [2]);
    world
        .get_mut::<Mod>(first)
        .unwrap()
        .enable_access(ModAccess::World);
    assert_eq!(tick(&mut world), [2, 2]);
}

#[test]
fn superseded_source_cannot_delete_the_winner() {
    let mut world = world();
    let (_, original) = load(
        &mut world,
        asset("game", &[("game::a", 1)], false),
        ModReplacement::Namespace,
    );
    assert_eq!(tick(&mut world), [1]);
    load(
        &mut world,
        asset("patch", &[("game::a", 2)], false),
        ModReplacement::Any,
    );
    assert_eq!(tick(&mut world), [2]);
    reload(&mut world, &original, asset("game", &[], false));
    assert_eq!(tick(&mut world), [2]);
}
