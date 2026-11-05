use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::anyhow;
use bevy_asset::Asset;
#[cfg(test)]
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashSet;
use bevy_reflect::{Reflect, TypePath};
use error_collection::Errors;
use serde::{Deserialize, Serialize};

use crate::system::{ModSystem, ModSystemName, ModSystemNamespace};
#[cfg(test)]
use crate::{access::ModAccess, cleanup::DespawnModEntities, system::add_systems};
#[cfg(test)]
use anyhow::Result;

/// Definitions discovered in a loaded mod revision, including partial successes.
#[derive(Default)]
pub struct ModDiscovery {
    pub systems: Vec<ModSystem>,
    pub errors: Errors,
}

impl ModDiscovery {
    pub(crate) fn replace_missing_namespace(&mut self, namespace: &ModSystemNamespace) {
        for system in &mut self.systems {
            system.name.replace_missing_namespace(namespace);
            for names in [&mut system.before, &mut system.after] {
                *names = std::mem::take(names)
                    .into_iter()
                    .map(|mut name| {
                        name.replace_missing_namespace(namespace);
                        name
                    })
                    .collect();
            }
        }
    }

    /// Must run after [Self::replace_missing_namespace]
    pub(crate) fn retain_unique(&mut self) {
        let mut duplicates = HashSet::new();
        for (index, system) in self.systems.iter().enumerate() {
            for other in &self.systems[..index] {
                if system.name == other.name {
                    self.errors.push(anyhow!(
                        "Conflicting mod system names: {} and {}",
                        other.name,
                        system.name
                    ));
                    duplicates.insert(other.name.clone());
                    duplicates.insert(system.name.clone());
                }
            }
        }
        self.systems
            .retain(|definition| !duplicates.contains(&definition.name));
    }
}

/// Permission for a [ModAsset] to replace or delete previously published systems on load. New names are always allowed.
///
/// Belongs to the [ModAsset], so every [Mod](crate::mods::Mod) using it shares the same replacement. Each published
/// revision is checked against this setting. Changing it does not revoke prior replacements or deletions.
/// Narrowing the policy can leave old systems running after hot reload. For best results choose one per asset and stick with it.
#[derive(Clone, Debug, Default, Reflect, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModReplacement {
    /// A newly loaded mod may override any [ModSystem], overriding any existing behaviour in your game
    #[default]
    Any,

    /// A newly loaded mod may never override existing [ModSystem]s.
    ///
    /// Warning: this will break hot-reloading, since a mod will not be able to patch the systems it declared
    None,

    /// A newly loaded mod may only override [ModSystem]s in the same namespace as the mod itself
    Namespace,

    /// A newly loaded mod may only override specified [ModSystem]s.
    Only(HashSet<ModSystemName>),
}

/// [AssetLoader](bevy_asset::AssetLoader) settings for loaders producing [ModAsset]s.
///
/// Set them with [AssetServer::load_builder](bevy_asset::AssetServer::load_builder) or
/// [Mods::load_with_settings](crate::mods::Mods::load_with_settings). The AssetServer applies them again
/// when it reloads the asset. It ignores the settings of later loads of an already loaded path
/// (<https://github.com/bevyengine/bevy/issues/11111>), so the first load of a path decides its policy.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModLoaderSettings {
    /// See [ModReplacement]
    pub replacements: ModReplacement,
}

impl From<ModReplacement> for ModLoaderSettings {
    fn from(replacements: ModReplacement) -> Self {
        Self { replacements }
    }
}

/// An asset representing a loaded wasvy mod.
#[derive(Asset, TypePath)]
pub struct ModAsset {
    systems: Arc<[ModSystem]>,
    errors: Arc<Errors>,
    namespace: ModSystemNamespace,
    settings: ModLoaderSettings,
    #[cfg(test)]
    pub(crate) revision: u64,
    pub(crate) retain_source: bool,
}

