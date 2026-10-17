//! Bevy-style commands that maintain wasvy's [runtime invariants](#runtime-invariants).
//!
//! Use [`RuntimeCommands`] and [`RuntimeEntityCommands`] as the runtime (wasvy_runtime)
//! equivalents of Bevy's [`Commands`] and [`EntityCommands`]. Their APIs handle keeping
//! invariants valid.
//!
//! # Runtime invariants
//!
//! Runtime invariants are the bookkeeping rules wasvy relies on when managing
//! entities spawned by mods:
//!
//! - **Sandbox membership:** with [`ModAccess::Sandbox`], newly spawned roots are
//!   attached to the sandbox through [`ChildOf`]. Descendants keep their hierarchy,
//!   and the sandbox's hooks maintain their [`Sandboxed`](crate::sandbox::Sandboxed)
//!   membership. With [`ModAccess::World`], no sandbox parent is added.
//! - **Entity cleanup:** newly spawned entities are associated with their mod
//!   through [`DespawnModEntity`] when [`ModDespawnBehaviour`] enables cleanup.
//!   This lets the runtime remove them when the mod is despawned or reloaded.
//!
//! The runtime captures these settings for each mod system. Nested runtime
//! commands inherit that configuration, and [`RuntimeEntityCommands::with_child`]
//! keeps new children under the requested parent.
//!
//! Spawning applies these rules to the new root and its [`Children`] descendants,
//! including descendants created by bundle effects or scenes. Other relationships
//! and unrelated entities created by hooks or effects are outside this bookkeeping.
//! Mutating an existing entity does not claim it for the mod; bundle effects of
//! [`RuntimeEntityCommands::insert`] are not tracked. Use the spawning helpers for
//! new entities that should follow the runtime invariants.
//!
//! These rules do not validate arbitrary entity IDs or provide a security boundary.
//! See [`Sandbox`](crate::sandbox::Sandbox) for the scope and limits of sandbox isolation.

use bevy_ecs::{
    component::ComponentId,
    prelude::*,
    reflect::ReflectCommandExt,
    system::{SystemParam, SystemState},
};
use bevy_reflect::PartialReflect;
use bevy_scene::{EntityWorldMutSceneExt, Scene};

use crate::{access::ModAccess, cleanup::DespawnModEntity, mods::ModDespawnBehaviour};

/// Configuration for the [runtime invariants](self#runtime-invariants) of
/// [`RuntimeCommands`].
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct RuntimeCommandsConfig {
    parent: Option<Entity>,
    owner: Option<Entity>,
}

impl RuntimeCommandsConfig {
    pub(crate) fn new(world: &World, mod_id: Entity, access: ModAccess) -> Self {
        Self {
            parent: match access {
                ModAccess::World => None,
                ModAccess::Sandbox(entity) => Some(entity),
            },
            owner: ModDespawnBehaviour::should_despawn_entities(world).then_some(mod_id),
        }
    }
}

#[derive(Resource, Debug, PartialEq)]
struct RuntimeCommandsInit(RuntimeCommandsConfig);

impl FromWorld for RuntimeCommandsInit {
    fn from_world(world: &mut World) -> Self {
        Self(
            world
                .get_resource::<Self>()
                .map(|state| state.0)
                .unwrap_or_default(),
        )
    }
}

pub(crate) fn with_runtime_commands_config<Output>(
    world: &mut World,
    config: RuntimeCommandsConfig,
    initialize: impl FnOnce(&mut World) -> Output,
) -> Output {
    debug_assert_eq!(world.remove_resource::<RuntimeCommandsInit>(), None);
    world.insert_resource(RuntimeCommandsInit(config));
    let output = initialize(world);
    world.remove_resource::<RuntimeCommandsInit>();
    output
}

