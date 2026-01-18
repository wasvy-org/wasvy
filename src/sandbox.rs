use std::alloc::Layout;

use bevy_ecs::{
    component::{ComponentCloneBehavior, ComponentDescriptor, ComponentId, StorageType},
    entity::EntityHashSet,
    lifecycle::HookContext,
    prelude::*,
    query::FilteredAccess,
    relationship::Relationship,
    world::{DeferredWorld, WorldId},
};

use crate::{cleanup::DisableSystemSet, mods::ModSystemSet, schedule::ModSchedules};

/// Sandboxes are subsets of entities within a bevy [World] in which [Mods](crate::mods::Mod) can run exclusively.
///
/// This means that systems belonging to mods configured to run in this Sandbox will have access to:
/// - All the entities within this Sandbox
/// - No entities outside this Sandbox
/// - No entities that are in another Sandbox, even if the Sandbox is nested in this one
///
/// A neat feature of sandboxes is that since systems of one sandbox do not conflict with those in another, bevy can run them in parallel.
///
/// ## Security and Isolation
///
/// **Loading a mod via a sandbox does not provide additional security!** Mods might have access to dangerous wasi apis (such as file io), and that doesn't change within a sandbox.
/// The goal of sandboxes is simply to restrict mod access to certain entities in the world. There are no strong security guarantees.
///
/// Note: Sandboxed mods can technically affect entities outside the sandbox via relations!
/// No guards are in place to prevent mods from creating components that reference entities outside their sandbox. Thus component hooks can mutate a component on an entity within a sandbox when a mod in a different sandbox mutates a component.
///
/// The intention is that an upcoming permissions system will solve this issue, giving fine-tuned access on what components mods can read from or mutate.
///
/// ## Example
///
/// ```ignore
/// # use bevy_ecs::prelude::*;
/// # use bevy_app::prelude::*;
/// # use wasvy::prelude::*;
/// # let mut app = App::new();
/// app.init_resource::<Sandboxes>();
/// app.add_systems(Startup, (load_mods, setup));
/// app.add_systems(PreUpdate, || info!("pre update"));
/// app.add_systems(PostUpdate, || info!("post update"));
///
/// /// A set of 3 "environments" for running our mods
/// #[derive(Resource)]
/// struct Sandboxes{
///     sandbox_rust: Entity,
///     sandbox_python: Entity,
///     sandbox_all: Entity,
/// }
///
/// impl FromWorld for Sandboxes {
///     fn from_world(world: &mut World) -> Self {
///         let sandbox = Sandbox::new(world, ModSchedules::default());
///         let sandbox_all = world.spawn(sandbox).id();
///
///         let sandbox = Sandbox::new(world, ModSchedules::default());
///         let sandbox_python = world.spawn(sandbox).id();
///
///         let sandbox = Sandbox::new(world, ModSchedules::default());
///         let sandbox_rust = world.spawn(sandbox).id();
///
///         Self {
///             sandbox_all,
///             sandbox_python,
///             sandbox_rust,
///         }
///     }
/// }
///
/// /// Use the Mods SystemParam to alter a mod's access
/// fn load_mods(mut mods: Mods, sandboxes: Res<Sandboxes>) {
///     let simple = mods.spawn("mods/simple.wasm");
///     mods.enable_access(simple, ModAccess::Sandbox(sandboxes.sandbox_rust));
///     mods.enable_access(simple, ModAccess::Sandbox(sandboxes.sandbox_all));
///
///     let python = mods.spawn("mods/python.wasm");
///     mods.enable_access(python, ModAccess::Sandbox(sandboxes.sandbox_python));
///     mods.enable_access(python, ModAccess::Sandbox(sandboxes.sandbox_all));
/// }
///
/// /// A marker component so mods can find the cube
/// #[derive(Component, Reflect)]
/// struct MyMarker;
///
/// /// Setup the scene and the 3 separate environments
/// fn setup(
///     mut commands: Commands,
///     sandboxes: Res<Sandboxes>,
///     mut meshes: ResMut<Assets<Mesh>>,
///     mut materials: ResMut<Assets<StandardMaterial>>,
/// ) {
///     commands.spawn((
///         PointLight {
///             shadows_enabled: true,
///             ..default()
///         },
///         Transform::from_xyz(4.0, 8.0, 4.0),
///     ));
///
///     commands.spawn((
///         Camera3d::default(),
///         Transform::from_xyz(-2.5, 3.5, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
///     ));
///
///     /// Spawn a uniquely colored cube in each sandbox
///     let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
///     for (color, transform, sandbox) in [
///         (
///             Color::srgb_u8(124, 144, 255),
///             Transform::from_xyz(-2.0, 0.0, 0.0),
///             sandboxes.sandbox_all,
///         ),
///         (
///             Color::srgb_u8(255, 124, 144),
///             Transform::default(),
///             sandboxes.sandbox_python,
///         ),
///         (
///             Color::srgb_u8(144, 255, 124),
///             Transform::from_xyz(2.0, 0.0, 0.0),
///             sandboxes.sandbox_rust,
///         ),
///     ] {
///         commands.entity(sandbox).insert(transform);
///
///         let material = materials.add(color);
///         commands.spawn((
///             Name::new("My cube"),
///             MyMarker,
///             Mesh3d(mesh.clone()),
///             MeshMaterial3d(material),
///             ChildOf(sandbox),
///             Transform::default(),
///         ));
///     }
/// }
/// ```
#[derive(Component)]
#[require(Name::new("Sandbox"))]
#[component(clone_behavior = Ignore, immutable)]
#[component(on_insert = Self::on_insert, on_replace = Self::on_replace, on_remove = Self::on_remove, on_despawn = Self::on_despawn)]
pub struct Sandbox {
    /// Responsible for tagging all [Sandboxed] entities as belonging to this Sandbox
    ///
    /// Markers have no data, they are just used for query filtering.
    ///
    /// If this is [None], then that indicates this is the [Sandbox] for the world, so for all entities not already in a sandbox.
    component_id: ComponentId,