impl ModAsset {
    /// Creates a mod asset after a completed "discovery phase" by a mod loader.
    ///
    /// You must first provide a unique namespace identifier for this mod.
    ///
    /// During mod discovery the asset loader creates:
    /// - [ModSystem]s
    /// - Optional errors that signify partial failures of mod setup
    ///
    /// The asset loader may also provide errors that happened during mod discovery.
    /// These usually signify partial/incomplete loading of systems.
    pub fn new(namespace: impl Into<ModSystemNamespace>, mut discovery: ModDiscovery) -> Self {
        let namespace = namespace.into();
        discovery.replace_missing_namespace(&namespace);
        discovery.retain_unique();
        #[cfg(test)]
        static REVISION: AtomicU64 = AtomicU64::new(1);
        Self {
            retain_source: false,
            #[cfg(test)]
            revision: REVISION.fetch_add(1, Ordering::Relaxed),
            namespace,
            settings: ModLoaderSettings::default(),
            systems: discovery.systems.into(),
            errors: discovery.errors.into(),
        }
    }

    /// Keeps the loader asset alive while its published systems are in use.
    pub fn with_source_retention(mut self) -> Self {
        self.retain_source = true;
        self
    }

    /// Applies [ModLoaderSettings] to the loaded mod asset.
    ///
    /// When manually replacing (not hot-reloading) the asset with a new one, remember to use the same settings again.
    /// See the docs for [ModLoaderSettings] and [ModReplacement].
    pub fn with_settings(mut self, settings: impl Into<ModLoaderSettings>) -> Self {
        self.settings = settings.into();
        self
    }

    /// See [ModLoaderSettings]
    pub fn settings(&self) -> &ModLoaderSettings {
        &self.settings
    }

    /// The reusable definitions prepared for this loaded revision.
    pub fn system(&self) -> &[ModSystem] {
        &self.systems
    }

    /// Discovery diagnostics. Missing systems are not deletions when this is nonempty.
    pub fn errors(&self) -> &Errors {
        &self.errors
    }

    pub fn systems(&self) -> &[ModSystem] {
        &self.systems
    }

    pub fn namespace(&self) -> &ModSystemNamespace {
        &self.namespace
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bevy_app::{PostUpdate, Update};
    use bevy_ecs::{query::FilteredAccess, schedule::ScheduleLabel, system::BoxedSystem};

    use super::*;
    use crate::{
        commands::RuntimeCommands,
        mods::ModSystemSet,
        sandbox::{Sandbox, Sandboxed},
        schedule::{ModSchedule, ModScheduleName, ModSchedules},
        system::{ModSystemBuild, ModSystemContext, ModSystemName},
    };

    #[derive(Default, Resource)]
    struct Runs(Vec<Run>);

    struct Run {
        name: &'static str,
        access: FilteredAccess,
        count: usize,
    }

    struct CountingSystem {
        name: &'static str,
        builds: Arc<AtomicUsize>,
        fail: bool,
    }

    unsafe impl ModSystemBuild for CountingSystem {
        fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
            self.builds.fetch_add(1, Ordering::Relaxed);
            anyhow::ensure!(!self.fail, "cannot build {}", self.name);
            let name = self.name;
            let access = context.access;
            Ok(Box::new(
                IntoSystem::into_system(move |mut count: Local<usize>, mut runs: ResMut<Runs>| {
                    *count += 1;
                    runs.0.push(Run {
                        name,
                        access: access.clone(),
                        count: *count,
                    });
                })
                .with_name(name),
            ))
        }
    }

    fn system(name: &'static str) -> ModSystem {
        ModSystem::custom(
            name,
            CountingSystem {
                name,
                builds: Arc::default(),
                fail: false,
            },
        )
        .in_schedules([ModScheduleName::Update])
    }

    fn test_world() -> World {
        let mut world = World::new();
        world.register_component::<Sandboxed>();
        world.init_resource::<Schedules>();
        world.init_resource::<ModSchedules>();
        world.init_resource::<Runs>();
        world
    }

    fn sandbox_access(world: &mut World) -> ModAccess {
        let sandbox = Sandbox::new(world, world.resource::<ModSchedules>().clone());
        ModAccess::Sandbox(world.spawn(sandbox).id())
    }

    fn install(world: &mut World, systems: &[ModSystem], accesses: &[ModAccess]) -> Entity {
        let mod_id = world.spawn_empty().id();
        let mut errors = Errors::new();
        for access in accesses {
            add_systems(&mut errors, systems, world, mod_id, "test mod", access);
        }
        assert!(errors.is_empty(), "{errors:?}");
        mod_id
    }