/// A [`Command`] queue to perform structural changes to the [`World`].
///
/// Equivalent to Bevy's [`Commands`] system parameter; refer to it for command
/// scheduling and deferred execution documentation.
///
/// Wraps [`Commands`] to maintain wasvy's [runtime invariants](self#runtime-invariants).
/// Mod systems receive their configuration during initialization. Systems registered
/// outside wasvy use the default configuration, so they can be ported without
/// changing their parameters.
#[derive(SystemParam)]
pub struct RuntimeCommands<'w, 's> {
    commands: Commands<'w, 's>,
    config: Local<'s, RuntimeCommandsInit>,
}

/// A [`World`] mutation.
///
/// Should be used with [`RuntimeCommands::queue`].
///
/// Equivalent to Bevy's [`Command`] trait, but mutations use [`RuntimeCommands`]
/// to maintain wasvy's [runtime invariants](self#runtime-invariants). See [`Command`]
/// for Bevy's command model. Closures accepting `&mut RuntimeCommands` also implement
/// this trait.
///
/// ```
/// use wasvy_runtime::prelude::*;
///
/// fn spawn_entity(mut commands: RuntimeCommands) {
///     commands.queue(|commands: &mut RuntimeCommands| {
///         commands.spawn_empty();
///     });
/// }
/// ```
pub trait RuntimeCommand: Send + 'static {
    /// Applies this command, causing it to queue mutations through `commands`.
    ///
    /// See [`Command::apply`] for Bevy's command application model.
    fn apply(self, commands: &mut RuntimeCommands<'_, '_>);
}

impl<F> RuntimeCommand for F
where
    F: FnOnce(&mut RuntimeCommands<'_, '_>) + Send + 'static,
{
    fn apply(self, commands: &mut RuntimeCommands<'_, '_>) {
        self(commands);
    }
}

