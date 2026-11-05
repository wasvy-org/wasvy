use bevy_asset::{AssetEvent, AssetId, AssetServer, Assets, Handle};
use bevy_ecs::{
    prelude::*,
    schedule::InternedScheduleLabel,
    system::{SystemParam, SystemState},
};
use bevy_log::warn;
use bevy_platform::collections::{HashMap, HashSet};
use error_collection::Errors;

use crate::{
    access::ModAccess,
    asset::{ModAsset, ModReplacement},
    cleanup::{DespawnModEntities, remove_systems_in_sets},
    mods::{Mod, ModDespawnBehaviour},
    schedule::{ModSchedules, ModStartup},
    system::{
        CopySet, ModSystem, ModSystemAsset, ModSystemName, ModSystemNamespace, PreparedSystem,
        prepare_systems,
    },
};

pub(crate) fn run_setup(
    mut publications: Local<Publications>,
    world: &mut World,
    params: &mut SystemState<SetupParams>,
) {
    let mut params = params.get_mut(world).expect("valid setup parameters");
    publications.track_asset_loads(params.events.read());
    let active = publications.remove_unused_sources(&params.mods);
    publications.publish_assets(&mut params, &active);
    let mods: Vec<_> = params.mods.iter().map(|(entity, ..)| entity).collect();
    let orphaned: Vec<_> = params.orphaned.iter().collect();
    remove_orphaned_installations(world, orphaned);

    let mut additions = Vec::new();
    for entity in mods {
        update_installation(world, entity, &publications, &mut additions);
    }

    let startup = additions
        .iter()
        .any(|copy| copy.schedule == bevy_ecs::schedule::ScheduleLabel::intern(&ModStartup));
    for copy in additions {
        world
            .resource_mut::<Schedules>()
            .add_systems(copy.schedule, copy.config);
    }
    if startup {
        ModStartup::run(world);
    }
}

#[derive(SystemParam)]
pub(crate) struct SetupParams<'w, 's> {
    events: MessageReader<'w, 's, AssetEvent<ModAsset>>,
    assets: ResMut<'w, Assets<ModAsset>>,
    systems: ResMut<'w, Assets<ModSystemAsset>>,
    server: Option<Res<'w, AssetServer>>,
    mods: Query<'w, 's, (Entity, &'static Mod, Option<&'static Installed>)>,
    orphaned: Query<'w, 's, Entity, (With<Installed>, Without<Mod>)>,
}

#[derive(Default)]
pub(crate) struct Publications {
    assets: HashMap<AssetId<ModAsset>, Revision>,
    sequence: Revision,
    slots: HashMap<ModSystemName, SystemSlot>,
    sources: HashMap<AssetId<ModAsset>, Source>,
}