    /// Filtered access just to the entities in this sandbox
    access: FilteredAccess,

    /// Mods in this sandbox will run only during the provided schedules
    schedules: ModSchedules,

    /// The world this Sandbox belongs to
    world_id: WorldId,
}

impl Sandbox {
    /// Creates a new [Sandbox] component
    ///
    /// Mods in this Sandbox will run only during the provided [ModSchedules]
    pub fn new(world: &mut World, schedules: ModSchedules) -> Self {
        // Get and also increment the count
        let sandbox_count = world.get_resource_or_insert_with(|| SandboxCount(1));
        let count = sandbox_count.0;
        sandbox_count.into_inner().0 += 1;

        // Activate the propagation when the very first Sandbox is added to the world
        if count == 1 {
            world.add_observer(Sandboxed::propagate);
        }

        let name = format!("Sandbox{count}");

        // When an entity is cloned from one Sandbox to another, the Marker should never be copied with it
        // Instead, if this entity is cloned into another sandbox, we'll rely on propagation to add the new component
        let clone_behavior = ComponentCloneBehavior::Ignore;

        // Safety:
        // - the drop fn is usable on this component type
        // - the component is safe to access from any thread
        let descriptor = unsafe {
            ComponentDescriptor::new_with_layout(
                name,
                StorageType::default(),
                Layout::new::<SandboxedMarker>(),
                None,
                false,
                clone_behavior,
                None,
            )
        };
        let component_id = world.register_component_with_descriptor(descriptor);

        let access = Self::generate_access(component_id, world);

        let world_id = world.id();

        Self {
            component_id,
            access,
            world_id,
            schedules,
        }
    }

    /// Returns the Schedules for
    pub fn schedules(&self) -> &ModSchedules {
        &self.schedules
    }