impl RuntimeCommands<'_, '_> {
    /// Spawns a new empty entity and returns its [`RuntimeEntityCommands`].
    ///
    /// See [`Commands::spawn_empty`] for Bevy's entity reservation behavior.
    /// Maintains the [runtime invariants](self#runtime-invariants).
    pub fn spawn_empty(&mut self) -> RuntimeEntityCommands<'_> {
        self.spawn(())
    }

    /// Spawns a new entity with the given components and returns its [`RuntimeEntityCommands`].
    ///
    /// See [`Commands::spawn`] for Bevy's bundle and entity reservation behavior.
    /// Maintains the [runtime invariants](self#runtime-invariants) for the entity
    /// and its [`Children`] hierarchy.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> RuntimeEntityCommands<'_> {
        let config = self.config();
        let entity = spawn_entity(&mut self.commands, config, bundle);
        self.entity(entity)
    }

    /// Spawns multiple entities with the same combination of components.
    ///
    /// See [`Commands::spawn_batch`] for Bevy's bundle concepts. This method returns
    /// the reserved entity IDs and uses [`Self::spawn`] for each bundle to maintain
    /// the [runtime invariants](self#runtime-invariants).
    pub fn spawn_batch<B: Bundle>(&mut self, bundles: impl IntoIterator<Item = B>) -> Vec<Entity> {
        bundles
            .into_iter()
            .map(|bundle| self.spawn(bundle).id())
            .collect()
    }

    /// Spawns a new entity with the given [`Scene`] and returns its [`RuntimeEntityCommands`].
    ///
    /// See [`CommandsSceneExt::spawn_scene`](bevy_scene::CommandsSceneExt::spawn_scene)
    /// for Bevy's scene spawning behavior and [`EntityWorldMutSceneExt::apply_scene`]
    /// for scene application behavior. The scene is resolved when commands are applied;
    /// its dependencies must already be loaded, and Bevy's
    /// [`AssetPlugin`](bevy_asset::AssetPlugin) and [`ScenePlugin`](bevy_scene::ScenePlugin)
    /// are required.
    ///
    /// Maintains the [runtime invariants](self#runtime-invariants) for the root and
    /// its [`Children`] hierarchy. Errors use Bevy's command error handler.
    pub fn spawn_scene(&mut self, scene: impl Scene) -> RuntimeEntityCommands<'_> {
        let config = self.config();
        let entity = self.spawn_empty().id();
        self.commands
            .queue(move |world: &mut World| -> bevy_ecs::error::Result {
                let result = world.entity_mut(entity).apply_scene(scene);
                mark_tree(world, entity, config);
                result?;
                Ok(())
            });
        self.entity(entity)
    }

    /// Returns the [`RuntimeEntityCommands`] for the requested [`Entity`].
    ///
    /// See [`Commands::entity`] for Bevy's entity access and error behavior.
    pub fn entity(&mut self, entity: Entity) -> RuntimeEntityCommands<'_> {
        RuntimeEntityCommands {
            config: self.config(),
            commands: self.commands.reborrow(),
            entity,
        }
    }

    /// Inserts a [`Resource`] into the [`World`] with a specific value.
    ///
    /// See [`Commands::insert_resource`] for Bevy's resource insertion behavior.
    pub fn insert_resource<R: Resource>(&mut self, resource: R) {
        self.commands.insert_resource(resource);
    }

    /// Initializes a [`Resource`] using [`FromWorld`] if it does not exist.
    ///
    /// See [`Commands::init_resource`] for Bevy's [`FromWorld`] initialization behavior.
    pub fn init_resource<R: Resource + FromWorld>(&mut self) {
        self.commands.init_resource::<R>();
    }

    /// Removes a [`Resource`] from the [`World`].
    ///
    /// See [`Commands::remove_resource`] for Bevy's resource removal behavior.
    pub fn remove_resource<R: Resource>(&mut self) {
        self.commands.remove_resource::<R>();
    }

    /// Triggers the given [`Event`], running any observers watching for it.
    ///
    /// See [`Commands::trigger`] for Bevy's event and observer behavior.
    pub fn trigger<'a>(&mut self, event: impl Event<Trigger<'a>: Default>) {
        self.commands.trigger(event);
    }

    /// Writes a [`Message`] to the world's message buffer.
    ///
    /// See [`Commands::write_message`] for Bevy's message registration and delivery behavior.
    pub fn write_message<M: Message>(&mut self, message: M) {
        self.commands.write_message(message);
    }

    /// Queues a [`RuntimeCommand`] to be executed, like a closure or custom command.
    ///
    /// See [`Commands::queue`] for Bevy's command queue behavior. The command and
    /// any nested runtime commands inherit this runtime's configuration.
    pub fn queue(&mut self, command: impl RuntimeCommand) {
        self.commands
            .queue(into_bevy_command(command, self.config()));
    }

    fn config(&self) -> RuntimeCommandsConfig {
        self.config.0
    }
}

/// A queue of commands that target a specific entity.
///
/// Equivalent to Bevy's [`EntityCommands`]; refer to it for entity
/// mutation documentation. Runtime operations maintain the
/// [runtime invariants](self#runtime-invariants) without exposing
/// the underlying Bevy command queue.
///
/// Obtain this queue from [`RuntimeCommands::entity`], [`RuntimeCommands::spawn`],
/// [`RuntimeCommands::spawn_empty`], or [`RuntimeCommands::spawn_scene`].
pub struct RuntimeEntityCommands<'a> {
    commands: Commands<'a, 'a>,
    entity: Entity,
    config: RuntimeCommandsConfig,
}

