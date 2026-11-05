use std::sync::atomic::{AtomicUsize, Ordering};

use bevy_app::{PostUpdate, Update};
use bevy_ecs::schedule::ScheduleLabel;

use super::*;
use crate::{
    cleanup::DespawnModEntities,
    commands::RuntimeCommands,
    sandbox::{Sandbox, Sandboxed},
    schedule::{ModSchedule, ModSchedules},
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
                    .filter(|run| run.name == name && run.access == access.filtered_access(&world))
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