impl Publications {
    fn track_asset_loads<'a>(&mut self, events: impl Iterator<Item = &'a AssetEvent<ModAsset>>) {
        for event in events {
            match *event {
                AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                    self.assets.insert(id, self.sequence.next());
                }
                AssetEvent::Removed { id } => {
                    self.assets.remove(&id);
                }
                _ => {}
            }
        }
    }

    fn remove_unused_sources(
        &mut self,
        mods: &Query<(Entity, &Mod, Option<&Installed>)>,
    ) -> HashSet<AssetId<ModAsset>> {
        let mut active: HashSet<_> = mods
            .iter()
            .flat_map(|(_, module, installed)| {
                std::iter::once(module.asset().id())
                    .chain(installed.and_then(|installed| installed.source))
            })
            .collect();
        self.slots.retain(|_, slot| active.contains(&slot.home));
        active.extend(self.slots.values().map(|slot| slot.owner));
        self.sources.retain(|id, _| active.contains(id));
        active
    }

    fn publish_assets(&mut self, params: &mut SetupParams, active: &HashSet<AssetId<ModAsset>>) {
        let mut candidates: Vec<_> = params
            .assets
            .ids()
            .filter_map(|id| {
                let revision = *self.assets.get(&id)?;
                if !active.contains(&id)
                    || self
                        .sources
                        .get(&id)
                        .is_some_and(|source| source.revision == revision)
                {
                    return None;
                }
                Some((revision, id))
            })
            .collect();
        candidates.sort_unstable_by_key(|(revision, _)| *revision);
        for (revision, id) in candidates {
            self.publish_asset(params, id, revision);
        }
    }

    fn publish_asset(
        &mut self,
        params: &mut SetupParams,
        id: AssetId<ModAsset>,
        revision: Revision,
    ) {
        let asset = params.assets.get(id).unwrap();
        let namespace = asset.namespace().clone();
        if self
            .sources
            .iter()
            .any(|(other, source)| *other != id && source.namespace == namespace)
        {
            warn!("Mod namespace {namespace:?} is already owned by another asset; refusing {id:?}");
            return;
        }
        let source = Source {
            revision,
            namespace,
            policy: asset.settings().replacements.clone(),
        };
        let source_handle =
            retained_source_handle(&mut params.assets, params.server.as_deref(), id);
        let asset = params.assets.get(id).unwrap();
        for definition in asset.systems() {
            self.publish_system(
                &mut params.systems,
                id,
                &source,
                source_handle.as_ref(),
                definition.clone(),
            );
        }
        if asset.errors().is_empty() {
            for (name, slot) in &mut self.slots {
                if slot.owner == id && slot.publication < revision && source.may_replace(name) {
                    slot.system = None;
                }
            }
        }
        self.sources.insert(id, source);
    }

    fn publish_system(
        &mut self,
        systems: &mut Assets<ModSystemAsset>,
        id: AssetId<ModAsset>,
        source: &Source,
        source_handle: Option<&Handle<ModAsset>>,
        definition: ModSystem,
    ) {
        let existing = self.slots.get(&definition.name);
        if let Some(slot) = existing {
            if slot.publication > source.revision {
                return;
            }
            if !source.may_replace(&definition.name) {
                warn!(
                    "Mod {} may not replace {}",
                    source.namespace, definition.name
                );
                return;
            }
        }
        let name = definition.name.clone();
        let system = systems.add(ModSystemAsset {
            definition,
            source: source_handle.cloned(),
        });
        self.slots.insert(
            name,
            SystemSlot {
                home: existing.map_or(id, |slot| slot.home),
                owner: id,
                publication: source.revision,
                system: Some(system),
            },
        );
    }
}

/// Order in which asset loads were observed; later revisions win contested system slots.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Revision(u64);

impl Revision {
    fn next(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}

struct Source {
    revision: Revision,
    namespace: ModSystemNamespace,
    policy: ModReplacement,
}

impl Source {
    fn may_replace(&self, name: &ModSystemName) -> bool {
        match &self.policy {
            ModReplacement::Any => true,
            ModReplacement::None => false,
            ModReplacement::Namespace => &self.namespace == name.namespace(),
            ModReplacement::Only(allowed) => allowed.contains(name),
        }
    }
}

struct SystemSlot {
    home: AssetId<ModAsset>,
    owner: AssetId<ModAsset>,
    publication: Revision,
    system: Option<Handle<ModSystemAsset>>,
}

fn retained_source_handle(
    assets: &mut Assets<ModAsset>,
    server: Option<&AssetServer>,
    id: AssetId<ModAsset>,
) -> Option<Handle<ModAsset>> {
    if !assets.get(id)?.retain_source {
        return None;
    }
    // AssetServer's handle also retains the path metadata needed for file watching.
    server
        .and_then(|server| server.get_id_handle(id))
        .or_else(|| assets.get_strong_handle(id))
}

#[derive(Component, Default)]
struct Installed {
    source: Option<AssetId<ModAsset>>,
    copies: HashMap<ModAccess, HashMap<ModSystemName, InstalledCopy>>,
}

struct InstalledCopy {
    asset: Handle<ModSystemAsset>,
    schedules: Vec<InternedScheduleLabel>,
}

struct DesiredSystems<'a> {
    source: AssetId<ModAsset>,
    slots: &'a HashMap<ModSystemName, SystemSlot>,
}