    /// Returns access to only the entities within this sandbox.
    ///
    /// This is used by Wasvy to build mod systems that run exclusively in these sandboxes.
    pub fn access(&self) -> &FilteredAccess {
        &self.access
    }

    /// Access to non-sandboxed entities
    ///
    /// This is used by Wasvy to build mod systems that run exclusively in the world
    pub fn access_non_sandboxed(world: &World) -> FilteredAccess {
        let mut access = FilteredAccess::default();

        // Avoid conflicting with sandboxes
        access.and_without(
            world
                .components()
                .component_id::<Sandboxed>()
                .expect("Sandboxed be registered"),
        );

        access
    }

    fn generate_access(component_id: ComponentId, world: &mut World) -> FilteredAccess {
        let mut access = FilteredAccess::default();

        // Require the unique marker component
        access.and_with(component_id);

        // Avoid conflicting with world systems
        access.and_with(
            world
                .components()
                .component_id::<Sandboxed>()
                .expect("Sandboxed be registered"),
        );

        // Avoid conflicting with all present sandboxes
        for other_sandbox in world
            .query::<&Sandbox>()
            .iter(&world)
            .filter(|sandbox| sandbox.component_id != component_id)
        {
            access.and_without(other_sandbox.component_id);
        }

        access
    }

    /// [On insert](bevy_ecs::lifecycle::ComponentHooks::on_insert) for [Sandbox]
    fn on_insert(mut world: DeferredWorld, ctx: HookContext) {
        let Self { world_id, .. } = world
            .entity(ctx.entity)
            .get()
            .expect("Sandbox was inserted");

        if world.id() != *world_id {
            panic!("Sandbox was created from one world, but spawned in another");
        }

        // Ensure all children are sandboxed
        Sandboxed::add_children(ctx.entity, ctx.entity, &mut world);
    }

    /// [On replace](bevy_ecs::lifecycle::ComponentHooks::on_replace) for [Sandbox]
    fn on_replace(mut world: DeferredWorld, ctx: HookContext) {
        let component_id = world
            .entity(ctx.entity)
            .get::<Self>()
            .expect("Sandbox was replaced")
            .component_id;

        if let Some(SandboxedEntities(entities)) = world.entity(ctx.entity).get() {
            // Make sure we remove the old, invalid marker component for all the sandboxed entites
            for entity in entities.clone() {
                world.commands().entity(entity).remove_by_id(component_id);
            }
        }
    }

    /// [On remove](bevy_ecs::lifecycle::ComponentHooks::on_remove) for [Sandbox]
    fn on_remove(mut world: DeferredWorld, ctx: HookContext) {
        // A SandboxedEntities and Sandboxed cannot exist without a Sandbox
        world
            .commands()
            .entity(ctx.entity)
            .remove::<SandboxedEntities>();
    }

    /// [On despawn](bevy_ecs::lifecycle::ComponentHooks::on_despawn) for [Sandbox]
    fn on_despawn(mut world: DeferredWorld, ctx: HookContext) {
        let schedules = world
            .entity(ctx.entity)
            .get::<Self>()
            .expect("Sandbox was replaced")
            .schedules()
            .clone();

        // After a sandbox is removed, its systems should no longer run
        world.commands().queue(DisableSystemSet {
            set: ModSystemSet::new_sandboxed(ctx.entity),
            schedules,
        });
    }
}

/// A component holding a reference to all of a [Sandbox]'s [Sandboxed] entites
///
/// You should never initialize this component on your own. Instead create a new sandbox with [Sandbox::new].
///
/// Note regarding this implementation: Ideally Sandbox would be used for the relation, but bevy requires that:
/// - Relations have a default impl (Sandbox cannot)
/// - Relations be cloneable (It'd be incorrect to allow Sandboxes to be cloned)
#[derive(Component, Default, Debug, PartialEq, Eq)]
#[relationship_target(relationship = Sandboxed)]
pub struct SandboxedEntities(EntityHashSet);

