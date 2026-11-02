use bevy_ecs::{
    entity::EntityHashSet,
    prelude::*,
    schedule::{InternedScheduleLabel, ScheduleCleanupPolicy, ScheduleError},
    system::SystemState,
};
use bevy_log::prelude::*;
use bevy_platform::collections::{HashMap, HashSet};

use crate::{mods::ModSystemSet, prelude::ModSchedules};

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

impl Command for DisableSystemSet {
    type Out = ();

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
    let mut messages = param.get_mut(world).expect("valid system parameter");

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
        remove_systems_in_sets(world, label, &sets);
    }
}

/// Removes the systems in each of the `sets` from the schedule with the given label.
///
/// The schedule must not be running: it is taken out of [Schedules] while this runs.
pub(crate) fn remove_systems_in_sets<'a>(
    world: &mut World,
    label: InternedScheduleLabel,
    sets: impl IntoIterator<Item = &'a ModSystemSet>,
) {
    // Remove sets once https://github.com/bevyengine/bevy/pull/26290 lands
    let policy = ScheduleCleanupPolicy::RemoveSystemsOnly;

    world.schedule_scope(label, |world, schedule|{
        for set in sets {
            // Checking first avoids initializing a schedule the set was never added to
            if schedule.graph().system_sets.contains(set.clone())
                && let Err(error) = schedule.remove_systems_in_set(set.clone(), world, policy)
                && !matches!(error, ScheduleError::SetNotFound)
            {
                warn!(
                    "Unable to remove system set {set:?}. Systems from unloaded or reloaded mods might still be running!\nError: {error}."
                );
            }
        }
    });
}

/// A component that tracks all of the entities spawned by a mod (and considered belonging to it).
///
/// When a mod is despawned, so will all the entities it spawned due to the `linked_spawn` clause of the relationship.
#[derive(Component, Default)]
#[relationship_target(relationship = DespawnModEntity, linked_spawn)]
pub struct DespawnModEntities(EntityHashSet);

/// A component that tracks the mod responsible for spawning an entity.
#[derive(Component)]
#[relationship(relationship_target = DespawnModEntities)]
pub struct DespawnModEntity(pub(crate) Entity);

impl DespawnModEntity {
    pub fn new(entity: Entity) -> Self {
        Self(entity)
    }
}
