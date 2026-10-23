use std::{
    borrow::Cow,
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use bevy_ecs::{prelude::*, query::FilteredAccess, system::BoxedSystem};
use bevy_log::warn;
use bevy_platform::collections::HashMap;
use error_collection::Errors;
use std::collections::HashSet;

use crate::{
    access::ModAccess,
    asset::ModDiscovery,
    commands::{RuntimeCommandsConfig, with_runtime_commands_config},
    mods::ModSystemSet,
    schedule::ModScheduleName,
};

/// A system path. Short qualified names can match module-qualified paths
/// through [`Self::conflicts_with`], separately from exact equality and hashing.
#[derive(Clone, Debug)]
pub struct ModSystemName(Cow<'static, str>);

impl ModSystemName {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Matches exact paths or a crate-qualified short alias of a full path.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self == other
            || (self.identity() == other.identity()
                && self.identity().0.is_some()
                && (self.0.matches("::").count() == 1 || other.0.matches("::").count() == 1))
    }

    fn identity(&self) -> (Option<&str>, &str) {
        match self.0.split_once("::") {
            Some((crate_name, path)) => (Some(crate_name), path.rsplit("::").next().unwrap()),
            None => (None, &self.0),
        }
    }
}

impl From<&'static str> for ModSystemName {
    fn from(name: &'static str) -> Self {
        Self::new(name)
    }
}

impl From<String> for ModSystemName {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl PartialEq for ModSystemName {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for ModSystemName {}

impl Hash for ModSystemName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Display for ModSystemName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A loader produces re-initializable system definitions that can be applied to any world as needed by the runtime.
///
/// For example, `wasvy_wasm` resolves access to systems inside `.wasm` asset as [ModSystemDefinition]s.
///
/// The runtime can add or remove copies of this definition when the mod is hot-reloaded.
#[derive(Clone)]
pub struct ModSystemDefinition {
    pub name: ModSystemName,
    pub schedules: HashSet<ModScheduleName>,
    pub before: HashSet<ModSystemName>,
    pub after: HashSet<ModSystemName>,
    builder: Arc<dyn ModSystemBuild>,
}

impl ModSystemDefinition {
    /// Creates an unscheduled definition with no ordering constraints.
    pub fn new(name: impl Into<ModSystemName>, builder: impl ModSystemBuild) -> Self {
        Self {
            name: name.into(),
            schedules: HashSet::new(),
            before: HashSet::new(),
            after: HashSet::new(),
            builder: Arc::new(builder),
        }
    }

    pub fn in_schedules(mut self, schedules: impl IntoIterator<Item = ModScheduleName>) -> Self {
        self.schedules.extend(schedules);
        self
    }
}

impl ModSystemBuild for ModSystemDefinition {
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
        self.builder.build(context)
    }
}

/// Declares a constructor for a fresh boxed system.
///
/// See docs for [Self::build].
pub trait ModSystemBuild: Send + Sync + 'static {
    /// Return a boxed scheduled system. Implementers must:
    /// - Build the system from the provided [ModSystemContext::world]
    /// - Have all component access honor [ModSystemContext::access]
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

pub(crate) fn retain_unique(discovery: &mut ModDiscovery) {
    let mut duplicates = HashSet::new();
    for (index, system) in discovery.systems.iter().enumerate() {
        for other in &discovery.systems[..index] {
            if system.name.conflicts_with(&other.name) {
                discovery.errors.push(anyhow!(
                    "Conflicting mod system names: {} and {}",
                    other.name,
                    system.name
                ));
                duplicates.insert(other.name.clone());
                duplicates.insert(system.name.clone());
            }
        }
    }
    discovery
        .systems
        .retain(|definition| !duplicates.contains(&definition.name));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_names() {
        let long: ModSystemName = "game::movement::integrate".into();
        let short_owned: ModSystemName = "game::integrate".to_string().into();
        assert_ne!(long, short_owned);
        assert_eq!(HashSet::from([long.clone(), short_owned]).len(), 2);
        let owned: ModSystemName = "game::movement::integrate".to_string().into();
        assert_eq!(long, owned);
        assert_eq!(HashSet::from([long.clone(), owned]).len(), 1);
        assert_eq!(long.as_str(), "game::movement::integrate");
        assert_ne!(long, "other_game::integrate".into());
        assert_ne!(long, "integrate".into());
    }

    #[test]
    fn name_conflicts() {
        for (left, right, expected) in [
            (
                "game::movement::integrate",
                "game::movement::integrate",
                true,
            ),
            ("game::movement::integrate", "game::integrate", true),
            ("game::movement::integrate", "game::other::integrate", false),
            ("game::movement::integrate", "other_game::integrate", false),
            ("game::movement::integrate", "integrate", false),
            ("integrate", "integrate", true),
        ] {
            let left = ModSystemName::from(left);
            let right = ModSystemName::from(right);
            assert_eq!(left.conflicts_with(&right), expected, "{left} vs {right}");
            assert_eq!(right.conflicts_with(&left), expected, "{right} vs {left}");
        }
    }
}
