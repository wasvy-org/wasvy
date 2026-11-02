use anyhow::Result;
use bevy_ecs::{
    component::Mutable,
    prelude::*,
    query::{FilteredAccess, QueryFilter, SingleEntityQueryData},
    system::{SystemChangeTick, SystemParam, SystemParamValidationError},
};
use variadics_please::all_tuples;

use crate::commands::RuntimeCommands;

/// A [SystemParam] that native systems can use as mod systems, see [ModSystemDefinition::new].
///
/// Each copy of a mod system builds its parameters for the [access](ModSystemContext::access) it runs in:
/// - [Query], [Single] and [Populated] only match the entities within the access. Their query data must only
///   access the queried entity ([SingleEntityQueryData]). Query filters must only read the queried entity or
///   resources, as all of Bevy's filters do.
/// - [RuntimeCommands] spawn entities within the access, see the [runtime invariants](crate::commands#runtime-invariants).
///   Like the commands of wasm mods, they can still target any entity with [RuntimeCommands::entity].
/// - Resources are shared by every access, so [Res], [ResMut] and messages ([MessageReader], [MessageWriter],
///   [MessageMutator], [PopulatedMessageReader]) are not restricted.
/// - [Local], [SystemChangeTick], [ParamSet], [Option], [Result](core::result::Result), [If] and tuples of the above.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use wasvy_runtime::{commands::RuntimeCommands, system::ModSystemDefinition};
/// #[derive(Component)]
/// struct Health(f32);
///
/// fn heal(mut healths: Query<&mut Health>, mut commands: RuntimeCommands) {
///     for mut health in &mut healths {
///         health.0 += 1.0;
///     }
///     commands.spawn(Health(100.0));
/// }
///
/// ModSystemDefinition::new(heal);
/// ```
///
/// Parameters that can reach any entity while the system runs are not supported, such as `&World`, `DeferredWorld` or
/// queries for data on other entities like `NestedQuery`. Neither are Bevy's [Commands], use [RuntimeCommands] instead.
///
/// ```compile_fail
/// # use bevy_ecs::prelude::*;
/// # use wasvy_runtime::system::ModSystemDefinition;
/// #[derive(Component)]
/// struct Health(f32);
///
/// fn heal(mut healths: Query<&mut Health>, mut commands: Commands) {
///     for mut health in &mut healths {
///         health.0 += 1.0;
///     }
///     commands.spawn(Health(100.0));
/// }
///
/// ModSystemDefinition::new(heal);
/// ```
///
/// # Safety
///
/// [Self::build_state] must return a state that is valid for this parameter (see [SystemParamBuilder]). While the
/// system runs, the parameter must only access resources and components of entities matched by the access.
///
/// Commands are applied later, with access to the whole world. They must be queued with [RuntimeCommands], which keep
/// the [runtime invariants](crate::commands#runtime-invariants).
#[diagnostic::on_unimplemented(
    message = "`{Self}` can not be used as a parameter of a mod system",
    label = "not supported by `ModSystemDefinition::new`",
    note = "see `ModSystemParam` for the supported parameters, such as `RuntimeCommands` instead of `Commands`"
)]
pub unsafe trait ModSystemParam: SystemParam {
    /// Creates the state of this parameter for a system running with `access`.
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State;
}

/// Builds any [ModSystemParam] for an access
pub(super) struct ModParamBuilder<'a>(pub(super) &'a FilteredAccess);

// Safety: ModSystemParam::build_state returns a valid state
unsafe impl<P: ModSystemParam> SystemParamBuilder<P> for ModParamBuilder<'_> {
    fn build(self, world: &mut World) -> P::State {
        P::build_state(world, self.0)
    }
}