impl RuntimeEntityCommands<'_> {
    /// Returns the [`Entity`] identifier of the entity.
    ///
    /// See [`EntityCommands::id`] for Bevy's entity identifier documentation.
    pub fn id(&self) -> Entity {
        self.entity
    }

    /// Inserts a [`Bundle`] of components into the entity.
    ///
    /// See [`EntityCommands::insert`] for Bevy's insertion and error behavior.
    /// To spawn descendants following the [runtime invariants](self#runtime-invariants),
    /// use [`Self::with_child`] or pass the hierarchy to [`RuntimeCommands::spawn`].
    pub fn insert<B: Bundle>(&mut self, bundle: B) -> &mut Self {
        self.commands.entity(self.entity).insert(bundle);
        self
    }

    /// Spawns a child of this entity with the given components.
    ///
    /// See [`EntityCommands::with_child`] for Bevy's child spawning behavior.
    /// Maintains the [runtime invariants](self#runtime-invariants) for the child
    /// and its [`Children`] hierarchy under this entity.
    pub fn with_child<B: Bundle>(&mut self, bundle: B) -> &mut Self {
        let config = RuntimeCommandsConfig {
            parent: Some(self.entity),
            ..self.config
        };
        spawn_entity(&mut self.commands, config, bundle);
        self
    }

    /// Removes a [`Bundle`] of components from the entity.
    ///
    /// See [`EntityCommands::remove`] for Bevy's removal and error behavior.
    pub fn remove<B: Bundle>(&mut self) -> &mut Self {
        self.commands.entity(self.entity).remove::<B>();
        self
    }

    /// Despawns the entity.
    ///
    /// See [`EntityCommands::despawn`] for Bevy's recursive despawning and error behavior.
    pub fn despawn(&mut self) {
        self.commands.entity(self.entity).despawn();
    }

    /// Despawns the entity if it exists.
    ///
    /// See [`EntityCommands::try_despawn`] for Bevy's recursive despawning behavior.
    pub fn try_despawn(&mut self) {
        self.commands.entity(self.entity).try_despawn();
    }

    /// Inserts a reflected component into the entity.
    ///
    /// See [`ReflectCommandExt::insert_reflect`] for Bevy's type registry requirements
    /// and reflection behavior.
    pub fn insert_reflect(&mut self, component: Box<dyn PartialReflect>) -> &mut Self {
        self.commands.entity(self.entity).insert_reflect(component);
        self
    }

    /// Removes a reflected component from the entity using its type path.
    ///
    /// See [`ReflectCommandExt::remove_reflect`] for Bevy's type registry requirements
    /// and reflection behavior.
    pub fn remove_reflect(&mut self, type_path: String) -> &mut Self {
        self.commands.entity(self.entity).remove_reflect(type_path);
        self
    }

    /// Removes a dynamic [`Component`] from the entity if it exists.
    ///
    /// See [`EntityCommands::remove_by_id`] for Bevy's component ID requirements
    /// and error behavior.
    pub fn remove_by_id(&mut self, component: ComponentId) -> &mut Self {
        self.commands.entity(self.entity).remove_by_id(component);
        self
    }

    /// Queues insertion of a serialized guest component identified by its type path.
    pub(crate) fn insert_guest_component(&mut self, type_path: String, value: Vec<u8>) {
        let entity = self.entity;
        self.commands.queue(move |world: &mut World| {
            crate::component::insert_guest_component(world, entity, type_path, value);
        });
    }
}

fn into_bevy_command(
    command: impl RuntimeCommand,
    config: RuntimeCommandsConfig,
) -> impl Command<Out = ()> {
    move |world: &mut World| {
        let mut state = with_runtime_commands_config(world, config, |world| {
            SystemState::<RuntimeCommands>::new(world)
        });
        command.apply(&mut state.get_mut(world).expect("valid runtime commands"));
        state.apply(world);
    }
}

fn spawn_entity(
    commands: &mut Commands,
    config: RuntimeCommandsConfig,
    bundle: impl Bundle,
) -> Entity {
    let entity = commands.spawn_empty().id();
    commands.queue(move |world: &mut World| {
        mark_entity(&mut world.entity_mut(entity), config, true);
        world.entity_mut(entity).insert(bundle);
        mark_tree(world, entity, config);
    });
    entity
}