impl DesiredSystems<'_> {
    fn iter(&self) -> impl Iterator<Item = (&ModSystemName, &Handle<ModSystemAsset>)> {
        self.slots
            .iter()
            .filter(|(_, slot)| slot.home == self.source)
            .filter_map(|(name, slot)| slot.system.as_ref().map(|system| (name, system)))
    }

    fn get(&self, name: &ModSystemName) -> Option<&Handle<ModSystemAsset>> {
        self.slots
            .get(name)
            .filter(|slot| slot.home == self.source)?
            .system
            .as_ref()
    }
}

fn remove_orphaned_installations(world: &mut World, orphaned: impl IntoIterator<Item = Entity>) {
    for entity in orphaned {
        let installed = world.entity_mut(entity).take::<Installed>().unwrap();
        for (access, copies) in installed.copies {
            for (name, mut copy) in copies {
                remove_copy(world, entity, access, name, &mut copy);
            }
        }
    }
}

fn update_installation(
    world: &mut World,
    entity: Entity,
    publications: &Publications,
    additions: &mut Vec<PreparedSystem>,
) {
    let requested = world.get::<Mod>(entity).unwrap().asset().id();
    let mut installed = world
        .entity_mut(entity)
        .take::<Installed>()
        .unwrap_or_default();
    let Some(source_id) = publications
        .sources
        .contains_key(&requested)
        .then_some(requested)
        .or(installed.source)
    else {
        return;
    };
    let desired = DesiredSystems {
        source: source_id,
        slots: &publications.slots,
    };
    let mut replaced = remove_stale_copies(world, entity, &desired, &mut installed);
    let accesses: Vec<_> = world
        .get::<Mod>(entity)
        .unwrap()
        .accesses()
        .copied()
        .collect();
    for access in accesses {
        if access.validate(world).is_ok() {
            replaced |= rebuild_access(
                world,
                entity,
                access,
                &desired,
                installed.copies.entry(access).or_default(),
                additions,
            );
        }
    }
    if replaced && ModDespawnBehaviour::should_despawn_entities(world) {
        despawn_owned_entities(world, entity);
    }
    installed.source = Some(source_id);
    world.entity_mut(entity).insert(installed);
}

fn remove_stale_copies(
    world: &mut World,
    entity: Entity,
    desired: &DesiredSystems<'_>,
    installed: &mut Installed,
) -> bool {
    let mut replaced = false;
    installed.copies.retain(|access, copies| {
        let enabled = world
            .get::<Mod>(entity)
            .unwrap()
            .accesses()
            .any(|candidate| candidate == access);
        let valid = enabled && access.validate(world).is_ok();
        copies.retain(|name, copy| {
            let desired = desired.get(name).is_some();
            if valid && desired {
                return true;
            }
            if !remove_copy(world, entity, *access, name.clone(), copy) {
                return true;
            }
            replaced |= enabled && !desired;
            false
        });
        !copies.is_empty()
    });
    replaced
}