// Safety:
// - QueryBuilder creates a valid state for the world
// - The query only matches entities within the access, and its data only accesses the matched entity
// - Bevy's filters only read the matched entity (or resources), others are not supported, see ModSystemParam docs
unsafe impl<D, F> ModSystemParam for Query<'_, '_, D, F>
where
    D: SingleEntityQueryData + 'static,
    F: QueryFilter + 'static,
{
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
        let mut builder = QueryBuilder::new(world);
        builder.extend_access(access.clone());
        builder.build()
    }
}

// Safety: Single uses the state of a Query
unsafe impl<D, F> ModSystemParam for Single<'_, '_, D, F>
where
    D: SingleEntityQueryData + 'static,
    F: QueryFilter + 'static,
{
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
        Query::<D, F>::build_state(world, access)
    }
}

// Safety: Populated uses the state of a Query
unsafe impl<D, F> ModSystemParam for Populated<'_, '_, D, F>
where
    D: SingleEntityQueryData + 'static,
    F: QueryFilter + 'static,
{
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
        Query::<D, F>::build_state(world, access)
    }
}

/// Implements [ModSystemParam] for parameters that the access does not apply to, built like for any Bevy system
macro_rules! impl_unrestricted {
    ($({$($generics:tt)*} $param:ty;)*) => {$(
        unsafe impl<$($generics)*> ModSystemParam for $param {
            fn build_state(world: &mut World, _access: &FilteredAccess) -> Self::State {
                Self::init_state(world)
            }
        }
    )*};
}

impl_unrestricted! {
    // Safety: Only queues commands, with RuntimeCommands
    {} RuntimeCommands<'_, '_>;
    // Safety: Only accesses a resource
    {T: Resource} Res<'_, T>;
    // Safety: Only accesses a resource
    {T: Resource<Mutability = Mutable>} ResMut<'_, T>;
    // Safety: Only accesses a resource
    {M: Message} MessageReader<'_, '_, M>;
    // Safety: Only accesses a resource
    {M: Message} PopulatedMessageReader<'_, '_, M>;
    // Safety: Only accesses a resource
    {M: Message} MessageWriter<'_, M>;
    // Safety: Only accesses a resource
    {M: Message} MessageMutator<'_, '_, M>;
    // Safety: Only accesses its own value while the system runs. T::from_world only runs while the system is built.
    {T: FromWorld + Send + 'static} Local<'_, T>;
    // Safety: Does not access the world
    {} SystemChangeTick;
}

// Safety: Uses the state of P
unsafe impl<P: ModSystemParam> ModSystemParam for Option<P> {
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
        P::build_state(world, access)
    }
}

// Safety: Uses the state of P
unsafe impl<P: ModSystemParam> ModSystemParam for Result<P, SystemParamValidationError> {
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
        P::build_state(world, access)
    }
}

// Safety: Uses the state of P
unsafe impl<P: ModSystemParam> ModSystemParam for If<P> {
    fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
        P::build_state(world, access)
    }
}

macro_rules! impl_tuple {
    ($($param:ident),*) => {
        // Safety: Uses the state of each parameter
        unsafe impl<$($param: ModSystemParam),*> ModSystemParam for ($($param,)*) {
            #[allow(unused_variables, clippy::unused_unit)]
            fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
                ($($param::build_state(world, access),)*)
            }
        }
    };
}

// Same lengths as Bevy's tuples of system params
all_tuples!(impl_tuple, 0, 16, P);

macro_rules! impl_param_set {
    ($($param:ident),*) => {
        // Safety: Uses the state of each parameter
        unsafe impl<$($param: ModSystemParam),*> ModSystemParam for ParamSet<'_, '_, ($($param,)*)> {
            fn build_state(world: &mut World, access: &FilteredAccess) -> Self::State {
                <($($param,)*)>::build_state(world, access)
            }
        }
    };
}

// Same lengths as Bevy's param sets
all_tuples!(impl_param_set, 1, 8, P);

#[cfg(test)]
mod tests {
    use bevy_app::Update;
    use bevy_ecs::{system::SystemParamValidationError, utils::DebugName};