fn mark_entity(entity: &mut EntityWorldMut, config: RuntimeCommandsConfig, attach_parent: bool) {
    if attach_parent
        && let Some(sandbox) = config.parent
        && entity.get::<ChildOf>().map(ChildOf::parent) != Some(sandbox)
    {
        entity.insert(ChildOf(sandbox));
    }
    if let Some(owner) = config.owner
        && entity.get::<DespawnModEntity>().map(|marker| marker.0) != Some(owner)
    {
        entity.insert(DespawnModEntity::new(owner));
    }
}

fn mark_tree(world: &mut World, root: Entity, config: RuntimeCommandsConfig) {
    let mut pending = vec![root];
    while let Some(entity) = pending.pop() {
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
        mark_entity(&mut world.entity_mut(entity), config, entity == root);
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::{
        relationship::Relationship,
        system::{ParamBuilder, SystemParamBuilder},
    };

    use super::*;
    use crate::{
        cleanup::{DespawnModEntities, DespawnModEntity},
        prelude::{ModAccess, ModDespawnBehaviour, ModSchedules, Sandbox},
        sandbox::Sandboxed,
    };
    use bevy_scene::bsn;

    fn world() -> (World, Entity, Entity) {
        let mut world = World::new();
        world.register_component::<Sandboxed>();
        let owner = world.spawn_empty().id();
        let sandbox = Sandbox::new(&mut world, ModSchedules::empty());
        let sandbox = world.spawn(sandbox).id();
        (world, owner, sandbox)
    }

    fn run(world: &mut World, owner: Entity, access: ModAccess, command: impl RuntimeCommand) {
        let config = RuntimeCommandsConfig::new(world, owner, access);
        let mut state = with_runtime_commands_config(world, config, |world| {
            SystemState::<RuntimeCommands>::new(world)
        });
        command.apply(&mut state.get_mut(world).unwrap());
        state.apply(world);
    }

    fn owned(world: &World, owner: Entity) -> Vec<Entity> {
        world
            .get::<DespawnModEntities>(owner)
            .unwrap()
            .iter()
            .collect()
    }

    #[test]
    fn default_config() {
        #[derive(Component)]
        struct Spawned;

        let mut world = World::new();
        let mut system = IntoSystem::into_system(|mut commands: RuntimeCommands| {
            commands.spawn(Spawned);
            commands.queue(|commands: &mut RuntimeCommands| {
                commands.spawn(Spawned);
            });
        });
        system.initialize(&mut world);
        system.run((), &mut world).unwrap();
        system.apply_deferred(&mut world);

        assert_eq!(world.query::<&Spawned>().iter(&world).count(), 2);
        assert_eq!(world.query::<&ChildOf>().iter(&world).count(), 0);
        assert_eq!(world.query::<&DespawnModEntity>().iter(&world).count(), 0);
    }

    #[test]
    fn dynamic_config() {
        let (mut world, owner, sandbox) = world();
        let config = RuntimeCommandsConfig::new(&world, owner, ModAccess::Sandbox(sandbox));
        let mut system = with_runtime_commands_config(&mut world, config, |world| {
            (ParamBuilder::of::<RuntimeCommands>(),)
                .build_state(world)
                .build_system(|mut commands: RuntimeCommands| {
                    commands.spawn_empty();
                })
        });
        system.initialize(&mut world);
        for _ in 0..2 {
            system.run((), &mut world).unwrap();
            system.apply_deferred(&mut world);
        }
        let entities = owned(&world, owner);
        assert_eq!(entities.len(), 2);
        for entity in entities {
            assert_eq!(world.get::<ChildOf>(entity).unwrap().parent(), sandbox);
            assert_eq!(world.get::<Sandboxed>(entity).unwrap().get(), sandbox);
        }
    }

    #[test]
    fn world_spawn() {
        let (mut world, owner, _) = world();
        run(
            &mut world,
            owner,
            ModAccess::World,
            |commands: &mut RuntimeCommands| {
                commands.spawn_empty();
            },
        );
        let entities = owned(&world, owner);
        assert_eq!(entities.len(), 1);
        assert!(world.get::<ChildOf>(entities[0]).is_none());
    }

    #[test]
    fn cleanup_disabled() {
        let (mut world, owner, sandbox) = world();
        world.insert_resource(ModDespawnBehaviour::None);
        run(
            &mut world,
            owner,
            ModAccess::Sandbox(sandbox),
            |commands: &mut RuntimeCommands| {
                commands.spawn_empty().insert(Name::new("persistent"));
            },
        );
        let entity = world
            .query::<(Entity, &Name)>()
            .iter(&world)
            .find(|(_, name)| name.as_str() == "persistent")
            .unwrap()
            .0;
        assert_eq!(world.get::<ChildOf>(entity).unwrap().parent(), sandbox);
        assert!(world.get::<DespawnModEntity>(entity).is_none());
        world.despawn(owner);
        assert!(world.get_entity(entity).is_ok());
    }

    #[derive(Resource, Default, PartialEq, Debug)]
    struct Count(usize);

    struct SpawnPair;

    impl RuntimeCommand for SpawnPair {
        fn apply(self, commands: &mut RuntimeCommands) {
            commands.spawn_batch([Name::new("one"), Name::new("two")]);
            commands.insert_resource(Count(2));
        }
    }

    #[test]
    fn nested_commands() {
        let (mut world, owner, sandbox) = world();
        run(
            &mut world,
            owner,
            ModAccess::Sandbox(sandbox),
            |commands: &mut RuntimeCommands| {
                commands.init_resource::<Count>();
                commands.queue(SpawnPair);
                commands.queue(|commands: &mut RuntimeCommands| {
                    commands.queue(|commands: &mut RuntimeCommands| {
                        commands.spawn_empty();
                    });
                });
            },
        );
        assert_eq!(world.resource::<Count>(), &Count(2));
        assert_eq!(owned(&world, owner).len(), 3);
        for entity in owned(&world, owner) {
            assert_eq!(world.get::<ChildOf>(entity).unwrap().parent(), sandbox);
            assert_eq!(world.get::<Sandboxed>(entity).unwrap().get(), sandbox);
        }
    }

    #[test]
    fn existing_entity_scope() {
        for already_owned in [false, true] {
            let (mut world, owner, sandbox) = world();
            let original_parent = world.spawn_empty().id();
            let original_owner = world.spawn_empty().id();
            let existing = world.spawn(ChildOf(original_parent)).id();
            if already_owned {
                world
                    .entity_mut(existing)
                    .insert(DespawnModEntity::new(original_owner));
            }
            run(
                &mut world,
                owner,
                ModAccess::Sandbox(sandbox),
                move |commands: &mut RuntimeCommands| {
                    commands.entity(existing).insert(Name::new("changed"));
                },
            );
            assert!(world.get::<Name>(existing).is_some());
            assert_eq!(
                world.get::<ChildOf>(existing).unwrap().parent(),
                original_parent
            );
            assert_eq!(
                world
                    .get::<DespawnModEntity>(existing)
                    .map(|marker| marker.0),
                already_owned.then_some(original_owner)
            );
            assert!(world.get::<Sandboxed>(existing).is_none());
        }
    }

    #[test]
    fn child_ownership() {
        for bundled in [false, true] {
            let (mut world, owner, sandbox) = world();
            run(
                &mut world,
                owner,
                ModAccess::Sandbox(sandbox),
                move |commands: &mut RuntimeCommands| {
                    if bundled {
                        commands.spawn((Name::new("root"), children![Name::new("child")]));
                    } else {
                        commands
                            .spawn(Name::new("root"))
                            .with_child(Name::new("child"));
                    }
                },
            );
            let entities = owned(&world, owner);
            assert_eq!(entities.len(), 2);
            let root = entities
                .iter()
                .copied()
                .find(|entity| world.get::<Name>(*entity).unwrap().as_str() == "root")
                .unwrap();
            assert_eq!(world.get::<ChildOf>(root).unwrap().parent(), sandbox);
            let children = world.get::<Children>(root).unwrap();
            assert_eq!(children.len(), 1);
            assert_eq!(world.get::<Name>(children[0]).unwrap().as_str(), "child");
            assert_eq!(world.get::<ChildOf>(children[0]).unwrap().parent(), root);
            assert!(entities.contains(&children[0]));
            for entity in entities {
                assert_eq!(world.get::<Sandboxed>(entity).unwrap().get(), sandbox);
            }
        }
    }

    #[test]
    fn scene_ownership() {
        let (mut world, owner, sandbox) = world();
        let mut app = bevy_app::App::new();
        app.add_plugins((
            bevy_app::TaskPoolPlugin::default(),
            bevy_asset::AssetPlugin::default(),
            bevy_scene::ScenePlugin,
        ));
        world.insert_resource(app.world().resource::<bevy_asset::AssetServer>().clone());
        world.init_resource::<bevy_asset::Assets<bevy_scene::ScenePatch>>();
        run(
            &mut world,
            owner,
            ModAccess::Sandbox(sandbox),
            |commands: &mut RuntimeCommands| {
                commands.spawn_scene(bsn! {
                    Name::new("root")
                    Children [
                        Name::new("child")
                        Children [Name::new("grandchild")]
                    ]
                });
            },
        );
        let entities = owned(&world, owner);
        assert_eq!(entities.len(), 3);
        for entity in &entities {
            assert_eq!(world.get::<Sandboxed>(*entity).unwrap().get(), sandbox);
        }
        let root = entities
            .iter()
            .copied()
            .find(|entity| world.get::<Name>(*entity).unwrap().as_str() == "root")
            .unwrap();
        let child = world.get::<Children>(root).unwrap()[0];
        let grandchild = world.get::<Children>(child).unwrap()[0];
        assert_eq!(world.get::<ChildOf>(root).unwrap().parent(), sandbox);
        assert_eq!(world.get::<ChildOf>(grandchild).unwrap().parent(), child);
        world.despawn(owner);
        for entity in entities {
            assert!(world.get_entity(entity).is_err());
        }
    }

    #[test]
    fn native_config() {
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let config = RuntimeCommandsConfig::new(&world, owner, ModAccess::World);
        let mut system = IntoSystem::into_system(|mut commands: RuntimeCommands| {
            commands.spawn_empty();
        });
        with_runtime_commands_config(&mut world, config, |world| {
            system.initialize(world);
        });
        assert!(!world.contains_resource::<RuntimeCommandsInit>());
        world.insert_resource(ModDespawnBehaviour::None);

        for _ in 0..2 {
            system.run((), &mut world).unwrap();
            system.apply_deferred(&mut world);
        }
        assert_eq!(world.get::<DespawnModEntities>(owner).unwrap().len(), 2);
    }

    #[test]
    fn init_error_cleanup() {
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let config = RuntimeCommandsConfig::new(&world, owner, ModAccess::World);

        let result = with_runtime_commands_config(&mut world, config, |world| {
            assert_eq!(RuntimeCommandsInit::from_world(world).0.owner, Some(owner));
            Err::<(), _>("build failed")
        });
        assert!(result.is_err());
        assert!(!world.contains_resource::<RuntimeCommandsInit>());
        let mut system = IntoSystem::into_system(|mut commands: RuntimeCommands| {
            commands.spawn_empty();
        });
        system.initialize(&mut world);
        let count = world.entities().count_spawned();
        system.run((), &mut world).unwrap();
        system.apply_deferred(&mut world);
        assert_eq!(world.entities().count_spawned(), count + 1);
        assert_eq!(world.query::<&DespawnModEntity>().iter(&world).count(), 0);
    }
}