    #[test]
    fn replace_missing_namespaces_in_names_and_ordering() {
        let names = ["tick", "::empty", "other::tick"];
        let mut discovery = ModDiscovery {
            systems: names.map(system).into(),
            ..Default::default()
        };
        for system in &mut discovery.systems {
            system.before = names.map(ModSystemName::new).into();
            system.after = system.before.clone();
        }

        discovery.replace_missing_namespace(&"game".into());
        discovery.replace_missing_namespace(&"another".into());

        for (system, expected_namespace) in discovery.systems.iter().zip(["game", "game", "other"])
        {
            assert_eq!(system.name.namespace().as_str(), Some(expected_namespace));
            assert_eq!(system.before.len(), names.len());
            assert_eq!(system.after.len(), names.len());
            for target in &discovery.systems {
                assert!(system.before.contains(&target.name));
                assert!(system.after.contains(&target.name));
            }
        }
    }

    #[test]
    fn remove_duplicates() {
        let mut discovery = ModDiscovery {
            systems: vec![
                system("game::one::first"),
                system("game::first"),
                system("other::two::second"),
                system("other::second"),
                system("identical"),
                system("identical"),
                system("unique"),
            ],
            ..Default::default()
        };
        discovery.retain_unique();

        assert_eq!(discovery.errors.len(), 1);
        assert!(discovery.errors[0].to_string().contains("identical"));
        assert_eq!(discovery.systems.len(), 5);
    }

    #[test]
    fn distinct_siblings() {
        let mut discovery = ModDiscovery {
            systems: vec![system("game::one::tick"), system("game::two::tick")],
            ..Default::default()
        };
        discovery.retain_unique();
        assert!(discovery.errors.is_empty());
        assert_eq!(discovery.systems.len(), 2);
    }

    #[test]
    fn short_paths_are_distinct_identities() {
        for alias_first in [false, true] {
            let mut discovery = ModDiscovery {
                systems: vec![
                    system("game::one::tick"),
                    system("game::two::tick"),
                    system("game::tick"),
                ],
                ..Default::default()
            };
            if alias_first {
                discovery.systems.reverse();
            }
            discovery.retain_unique();
            assert!(discovery.errors.is_empty());
            assert_eq!(discovery.systems.len(), 3);
        }
    }

    #[test]
    fn distinct_module_order() {
        let mut world = test_world();
        let mut first = system("game::one::tick");
        first.after.insert("game::two::tick".into());
        install(
            &mut world,
            &[first, system("game::two::tick")],
            &[ModAccess::World],
        );
        world.run_schedule(Update);
        let names: Vec<_> = world
            .resource::<Runs>()
            .0
            .iter()
            .map(|run| run.name)
            .collect();
        assert_eq!(names, ["game::two::tick", "game::one::tick"]);
    }

    #[test]
    fn ordering_targets_exact_names() {
        let mut world = test_world();
        let mut last = system("last");
        last.after.insert("game::tick".into());
        install(
            &mut world,
            &[
                last,
                system("game::tick"),
                system("game::one::tick"),
                system("game::two::tick"),
            ],
            &[ModAccess::World],
        );
        world.run_schedule(Update);
        let schedule = world.resource::<Schedules>().get(Update).unwrap();
        let graph = schedule.graph();
        let names: std::collections::HashMap<_, _> = schedule
            .systems()
            .unwrap()
            .map(|(key, system)| {
                (
                    bevy_ecs::schedule::NodeId::System(key),
                    system.name().to_string(),
                )
            })
            .collect();
        let mut predecessors: Vec<_> = graph
            .dependency()
            .graph()
            .all_edges()
            .filter(|(_, target)| names.get(target).map(String::as_str) == Some("last"))
            .flat_map(|(source, _)| graph.hierarchy().graph().neighbors(source))
            .map(|node| names[&node].as_str())
            .collect();
        predecessors.sort();
        assert_eq!(predecessors, ["game::tick"]);
        let runs = &world.resource::<Runs>().0;
        let target = runs
            .iter()
            .position(|run| run.name == "game::tick")
            .unwrap();
        let last = runs.iter().position(|run| run.name == "last").unwrap();
        assert!(target < last);
    }