/// An entity that belongs to a sandbox
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable, clone_behavior = Ignore)]
#[component(on_insert = Self::on_insert, on_replace = Self::on_replace)]
pub struct Sandboxed(Entity);

// Manually implement due to compile error "Custom on_insert hooks are not supported as relationships already define an on_insert hook"
impl Relationship for Sandboxed {
    type RelationshipTarget = SandboxedEntities;

    fn get(&self) -> Entity {
        self.0
    }

    fn from(entity: Entity) -> Self {
        Self(entity)
    }

    fn set_risky(&mut self, entity: Entity) {
        self.0 = entity;
    }
}

impl Sandboxed {
    /// An observer that Ensures that new children inside a [Sandbox] get the [Sandboxed] component
    fn propagate(add: On<Insert, ChildOf>, mut world: DeferredWorld) {
        let mut entity = add.entity;
        while let Some(ChildOf(parent)) = world.get(entity) {
            entity = *parent;

            // Find the first parent Sandbox
            if world.get::<Sandbox>(entity).is_some() {
                Self::add_children(add.entity, entity, &mut world);
                break;
            }
        }
    }

    /// [On insert](bevy_ecs::lifecycle::ComponentHooks::on_insert) for [Sandboxed]
    fn on_insert(mut world: DeferredWorld, ctx: HookContext) {
        let Self(sandbox) = world.entity(ctx.entity).get().expect("Component was added");

        if let Some(sandbox) = world.entity(*sandbox).get::<Sandbox>() {
            let component_id = sandbox.component_id;

            // SAFETY
            // - component_id is from the same world
            // - SandboxedMarker is the same layout
            unsafe {
                world
                    .commands()
                    .entity(ctx.entity)
                    .insert_by_id(component_id, SandboxedMarker);
            }
        } else {
            world.commands().entity(ctx.entity).remove::<Self>();
        }

        // Relationship impl
        <Self as Relationship>::on_insert(world, ctx);
    }

    /// [On replace](bevy_ecs::lifecycle::ComponentHooks::on_replace) for [Sandboxed]
    fn on_replace(mut world: DeferredWorld, ctx: HookContext) {
        let Self(sandbox) = world.entity(ctx.entity).get().expect("Component was added");

        // Might be none if the Sandbox was deleted
        // In that case, the marker component was already removed by Sandbox::on_replace
        if let Some(sandbox) = world.entity(*sandbox).get::<Sandbox>() {
            let component_id = sandbox.component_id;

            // Remove marker component
            world
                .commands()
                .entity(ctx.entity)
                .remove_by_id(component_id);
        }

        // Relationship impl
        <Self as Relationship>::on_insert(world, ctx);
    }

    /// Recursively sandbox the provided entity and its descendants
    fn add_children(entity: Entity, sandbox: Entity, world: &mut DeferredWorld) {
        // A sandbox should not be sandboxed in itself. Skip and continue with its children
        if entity != sandbox {
            world.commands().entity(entity).insert(Sandboxed(sandbox));

            // Stop recursing when another sandbox is encountered
            // The Sandbox should be sandboxed in it's parent, but not it's children (those already belong to this sandbox)
            if world.get::<Sandbox>(entity).is_some() {
                return;
            }
        }

        if let Some(children) = world.get::<Children>(entity) {
            let children: Vec<Entity> = children.iter().collect();
            for child in children {
                Self::add_children(child, sandbox, world);
            }
        }
    }
}

/// A hidden custom marker component for [Sandboxed] entities
struct SandboxedMarker;

/// Tracks the Count of [Sandboxes]
#[derive(Resource, Default)]
struct SandboxCount(pub usize);

#[cfg(test)]
mod tests {
    use bevy_ecs::{prelude::*, relationship::RelationshipSourceCollection};

    use super::*;
    use crate::schedule::ModSchedules;

