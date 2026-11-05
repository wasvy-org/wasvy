use bevy_asset::{AssetId, Assets, Handle};
use bevy_ecs::{prelude::*, schedule::InternedScheduleLabel};
use bevy_log::warn;
use bevy_platform::collections::{HashMap, HashSet};
use error_collection::Errors;

use super::publish::{Publications, SystemSlot};
use crate::{
    access::ModAccess,
    asset::ModAsset,
    cleanup::{DespawnModEntities, remove_systems_in_sets},
    mods::{Mod, ModDespawnBehaviour},
    schedule::{ModSchedules, ModStartup},
    system::{CopySet, ModSystem, ModSystemAsset, ModSystemName, PreparedSystem, prepare_systems},
};

#[derive(Component, Default)]
pub(super) struct Installed {
    pub(super) source: Option<AssetId<ModAsset>>,
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

pub(super) fn remove_orphaned_installations(
    world: &mut World,
    orphaned: impl IntoIterator<Item = Entity>,
) {
    for entity in orphaned {
        let installed = world.entity_mut(entity).take::<Installed>().unwrap();
        for (access, copies) in installed.copies {
            for (name, mut copy) in copies {
                remove_copy(world, entity, access, name, &mut copy);
            }
        }
    }
}

pub(super) fn update_installation(
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
