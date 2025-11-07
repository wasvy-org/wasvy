use std::alloc::Layout;

use bevy::{
    ecs::{
        component::{ComponentCloneBehavior, ComponentDescriptor, ComponentId, StorageType},
        lifecycle::HookContext,
        query::FilteredAccess,
        relationship::Relationship,
        world::{DeferredWorld, WorldId},
    },
    prelude::*,
};

use crate::schedule::ModSchedules;

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
/// ## Global Sandbox
///
/// During plugin instantiation, a sandbox is created that represents access to the entire world (except for other sandboxes).
///
/// This Sandbox should not be removed.
#[derive(Component)]
#[component(clone_behavior = Ignore, immutable)]
#[component(on_add = Self::on_add, on_insert = Self::on_insert, on_replace = Self::on_replace, on_remove = Self::on_remove)]
pub struct Sandbox {
    /// Responsible for tagging all [Sandboxed] entities as belonging to this Sandbox
    ///
    /// Markers have no data, they are just used for query filtering.
    ///
    /// If this is [None], then that indicates this is the [Sandbox] for the world, so for all entities not already in a sandbox.
    component_id: Option<ComponentId>,

    /// The world this Sandbox belongs to
    world_id: WorldId,

    /// The component id for [Sandboxed] in the world the sandbox was created
    sandboxed_component_id: ComponentId,

    /// Mods in this sandbox will run only during the provided schedules
    schedules: ModSchedules,
}