    fn setup() -> World {
        let mut world = World::new();
        world.register_component::<Sandboxed>();
        world
    }

    #[test]
    fn sandboxed_propagate_marker() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker = component.component_id;
        let sandbox = world.spawn(component).id();
        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        assert!(
            world.get_by_id(sandbox, marker).is_none(),
            "Sandbox should not have a marker"
        );
        assert!(
            world.get_by_id(child, marker).is_some(),
            "Child has the SandboxMarker"
        );
    }

    #[test]
    fn simple_sandboxed_propagate() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let sandbox = world.spawn(component).id();
        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();
        let nested_child = world.spawn_empty().insert(ChildOf(child)).id();

        let mut set = EntityHashSet::new();
        set.add(child);
        set.add(nested_child);

        assert_eq!(
            world.entity(sandbox).get(),
            Some(&SandboxedEntities(set)),
            "All Children have the sandbox relation and were added to the SandboxedEntities"
        );
    }

    #[test]
    fn reparent_sandboxed() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker1 = component.component_id;
        let sandbox1 = world.spawn(component).id();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker2 = component.component_id;
        let sandbox2 = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox1)).id();
        let nested_child = world.spawn_empty().insert(ChildOf(child)).id();

        world.entity_mut(child).insert(ChildOf(sandbox2));

        assert_eq!(
            world.get(child),
            Some(&Sandboxed(sandbox2)),
            "A reparented sandboxed entity should be updated"
        );
        assert_eq!(
            world.get(nested_child),
            Some(&Sandboxed(sandbox2)),
            "The child of a reparented sandboxed entity should be updated"
        );
        assert!(
            world.get_by_id(nested_child, marker1).is_none()
                && world.get_by_id(nested_child, marker2).is_some(),
            "A reparented child should have the correct marker trait"
        );
    }

    #[test]
    fn replace_sandbox() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker1 = component.component_id;
        let sandbox = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker2 = component.component_id;
        world.entity_mut(sandbox).insert(component);

        assert!(
            world.get_by_id(child, marker1).is_none() && world.get_by_id(child, marker2).is_some(),
            "Child should have the correct marker trait"
        );
        assert!(
            world.get::<SandboxedEntities>(sandbox).is_some(),
            "SandboxedEntities should not be removed"
        );
    }

    #[test]
    fn remove_sandbox() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker = component.component_id;
        let sandbox = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        world.entity_mut(sandbox).remove::<Sandbox>();

        assert!(
            world.get::<Sandboxed>(child).is_none(),
            "A child of a removed sandbox should have no more markers"
        );
        assert!(
            world.get_by_id(child, marker).is_none(),
            "A child of a removed sandbox should have no more markers"
        );
    }

    #[test]
    fn nested_sandbox_propagate() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let sandbox1 = world.spawn(component).id();
        let child1 = world.spawn(ChildOf(sandbox1)).id();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let sandbox2 = world.spawn((component, ChildOf(child1))).id();
        let child2 = world.spawn(ChildOf(sandbox2)).id();

        assert_eq!(
            world.get(sandbox2),
            Some(&Sandboxed(sandbox1)),
            "Sandbox is sandboxed"
        );
        assert_eq!(
            world.get(child2),
            Some(&Sandboxed(sandbox2)),
            "Nested sandbox children belong to their own sandbox"
        );
    }

    #[test]
    fn panic_world_mismatch() {
        let result = std::panic::catch_unwind(move || {
            let mut world = setup();
            let mut other_world = setup();

            let component = Sandbox::new(&mut other_world, ModSchedules::empty());

            // Should panic
            world.spawn(component);
        });

        assert!(
            result.is_err(),
            "Should panic when Sandbox was created in different world"
        );
        assert_eq!(
            result.unwrap_err().downcast_ref::<&str>(),
            Some(&"Sandbox was created from one world, but spawned in another")
        );
    }
}