    use crate::{
        cleanup::DespawnModEntities,
        commands::RuntimeCommands,
        sandbox::{Sandbox, Sandboxed},
        schedule::{ModScheduleName, ModSchedules},
        system::*,
    };

    #[derive(Component)]
    struct Count(usize);

    /// Entities that systems know about, even those outside their access
    #[derive(Resource)]
    struct Targets(Vec<Entity>);

    fn query(mut counts: Query<&mut Count>) {
        for mut count in &mut counts {
            count.0 += 1;
        }
    }

    fn targeted(mut counts: Query<&mut Count>, targets: Res<Targets>) {
        for entity in &targets.0 {
            if let Ok(mut count) = counts.get_mut(*entity) {
                count.0 += 1;
            }
        }
    }

    /// Matches every entity, whether it is sandboxed or not
    type Everything = Or<(With<Sandboxed>, Without<Sandboxed>)>;

    fn filtered(mut counts: Query<&mut Count, Everything>) {
        for mut count in &mut counts {
            count.0 += 1;
        }
    }

    fn single(mut count: Single<&mut Count>) {
        count.0 += 1;
    }

    fn optional_single(count: Option<Single<&mut Count>>) {
        if let Some(mut count) = count {
            count.0 += 1;
        }
    }

    fn fallible_single(count: Result<Single<&mut Count>, SystemParamValidationError>) {
        if let Ok(mut count) = count {
            count.0 += 1;
        }
    }

    fn if_single(If(mut count): If<Single<&mut Count>>) {
        count.0 += 1;
    }

    fn populated(mut counts: Populated<&mut Count>) {
        for mut count in &mut counts {
            count.0 += 1;
        }
    }

    fn entity_mut(mut entities: Query<EntityMut>) {
        for mut entity in &mut entities {
            if let Some(mut count) = entity.get_mut::<Count>() {
                count.0 += 1;
            }
        }
    }

    fn param_set(mut set: ParamSet<(Query<&Count>, Query<&mut Count>)>) {
        for mut count in &mut set.p1() {
            count.0 += 1;
        }
    }

    /// Each system adds 1 to the count of every entity it can access
    fn counting_systems() -> Vec<ModSystemDefinition> {
        [
            ModSystemDefinition::new(query),
            ModSystemDefinition::new(targeted),
            ModSystemDefinition::new(filtered),
            ModSystemDefinition::new(single),
            ModSystemDefinition::new(optional_single),
            ModSystemDefinition::new(fallible_single),
            ModSystemDefinition::new(if_single),
            ModSystemDefinition::new(populated),
            ModSystemDefinition::new(entity_mut),
            ModSystemDefinition::new(param_set),
        ]
        .map(|definition| definition.in_schedules([ModScheduleName::Update]))
        .into()
    }

    fn test_world() -> World {
        let mut world = World::new();
        world.register_component::<Sandboxed>();
        world.init_resource::<Schedules>();
        world.init_resource::<ModSchedules>();
        world
    }

    fn sandbox(world: &mut World) -> Entity {
        let sandbox = Sandbox::new(world, ModSchedules::default());
        world.spawn(sandbox).id()
    }

    fn install(
        world: &mut World,
        definitions: &[ModSystemDefinition],
        accesses: &[ModAccess],
    ) -> Entity {
        let mod_id = world.spawn_empty().id();
        let mut errors = Errors::new();
        for access in accesses {
            add_systems(&mut errors, definitions, world, mod_id, "test mod", access);
        }
        assert!(errors.is_empty(), "{errors:?}");
        mod_id
    }

    /// An entity with a [Count] in the world, in sandboxes `a` and `b`, and in a sandbox nested in `a`
    struct Counts {
        a: Entity,
        b: Entity,
        entities: [Entity; 4],
    }