impl Sandbox {
    /// Creates a new [Sandbox] component
    ///
    /// Mods in this Sandbox will run only during the provided [Schedules]
    pub fn new(world: &mut World, schedules: ModSchedules) -> Self {
        let count = world.get_resource_or_init::<SandboxCount>().0;
        let name = format!("SandboxMarker{count}");

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
            )
        };

        Self::new_inner(world, schedules, Some(descriptor))
    }

    /// Returns true if this is the "Global Sandbox." See [Sandbox] docs for more details.
    pub fn is_global(&self) -> bool {
        self.component_id.is_none()
    }

    /// Returns the Schedules for
    pub fn schedules(&self) -> &ModSchedules {
        &self.schedules
    }

    /// Returns access to only the entities within this sandbox.
    ///
    /// This is used by Wasvy to build mod systems that run exclusively in these sandboxes.
    pub fn access(&self) -> FilteredAccess {
        let mut access = FilteredAccess::default();
        if let Some(component_id) = self.component_id {
            // Access to Sandboxed, and this Sandbox specifically
            access.and_with(self.sandboxed_component_id);
            access.and_with(component_id);
        } else {
            // World access, without sandboxed
            access.and_without(self.sandboxed_component_id);
        }
        access
    }

    /// Retrieves the system set for this Sandbox
    pub fn system_set(&self) -> SandboxSet {
        SandboxSet(self.component_id.map(|id| id.index()))
    }

    /// Add the Global Sandbox, see [Self]
    pub(crate) fn spawn_global(world: &mut World, schedules: ModSchedules) {
        let component = Sandbox::new_inner(world, schedules, None);
        world.spawn((component, Name::new("Global Sandbox")));
    }

    fn new_inner(
        world: &mut World,
        schedules: ModSchedules,
        descriptor: Option<ComponentDescriptor>,
    ) -> Self {
        let component_id =
            descriptor.map(|descriptor| world.register_component_with_descriptor(descriptor));

        let world_id = world.id();
        let sandboxed_component_id = world.register_component::<Sandboxed>();

        Self {
            component_id,
            world_id,
            sandboxed_component_id,
            schedules,
        }
    }

    /// [On add](bevy::ecs::lifecycle::ComponentHooks::on_add) for [Sandbox]
    fn on_add(mut world: DeferredWorld, ctx: HookContext) {
        world.commands().queue(move |world: &mut World| {
            // Get and also increment the count
            let count = world.get_resource_or_init::<SandboxCount>().0;
            world.get_resource_mut::<SandboxCount>().expect("init").0 = count + 1;

            // Once a custom Sandbox is added, activate the propagation
            // This is not needed when just the "Global Sandbox" is present
            if count == 1 {
                world.add_observer(Sandboxed::propagate);
            }

            // Add a name for debug usage
            world
                .entity_mut(ctx.entity)
                .insert_if_new(Name::new(format!("Sandbox {count}")));
        });
    }

    /// [On insert](bevy::ecs::lifecycle::ComponentHooks::on_insert) for [Sandbox]
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

    /// [On replace](bevy::ecs::lifecycle::ComponentHooks::on_replace) for [Sandbox]
    fn on_replace(mut world: DeferredWorld, ctx: HookContext) {
        let Self { component_id, .. } = world
            .entity(ctx.entity)
            .get()
            .expect("Sandbox was replaced");
        let component_id = component_id.expect("Global Sandbox to never be removed");

        let Some(SandboxedEntities(entities)) = world.entity(ctx.entity).get() else {
            return;
        };

        // Make sure we remove the old, invalid marker component for all the sandboxed entites
        for entity in entities.clone() {
            world.commands().entity(entity).remove_by_id(component_id);
        }
    }

    /// [On remove](bevy::ecs::lifecycle::ComponentHooks::on_remove) for [Sandbox]
    fn on_remove(mut world: DeferredWorld, ctx: HookContext) {
        // A SandboxedEntities and Sandboxed cannot exist without a Sandbox
        world
            .commands()
            .entity(ctx.entity)
            .remove::<SandboxedEntities>();
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
pub struct SandboxedEntities(Vec<Entity>);

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

    /// [On insert](bevy::ecs::lifecycle::ComponentHooks::on_insert) for [Sandboxed]
    fn on_insert(mut world: DeferredWorld, ctx: HookContext) {
        let Self(sandbox) = world.entity(ctx.entity).get().expect("Component was added");

        if let Some(Sandbox {
            component_id: Some(component_id),
            ..
        }) = world.entity(*sandbox).get()
        {
            let component_id = *component_id;

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

    /// [On replace](bevy::ecs::lifecycle::ComponentHooks::on_replace) for [Sandboxed]
    fn on_replace(mut world: DeferredWorld, ctx: HookContext) {
        let Self(sandbox) = world.entity(ctx.entity).get().expect("Component was added");

        // Might be none if the Sandbox was deleted
        // In that case, the marker component was already removed by Sandbox::on_replace
        if let Some(Sandbox {
            component_id: Some(component_id),
            ..
        }) = world.entity(*sandbox).get()
        {
            let component_id = *component_id;

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

/// A unique set containing all the systems for a specific Sandbox
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub struct SandboxSet(Option<usize>);

/// Tracks the Count of [Sandboxes]
#[derive(Resource, Default)]
struct SandboxCount(pub usize);

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::schedule::ModSchedules;

    fn setup() -> World {
        let mut world = World::new();
        Sandbox::spawn_global(&mut world, ModSchedules::empty());
        world
    }

    #[test]
    fn sandboxed_propagate_marker() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker = component.component_id.unwrap().clone();
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

        assert_eq!(
            world.entity(sandbox).get(),
            Some(&SandboxedEntities(vec![child, nested_child])),
            "All Children have the sandbox relation and were added to the SandboxedEntities"
        );
    }

    #[test]
    fn reparent_sandboxed() {
        let mut world = setup();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker1 = component.component_id.unwrap();
        let sandbox1 = world.spawn(component).id();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker2 = component.component_id.unwrap();
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
        let marker1 = component.component_id.unwrap();
        let sandbox = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        let component = Sandbox::new(&mut world, ModSchedules::empty());
        let marker2 = component.component_id.unwrap();
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
        let marker = component.component_id.unwrap();
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
