use std::hash::Hash;

use anyhow::Result;
use bevy_ecs::{prelude::*, query::FilteredAccess, system::BoxedSystem};
use bevy_log::warn;
use bevy_platform::collections::HashMap;
use error_collection::Errors;

use crate::{
    access::ModAccess,
    commands::{RuntimeCommandsConfig, with_runtime_commands_config},
    mods::ModSystemSet,
};

mod definition;
pub use definition::ModSystemDefinition;

mod name;
pub use name::ModSystemName;

mod param;
pub use param::ModSystemParam;

/// Declares a constructor for a fresh boxed system.
///
/// # Safety
///
/// Implementers must uphold the invariants listed in the docs for [Self::build].
pub unsafe trait ModSystemBuild: Send + Sync + 'static {
    /// Return a boxed scheduled system. Implementers must:
    /// - Build the system from the provided [ModSystemContext::world]
    /// - Have all component access honor [ModSystemContext::access], see [ModSystemParam]
    /// - Use [`RuntimeCommands`](crate::commands::RuntimeCommands) instead of native Bevy [`Commands`]
    ///
    /// The runtime will handle initializing the returned system before adding it to a schedule.
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem>;
}

/// System construction context. See [ModSystemBuild].
pub struct ModSystemContext<'a> {
    pub world: &'a mut World,
    pub mod_name: &'a str,
    pub access: FilteredAccess,
}

impl<'a> ModSystemContext<'a> {
    fn new(world: &'a mut World, mod_name: &'a str, access: &ModAccess) -> Self {
        let access = access.filtered_access(world);
        Self {
            world,
            mod_name,
            access,
        }
    }
}

pub(crate) fn add_systems(
    errors: &mut Errors,
    definitions: &[ModSystemDefinition],
    world: &mut World,
    mod_id: Entity,
    mod_name: &str,
    access: &ModAccess,
) {
    let schedules = access.schedules(world);
    let mut placements = Vec::new();
    let mut ids = HashMap::new();
    for definition in definitions {
        for request in &definition.schedules {
            let Some(schedule) = schedules.iter().find(|schedule| request.matches(schedule)) else {
                warn!(
                    "Mod tried adding systems to schedule {request:?}, but that schedule is not enabled. See ModSchedules docs."
                );
                continue;
            };
            let schedule = schedule.schedule_label();
            let key = (&definition.name, schedule);
            if ids.contains_key(&key) {
                continue;
            }
            let id = DynamicSystemId::new(world);
            ids.insert(key, id);
            placements.push((definition, schedule, id));
        }
    }
    for (definition, schedule, id) in placements {
        let commands = RuntimeCommandsConfig::new(world, mod_id, *access);
        let system = with_runtime_commands_config(world, commands, |world| {
            definition
                .build(ModSystemContext::new(world, mod_name, access))
                .map(|mut system| {
                    system.initialize(world);
                    system
                })
        });
        let system = system.map_err(|error| {
            error.context(format!(
                "Failed to build system {} for {}",
                definition.name,
                access.display(world)
            ))
        });
        let Some(system) = errors.collect(system) else {
            continue;
        };
        let mut config = system
            .in_set(id)
            .in_set(ModSystemSet::All)
            .in_set(ModSystemSet::Mod(mod_id))
            .in_set(ModSystemSet::Access(*access));
        for name in &definition.before {
            for ((candidate, candidate_schedule), other) in &ids {
                if *candidate_schedule == schedule && name.conflicts_with(candidate) {
                    config = config.before(*other);
                }
            }
        }
        for name in &definition.after {
            for ((candidate, candidate_schedule), other) in &ids {
                if *candidate_schedule == schedule && name.conflicts_with(candidate) {
                    config = config.after(*other);
                }
            }
        }
        world
            .resource_mut::<Schedules>()
            .add_systems(schedule, config);
    }
}

/// Bevy doesn't return an identifier for systems added directly to the scheduler. There is
/// [NodeId](bevy_ecs::schedule::NodeId) but that has no clear way of being used for system ordering.
///
/// So instead we take inspiration from bevy's [AnonymousSet](bevy_ecs::schedule::AnonymousSet)
/// and we identify each system with an extra [SystemSet] all to itself.
// Note: Using an AnonymousSet could work but unfortunately the method used to create one is private.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct DynamicSystemId(usize);

impl DynamicSystemId {
    fn new(world: &mut World) -> Self {
        world.init_resource::<DynamicSystemSetCount>();
        let mut count = world.resource_mut::<DynamicSystemSetCount>();
        let id = Self(count.0);
        count.0 += 1;
        id
    }
}

impl SystemSet for DynamicSystemId {
    // As of bevy 0.18 this function's only purpose is for debugging
    fn is_anonymous(&self) -> bool {
        // This is technically incorrect, but it makes bevy use the system name as node name instead of DynamicSystemId(usize)
        true
    }

    fn dyn_clone(&self) -> Box<dyn SystemSet> {
        Box::new(*self)
    }
}

#[derive(Default, Resource)]
struct DynamicSystemSetCount(usize);