    impl Counts {
        fn new(world: &mut World) -> Self {
            let a = sandbox(world);
            let b = sandbox(world);
            let in_world = world.spawn(Count(0)).id();
            let in_a = world.spawn((Count(0), ChildOf(a))).id();
            let in_b = world.spawn((Count(0), ChildOf(b))).id();
            let nested = Sandbox::new(world, ModSchedules::default());
            let nested = world.spawn((nested, ChildOf(in_a))).id();
            let in_nested = world.spawn((Count(0), ChildOf(nested))).id();
            let entities = [in_world, in_a, in_b, in_nested];
            world.insert_resource(Targets(entities.to_vec()));
            Self { a, b, entities }
        }

        /// The counts of the entities in the world, `a`, `b` and the nested sandbox
        fn get(&self, world: &World) -> [usize; 4] {
            self.entities
                .map(|entity| world.get::<Count>(entity).unwrap().0)
        }
    }

    #[test]
    fn sandboxed_system_does_not_mutate_other_sandboxes() {
        let mut world = test_world();
        let counts = Counts::new(&mut world);
        let systems = counting_systems();
        install(&mut world, &systems, &[ModAccess::Sandbox(counts.a)]);

        world.run_schedule(Update);
        assert_eq!(counts.get(&world), [0, systems.len(), 0, 0]);
    }

    #[test]
    fn copy_per_access() {
        let mut world = test_world();
        let counts = Counts::new(&mut world);
        let systems = counting_systems();
        install(
            &mut world,
            &systems,
            &[
                ModAccess::World,
                ModAccess::Sandbox(counts.a),
                ModAccess::Sandbox(counts.b),
            ],
        );

        world.run_schedule(Update);
        let n = systems.len();
        assert_eq!(counts.get(&world), [n, n, n, 0]);
    }

    #[test]
    fn copies_can_run_in_parallel() {
        let mut world = test_world();
        let counts = Counts::new(&mut world);
        let accesses = [
            ModAccess::World,
            ModAccess::Sandbox(counts.a),
            ModAccess::Sandbox(counts.b),
        ];
        for definition in counting_systems() {
            let copies: Vec<_> = accesses
                .iter()
                .map(|access| {
                    let context = ModSystemContext::new(&mut world, "test mod", access);
                    let mut system = definition.build(context).unwrap();
                    system.initialize(&mut world)
                })
                .collect();

            for (index, copy) in copies.iter().enumerate() {
                for other in &copies[..index] {
                    assert!(copy.is_compatible(other), "{}", definition.name);
                }
            }
        }
    }

    #[test]
    fn native_names() {
        let mut world = test_world();
        let definition = ModSystemDefinition::new(query);
        assert_eq!(definition.name.as_str(), std::any::type_name_of_val(&query));

        let context = ModSystemContext::new(&mut world, "test mod", &ModAccess::World);
        let system = definition.build(context).unwrap();
        assert_eq!(system.name(), DebugName::type_name_of_val(&query));
    }

    #[test]
    fn runtime_commands_spawn_within_access() {
        #[derive(Component)]
        struct Spawned;

        fn spawn_once(mut commands: RuntimeCommands, mut spawned: Local<bool>) {
            if !*spawned {
                commands.spawn(Spawned);
                *spawned = true;
            }
        }

        let mut world = test_world();
        let sandbox = sandbox(&mut world);
        let definition =
            ModSystemDefinition::new(spawn_once).in_schedules([ModScheduleName::Update]);
        let mod_id = install(
            &mut world,
            &[definition],
            &[ModAccess::World, ModAccess::Sandbox(sandbox)],
        );

        world.run_schedule(Update);
        world.run_schedule(Update);
        let mut parents: Vec<_> = world
            .query_filtered::<Option<&ChildOf>, With<Spawned>>()
            .iter(&world)
            .map(|parent| parent.map(ChildOf::parent))
            .collect();
        parents.sort();
        assert_eq!(parents, [None, Some(sandbox)], "one entity per access");
        assert_eq!(world.get::<DespawnModEntities>(mod_id).unwrap().len(), 2);
    }
}
