use std::{fmt, marker::PhantomData, sync::Arc};

use anyhow::Result;
use bevy_ecs::{
    prelude::*,
    query::FilteredAccess,
    system::{BoxedSystem, IntoResult},
};
use bevy_log::warn;
use bevy_platform::collections::{HashMap, HashSet};
use error_collection::Errors;

use crate::{
    access::ModAccess,
    commands::{RuntimeCommandsConfig, with_runtime_commands_config},
    mods::ModSystemSet,
    schedule::ModScheduleName,
};

mod asset;
pub use asset::ModSystemAsset;

mod name;
pub use name::{ModSystemName, ModSystemNamespace};

mod param;
use param::ModParamBuilder;
pub use param::ModSystemParam;

/// A loader produces re-initializable system definitions that can be applied to any world as needed by the runtime.
///
/// For example:
/// - [Self::new] converts a native Bevy system into a definition
/// - `wasvy_wasm` resolves the systems inside a `.wasm` asset as [ModSystem]s
///
/// The runtime can add or remove copies of this definition when the mod is hot-reloaded.
#[derive(Clone)]
pub struct ModSystem {
    pub name: ModSystemName,
    pub schedules: std::collections::HashSet<ModScheduleName>,
    pub before: std::collections::HashSet<ModSystemName>,
    pub after: std::collections::HashSet<ModSystemName>,
    builder: Arc<dyn ModSystemBuild>,
}

impl ModSystem {
    /// Convert a Bevy system function into a definition. This has no ordering constraints or schedule.
    ///
    /// Every copy of the system is built for the access it runs in: its queries only match entities in that access and
    /// its [RuntimeCommands](crate::commands::RuntimeCommands) spawn into it. See [ModSystemParam] for the parameters a system can use.
    ///
    /// Only functions and closures without system input are supported, not piped, exclusive or already built systems.
    pub fn new<M, F>(system: F) -> Self
    where
        M: 'static,
        F: SystemParamFunction<M, In = (), Out: IntoResult<()>> + Clone,
        F::Param: ModSystemParam,
    {
        Self::custom(
            ModSystemName::from_system(system.clone()),
            NativeSystem {
                system,
                marker: PhantomData,
            },
        )
    }

    /// Build a custom definition. Creates an unscheduled definition with no ordering constraints.
    pub fn custom(name: impl Into<ModSystemName>, builder: impl ModSystemBuild) -> Self {
        Self {
            name: name.into(),
            schedules: std::collections::HashSet::new(),
            before: std::collections::HashSet::new(),
            after: std::collections::HashSet::new(),
            builder: Arc::new(builder),
        }
    }

    pub fn in_schedules(mut self, schedules: impl IntoIterator<Item = ModScheduleName>) -> Self {
        self.schedules.extend(schedules);
        self
    }
}

impl fmt::Debug for ModSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModSystem")
            .field("name", &self.name)
            .field("schedules", &self.schedules)
            .field("before", &self.before)
            .field("after", &self.after)
            .finish_non_exhaustive()
    }
}

// Safety: passes on invariants
unsafe impl ModSystemBuild for ModSystem {
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
        self.builder.build(context)
    }
}

/// Builds fresh copies of a native Bevy system, see [ModSystem::new]
struct NativeSystem<M, F> {
    system: F,
    marker: PhantomData<fn() -> M>,
}

// Safety:
// - The system is built from the context's world
// - Every parameter is built for the context's access, see ModSystemParam
// - Bevy's Commands do not implement ModSystemParam, so commands can only be queued with RuntimeCommands
unsafe impl<M, F> ModSystemBuild for NativeSystem<M, F>
where
    M: 'static,
    F: SystemParamFunction<M, In = (), Out: IntoResult<()>> + Clone,
    F::Param: ModSystemParam,
{
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
        let system = ModParamBuilder(&context.access)
            .build_state(context.world)
            .build_any_system(self.system.clone())
            // Built systems are named after their parameters otherwise
            .with_name(std::any::type_name::<F>());
        Ok(Box::new(system))
    }
}

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

pub(crate) fn prepare_systems(
    rebuild: &HashSet<ModSystemName>,
    errors: &mut Errors,
    definitions: &[ModSystem],
    world: &mut World,
    mod_id: Entity,
    mod_name: &str,
    access: &ModAccess,
) -> Vec<PreparedSystem> {
    let schedules = access.schedules(world);
    let mut prepared = Vec::new();
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
            let id = CopySet {
                mod_id,
                access: *access,
                name: definition.name.clone(),
            };
            ids.insert(key, id.clone());
            placements.push((definition, schedule, id));
        }
    }
    for (definition, schedule, id) in placements {
        if !rebuild.contains(&definition.name) {
            continue;
        }
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
            .in_set(id.clone())
            .in_set(ModSystemSet::All)
            .in_set(ModSystemSet::Mod(mod_id))
            .in_set(ModSystemSet::Access(*access));
        for name in &definition.before {
            for ((candidate, candidate_schedule), other) in &ids {
                if *candidate_schedule == schedule && *candidate == name {
                    config = config.before(other.clone());
                }
            }
        }
        for name in &definition.after {
            for ((candidate, candidate_schedule), other) in &ids {
                if *candidate_schedule == schedule && *candidate == name {
                    config = config.after(other.clone());
                }
            }
        }
        prepared.push(PreparedSystem {
            name: definition.name.clone(),
            schedule,
            config,
        });
    }
    prepared
}

pub(crate) struct PreparedSystem {
    pub name: ModSystemName,
    pub schedule: bevy_ecs::schedule::InternedScheduleLabel,
    pub config: bevy_ecs::schedule::ScheduleConfigs<bevy_ecs::system::ScheduleSystem>,
}

#[cfg(test)]
pub(crate) fn add_systems(
    errors: &mut Errors,
    definitions: &[ModSystem],
    world: &mut World,
    mod_id: Entity,
    mod_name: &str,
    access: &ModAccess,
) {
    let rebuild = definitions
        .iter()
        .map(|definition| definition.name.clone())
        .collect();
    for prepared in prepare_systems(
        &rebuild,
        errors,
        definitions,
        world,
        mod_id,
        mod_name,
        access,
    ) {
        world
            .resource_mut::<Schedules>()
            .add_systems(prepared.schedule, prepared.config);
    }
}

#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct CopySet {
    pub mod_id: Entity,
    pub access: ModAccess,
    pub name: ModSystemName,
}