fn rebuild_access(
    world: &mut World,
    entity: Entity,
    access: ModAccess,
    desired: &DesiredSystems<'_>,
    copies: &mut HashMap<ModSystemName, InstalledCopy>,
    additions: &mut Vec<PreparedSystem>,
) -> bool {
    let rebuild: HashSet<_> = desired
        .iter()
        .filter(|(name, asset)| copies.get(*name).is_none_or(|copy| copy.asset != **asset))
        .map(|(name, _)| name.clone())
        .collect();
    if rebuild.is_empty() {
        return false;
    }
    let definitions: Vec<_> = desired
        .iter()
        .filter_map(|(_, asset)| {
            world
                .resource::<Assets<ModSystemAsset>>()
                .get(asset)
                .map(|system| system.definition.clone())
        })
        .collect();
    let label = world.get::<Name>(entity).cloned();
    let label = label.as_ref().map_or("unknown", Name::as_str);
    let mut errors = Errors::new();
    let prepared = prepare_systems(
        &rebuild,
        &mut errors,
        &definitions,
        world,
        entity,
        label,
        &access,
    );
    if !errors.is_empty() {
        warn!("Failed to prepare {label}: {errors:?}");
    }
    let schedules = access.schedules(world);
    let mut ready = rebuild;
    ready.retain(|name| {
        definitions.iter().any(|definition| {
            definition.name == *name && fully_prepared(definition, &schedules, &prepared)
        })
    });
    let replaced = replace_ready_copies(world, entity, access, desired, copies, &mut ready);
    for copy in prepared
        .into_iter()
        .filter(|copy| ready.contains(&copy.name))
    {
        copies
            .get_mut(&copy.name)
            .unwrap()
            .schedules
            .push(copy.schedule);
        additions.push(copy);
    }
    replaced
}

fn fully_prepared(
    definition: &ModSystem,
    schedules: &ModSchedules,
    prepared: &[PreparedSystem],
) -> bool {
    schedules
        .iter()
        .filter(|schedule| {
            definition
                .schedules
                .iter()
                .any(|request| request.matches(schedule))
        })
        .all(|schedule| {
            prepared.iter().any(|copy| {
                copy.name == definition.name && copy.schedule == schedule.schedule_label()
            })
        })
}

fn replace_ready_copies(
    world: &mut World,
    entity: Entity,
    access: ModAccess,
    desired: &DesiredSystems<'_>,
    copies: &mut HashMap<ModSystemName, InstalledCopy>,
    ready: &mut HashSet<ModSystemName>,
) -> bool {
    let mut replaced = false;
    ready.retain(|name| {
        if let Some(copy) = copies.get_mut(name) {
            if !remove_copy(world, entity, access, name.clone(), copy) {
                return false;
            }
            copy.asset = desired.get(name).unwrap().clone();
            replaced = true;
        } else {
            copies.insert(
                name.clone(),
                InstalledCopy {
                    asset: desired.get(name).unwrap().clone(),
                    schedules: Vec::new(),
                },
            );
        }
        true
    });
    replaced
}

fn despawn_owned_entities(world: &mut World, entity: Entity) {
    let entities: Vec<_> = world
        .get::<DespawnModEntities>(entity)
        .map(|owned| owned.iter().collect())
        .unwrap_or_default();
    for owned in entities {
        world.despawn(owned);
    }
}

fn remove_copy(
    world: &mut World,
    mod_id: Entity,
    access: ModAccess,
    name: ModSystemName,
    copy: &mut InstalledCopy,
) -> bool {
    let set = CopySet {
        mod_id,
        access,
        name,
    };
    copy.schedules.retain(|schedule| {
        *schedule != bevy_ecs::schedule::ScheduleLabel::intern(&ModStartup)
            && !remove_systems_in_sets(world, *schedule, [&set])
    });
    copy.schedules.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        asset::ModDiscovery,
        sandbox::Sandboxed,
        schedule::{ModScheduleName, ModSchedules},
        system::ModSystem,
    };
    use bevy_app::Update;

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

    fn load(
        world: &mut World,
        asset: ModAsset,
        policy: ModReplacement,
    ) -> (Entity, Handle<ModAsset>) {
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
            .filter(|(id, asset)| {
                world.resource::<TestRevisions>().0.get(id) != Some(&asset.revision)
            })
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

        for (value, policy, expected) in [(3, ModReplacement::None, 2), (4, ModReplacement::Any, 4)]
        {
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
}