    #[test]
    fn access_local_state() {
        let mut world = test_world();
        let sandbox = sandbox_access(&mut world);
        install(
            &mut world,
            &[system("counter")],
            &[ModAccess::World, sandbox],
        );

        world.run_schedule(Update);
        world.run_schedule(Update);

        for access in [ModAccess::World, sandbox] {
            let counts: Vec<_> = world
                .resource::<Runs>()
                .0
                .iter()
                .filter(|run| run.access == access.filtered_access(&world))
                .map(|run| run.count)
                .collect();
            assert_eq!(counts, [1, 2], "each access owns its own counter");
        }
    }

    #[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
    struct Custom;

    #[test]
    fn schedule_local_state() {
        let mut world = test_world();
        world.insert_resource(ModSchedules(
            [
                ModSchedule::Update,
                ModSchedule::new_custom("custom", Custom),
            ]
            .into(),
        ));
        let builds = Arc::new(AtomicUsize::new(0));
        let mut counter = ModSystem::custom(
            "counter",
            CountingSystem {
                name: "counter",
                builds: builds.clone(),
                fail: false,
            },
        )
        .in_schedules([ModScheduleName::Update]);
        counter
            .schedules
            .extend([ModScheduleName::Custom("custom".into())]);
        install(&mut world, &[counter], &[ModAccess::World]);

        world.run_schedule(Update);
        world.run_schedule(Update);
        world.run_schedule(Custom);

        let counts: Vec<_> = world
            .resource::<Runs>()
            .0
            .iter()
            .map(|run| run.count)
            .collect();
        assert_eq!(counts, [1, 2, 1]);
        assert_eq!(builds.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn disabled_schedule() {
        let mut world = test_world();
        world.insert_resource(ModSchedules([ModSchedule::Update].into()));
        let builds = Arc::new(AtomicUsize::new(0));
        let counter = ModSystem::custom(
            "counter",
            CountingSystem {
                name: "counter",
                builds: builds.clone(),
                fail: false,
            },
        )
        .in_schedules([ModScheduleName::Update, ModScheduleName::PostUpdate]);
        install(&mut world, &[counter], &[ModAccess::World]);
        world.run_schedule(Update);
        assert_eq!(world.resource::<Runs>().0.len(), 1);
        assert_eq!(builds.load(Ordering::Relaxed), 1);
        assert!(world.resource::<Schedules>().get(PostUpdate).is_none());
    }

    #[test]
    fn system_groups() {
        #[derive(Resource)]
        struct DisabledSet(ModSystemSet);

        let mut world = test_world();
        let sandbox = sandbox_access(&mut world);
        let mod_id = install(
            &mut world,
            &[system("first"), system("second")],
            &[ModAccess::World, sandbox],
        );
        install(&mut world, &[system("other")], &[ModAccess::World]);
        world.run_schedule(Update);

        let schedule = world.resource::<Schedules>().get(Update).unwrap();
        for (set, expected) in [
            (ModSystemSet::All, 5),
            (ModSystemSet::Mod(mod_id), 4),
            (ModSystemSet::Access(ModAccess::World), 3),
            (ModSystemSet::Access(sandbox), 2),
        ] {
            assert_eq!(
                schedule.graph().systems_in_set(set.intern()).unwrap().len(),
                expected
            );
        }
        for set in [
            ModSystemSet::All,
            ModSystemSet::Mod(mod_id),
            ModSystemSet::Access(ModAccess::World),
            ModSystemSet::Access(sandbox),
        ] {
            let disabled_set = set.clone();
            world.resource_mut::<Schedules>().configure_sets(
                Update,
                set.run_if(move |disabled: Res<DisabledSet>| disabled.0 != disabled_set),
            );
        }
        for (set, expected) in [
            (ModSystemSet::All, vec![]),
            (ModSystemSet::Mod(mod_id), vec![("other", ModAccess::World)]),
            (
                ModSystemSet::Access(ModAccess::World),
                vec![("first", sandbox), ("second", sandbox)],
            ),
            (
                ModSystemSet::Access(sandbox),
                vec![
                    ("first", ModAccess::World),
                    ("second", ModAccess::World),
                    ("other", ModAccess::World),
                ],
            ),
        ] {
            world.insert_resource(DisabledSet(set));
            world.resource_mut::<Runs>().0.clear();
            world.run_schedule(Update);
            let runs = &world.resource::<Runs>().0;
            assert_eq!(runs.len(), expected.len());
            for (name, access) in expected {
                assert_eq!(
                    runs.iter()
                        .filter(
                            |run| run.name == name && run.access == access.filtered_access(&world)
                        )
                        .count(),
                    1
                );
            }
        }
    }

    fn assert_order_with_sandbox_first(systems: Vec<ModSystem>) {
        let mut world = test_world();
        let sandbox = sandbox_access(&mut world);
        install(&mut world, &systems, &[ModAccess::World, sandbox]);
        // Reversing access order would create a cycle if copy IDs leaked across accesses.
        world.resource_mut::<Schedules>().configure_sets(
            Update,
            ModSystemSet::Access(sandbox).before(ModSystemSet::Access(ModAccess::World)),
        );
        world.run_schedule(Update);

        let actual: Vec<_> = world
            .resource::<Runs>()
            .0
            .iter()
            .map(|run| (run.name, run.access.clone()))
            .collect();
        assert_eq!(
            actual,
            [
                ("game::module::first", sandbox.filtered_access(&world)),
                ("second", sandbox.filtered_access(&world)),
                (
                    "game::module::first",
                    ModAccess::World.filtered_access(&world)
                ),
                ("second", ModAccess::World.filtered_access(&world)),
            ]
        );
    }

    #[test]
    fn before_order() {
        for reverse in [false, true] {
            let mut first = system("game::module::first");
            first.before.insert("second".into());
            let mut systems = vec![first, system("second")];
            if reverse {
                systems.reverse();
            }
            assert_order_with_sandbox_first(systems);
        }
    }

    #[test]
    fn after_exact_name() {
        for reverse in [false, true] {
            let mut second = system("second");
            second.after.insert("game::module::first".into());
            let mut systems = vec![second, system("game::module::first")];
            if reverse {
                systems.reverse();
            }
            assert_order_with_sandbox_first(systems);
        }
    }

    #[test]
    fn commands_per_access() {
        struct NativeCommandSystem;

        unsafe impl ModSystemBuild for NativeCommandSystem {
            fn build(&self, _context: ModSystemContext<'_>) -> Result<BoxedSystem> {
                Ok(Box::new(IntoSystem::into_system(
                    |mut commands: RuntimeCommands| {
                        commands.spawn_empty();
                    },
                )))
            }
        }

        let mut world = test_world();
        let sandbox = sandbox_access(&mut world);
        let definition =
            ModSystem::custom("spawn", NativeCommandSystem).in_schedules([ModScheduleName::Update]);
        let mod_id = install(&mut world, &[definition], &[ModAccess::World, sandbox]);

        world.run_schedule(Update);
        let entities = world.get::<DespawnModEntities>(mod_id).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(
            entities
                .iter()
                .filter(|entity| world.get::<Sandboxed>(*entity).is_some())
                .count(),
            1,
        );
        let ModAccess::Sandbox(sandbox_id) = sandbox else {
            unreachable!()
        };
        for entity in entities.iter() {
            if let Some(membership) = world.get::<Sandboxed>(entity) {
                use bevy_ecs::relationship::Relationship;
                assert_eq!(membership.get(), sandbox_id);
                assert_eq!(world.get::<ChildOf>(entity).unwrap().parent(), sandbox_id);
            } else {
                assert!(world.get::<ChildOf>(entity).is_none());
            }
        }
    }

    #[test]
    fn partial_build() {
        let mut world = test_world();
        let mut systems = vec![
            system("broken first"),
            system("working"),
            system("broken last"),
        ];
        for index in [0, 2] {
            systems[index] = ModSystem::custom(
                systems[index].name.clone(),
                CountingSystem {
                    name: "broken",
                    builds: Arc::default(),
                    fail: true,
                },
            )
            .in_schedules([ModScheduleName::Update]);
        }
        let mod_id = world.spawn_empty().id();
        let mut errors = Errors::new();
        add_systems(
            &mut errors,
            &systems,
            &mut world,
            mod_id,
            "test mod",
            &ModAccess::World,
        );

        assert_eq!(errors.len(), 2);
        world.run_schedule(Update);
        let runs = &world.resource::<Runs>().0;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].name, "working");
    }
}
