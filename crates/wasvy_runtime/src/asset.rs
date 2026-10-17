use std::{fmt, sync::Arc};

use anyhow::Result;
use bevy_asset::{Asset, AssetId, Assets};
use bevy_ecs::prelude::*;
use bevy_reflect::TypePath;
use error_collection::Errors;

use crate::system::{ModSystemDefinition, add_systems, retain_unique};
use crate::{access::ModAccess, cleanup::DespawnModEntities, mods::ModDespawnBehaviour};

/// Definitions discovered in a loaded mod revision, including partial successes.
#[derive(Default)]
pub struct ModDiscovery {
    pub systems: Vec<ModSystemDefinition>,
    pub errors: Errors,
}

/// An asset representing a loaded wasvy mod.
#[derive(Asset, TypePath)]
pub struct ModAsset {
    systems: Arc<[ModSystemDefinition]>,
    errors: Arc<Errors>,
}

impl ModAsset {
    /// Creates a mod asset after a completed "discovery phase" by a mod loader.
    ///
    /// During mod discovery the asset loader creates:
    /// - [ModSystemDefinition]s
    ///
    /// The asset loader may also provide errors that happened during mod discovery.
    /// These usually signify partial/inclomplete loading of systems.
    pub fn new(mut discovery: ModDiscovery) -> Self {
        // Reject conflicting systems
        retain_unique(&mut discovery);
        Self {
            systems: discovery.systems.into(),
            errors: discovery.errors.into(),
        }
    }

    /// The reusable definitions prepared for this loaded revision.
    pub fn system(&self) -> &[ModSystemDefinition] {
        &self.systems
    }

    /// Initialize the mod into the world
    pub(crate) fn initiate(
        world: &mut World,
        asset_id: &AssetId<ModAsset>,
        mod_id: Entity,
        mod_name: &str,
        accesses: &[ModAccess],
    ) -> Result<Errors> {
        let assets = world
            .get_resource::<Assets<Self>>()
            .expect("ModAssets be registered");
        let asset = assets.get(*asset_id).ok_or(AssetNotFound)?;
        let systems = Arc::clone(&asset.systems);
        let mut errors = Errors::new();
        if !asset.errors.is_empty() {
            errors.push(anyhow::Error::new(Arc::clone(&asset.errors)));
        }

        if ModDespawnBehaviour::should_despawn_entities(world) {
            let (entities, mut commands) = world.entities_and_commands();
            let despawn = entities
                .get(mod_id)
                .expect("Mod entity exists")
                .get::<DespawnModEntities>()
                .expect(
                    "DespawnModEntities should have been registered as a required componet for Mod",
                );
            for source_entity in despawn.iter() {
                commands.entity(source_entity).try_despawn();
            }
        }

        for access in accesses {
            add_systems(&mut errors, &systems, world, mod_id, mod_name, access);
        }
        Ok(errors)
    }
}

#[derive(Debug)]
pub(crate) struct AssetNotFound;

impl std::error::Error for AssetNotFound {}

impl fmt::Display for AssetNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Asset not found. Maybe it hasn't loaded yet.")
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
        system::{ModSystemBuild, ModSystemContext},
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

    impl ModSystemBuild for CountingSystem {
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

    fn system(name: &'static str) -> ModSystemDefinition {
        ModSystemDefinition::new(
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

    fn install(
        world: &mut World,
        systems: &[ModSystemDefinition],
        accesses: &[ModAccess],
    ) -> Entity {
        let mod_id = world.spawn_empty().id();
        let mut errors = Errors::new();
        for access in accesses {
            add_systems(&mut errors, systems, world, mod_id, "test mod", access);
        }
        assert!(errors.is_empty(), "{errors:?}");
        mod_id
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
        retain_unique(&mut discovery);

        assert_eq!(discovery.errors.len(), 3);
        assert!(discovery.errors[0].to_string().contains("first"));
        assert!(discovery.errors[1].to_string().contains("second"));
        assert!(discovery.errors[2].to_string().contains("identical"));
        assert_eq!(discovery.systems.len(), 1);
        assert_eq!(discovery.systems[0].name.as_str(), "unique");
    }

    #[test]
    fn distinct_siblings() {
        let mut discovery = ModDiscovery {
            systems: vec![system("game::one::tick"), system("game::two::tick")],
            ..Default::default()
        };
        retain_unique(&mut discovery);
        assert!(discovery.errors.is_empty());
        assert_eq!(discovery.systems.len(), 2);
    }

    #[test]
    fn short_alias_conflict() {
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
            retain_unique(&mut discovery);
            assert_eq!(discovery.errors.len(), 2);
            assert!(discovery.systems.is_empty());
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
    fn alias_order_targets() {
        let mut world = test_world();
        let mut last = system("last");
        last.after.insert("game::tick".into());
        install(
            &mut world,
            &[last, system("game::one::tick"), system("game::two::tick")],
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
        assert_eq!(predecessors, ["game::one::tick", "game::two::tick"]);
        assert_eq!(world.resource::<Runs>().0.last().unwrap().name, "last");
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
        let mut counter = ModSystemDefinition::new(
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
        let counter = ModSystemDefinition::new(
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

    fn assert_order_with_sandbox_first(systems: Vec<ModSystemDefinition>) {
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
    fn after_short_alias() {
        for reverse in [false, true] {
            let mut second = system("second");
            second.after.insert("game::first".into());
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

        impl ModSystemBuild for NativeCommandSystem {
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
        let definition = ModSystemDefinition::new("spawn", NativeCommandSystem)
            .in_schedules([ModScheduleName::Update]);
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
            systems[index] = ModSystemDefinition::new(
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
