use bevy_ecs::{
    entity::EntityHashSet,
    prelude::*,
    schedule::{ScheduleCleanupPolicy, ScheduleError},
    system::SystemState,
};
use bevy_log::prelude::*;
use bevy_platform::collections::{HashMap, HashSet};

use crate::{
    mods::{ModDespawnBehaviour, ModSystemSet},
    prelude::ModSchedules,
};

/// A [Message] that triggers disabling of scheduled [ModSystemSets](ModSystemSet).
///
/// For ease of use within component hooks, this is also a command that can be enqueued like any other.
///
/// This is a message because it must run during the setup_schedule. This way
/// it can cleanup all schedules, including the one from which the message was written.
#[derive(Message)]
pub(crate) struct DisableSystemSet {
    pub(crate) set: ModSystemSet,
    pub(crate) schedules: ModSchedules,
}

impl Command<()> for DisableSystemSet {
    fn apply(self, world: &mut World) {
        if !self.schedules.0.is_empty() {
            world.write_message(self);
        }
    }
}

pub(crate) fn disable_mod_system_sets(
    world: &mut World,
    param: &mut SystemState<MessageReader<DisableSystemSet>>,
) {
    let mut messages = param.get_mut(world);

    // Collect a map of unique bevy schedule labels and the sets that need to be removed from them
    let mut remove = HashMap::new();
    for DisableSystemSet { set, schedules } in messages.read() {
        for schedule in schedules.0.iter() {
            remove
                .entry(schedule.schedule_label())
                .or_insert(HashSet::new())
                .insert(set.clone());
        }
    }

    // Remove sets from each schedule
    for (label, sets) in remove {
        let mut schedules = world
            .get_resource_mut::<Schedules>()
            .expect("Running in a bevy App");

        // We must remove and then re-add the schedule so we can call remove_systems_in_set with exclusive world access
        let Some(mut schedule) = schedules.remove(label) else {
            continue;
        };

        for set in sets {
            if let Err(error) = schedule.remove_systems_in_set(
                set.clone(),
                world,
                ScheduleCleanupPolicy::RemoveSetAndSystems,
            ) && !matches!(error, ScheduleError::SetNotFound)
            {
                warn!(
                    "Unable to remove system set {set:?}. Systems from unloaded mods might still be running!\nError: {error}."
                );
            }
        }

        world
            .get_resource_mut::<Schedules>()
            .expect("Running in a bevy App")
            .insert(schedule);
    }
}

/// A component that tracks all of the entities spawned by a mod (and considered belonging to it).
///
/// When a mod is despawned, so will all the entities it spawned due to the `linked_spawn` clause of the relationship.
#[derive(Component, Default)]
#[relationship_target(relationship = DespawnModEntity, linked_spawn)]
pub(crate) struct DespawnModEntities(EntityHashSet);

/// A component that tracks the mod responsible for spawning an entity.
#[derive(Component)]
#[relationship(relationship_target = DespawnModEntities)]
pub(crate) struct DespawnModEntity(pub(crate) Entity);

/// Determines whether [DespawnModEntity] should be inserted to entities spawned by mods
#[derive(Clone, Copy)]
pub(crate) struct InsertDespawnComponent(pub(crate) Option<Entity>);

impl InsertDespawnComponent {
    pub(crate) fn new(mod_id: Entity, world: &World) -> Self {
        Self(if ModDespawnBehaviour::should_despawn_entities(world) {
            Some(mod_id)
        } else {
            None
        })
    }
}
