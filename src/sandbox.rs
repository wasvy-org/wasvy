use std::alloc::Layout;

use bevy::{
    ecs::{
        component::{ComponentCloneBehavior, ComponentDescriptor, ComponentId, StorageType},
        lifecycle::HookContext,
        query::FilteredAccess,
        relationship::Relationship,
        world::DeferredWorld,
    },
    prelude::*,
};

pub(crate) struct SandboxPlugin;

impl Plugin for SandboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(Sandboxed::propagate);
    }
}

/// Sandboxes are subsets of entities within a bevy [World] in which [Mods](crate::mods::Mod) can run exclusively.
///
/// This means that mod systems configured to run in this Sandbox will have access to:
/// - All the entities within this Sandbox
/// - No entities outside this Sandbox
/// - No entities that are in another Sandbox, even if the Sandbox is nested in this one
///
/// All sandboxes are parallelized.
#[derive(Component)]
#[require(SandboxedEntities)]
#[component(clone_behavior = Ignore, immutable)]
#[component(on_insert = Self::on_insert, on_replace = Self::on_replace, on_remove = Self::on_remove)]
pub struct Sandbox {
    /// A unique id for this sandbox
    id: usize,

    /// The marker Component responsible for tagging all the sandboxes children as belonging to this Sandbox
    ///
    /// Markers have no data, they are just used for queries
    component_id: ComponentId,
}

/// Tracks the last sandbox id so ids are guaranteed to be unique
#[derive(Resource, Default)]
struct SandboxCount(usize);

impl Sandbox {
    /// Creates a new [Sandbox] component
    pub fn new(world: &mut World) -> Self {
        // Start at 1, then count up for each new sandbox
        let id = world.get_resource_or_init::<SandboxCount>().0 + 1;
        world
            .get_resource_mut::<SandboxCount>()
            .expect("SandboxCount to be initialized")
            .0 = id;

        let name = format!("Sandbox {id}");

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

        let component_id = world.register_component_with_descriptor(descriptor);

        Self { id, component_id }
    }

    /// Returns access to only the entities within that sandbox
    ///
    /// This can be used to (for example) create a custom query over just its entities, and thus no entities from outside the sandbox or in another sandbox.
    pub fn access(&self) -> FilteredAccess {
        let mut access = FilteredAccess::matches_everything();
        access.and_with(self.component_id);
        access
    }

    /// Retrieves the system set for this Sandbox
    ///
    /// For each sys
    pub fn set(&self) -> Box<dyn SystemSet> {
        let set = SandboxSet(self.id);
        SystemSet::dyn_clone(&set)
    }

    /// [On insert](bevy::ecs::lifecycle::ComponentHooks::on_insert) for [Sandbox]
    fn on_insert(mut world: DeferredWorld, ctx: HookContext) {
        let Self { id, .. } = world
            .entity(ctx.entity)
            .get()
            .expect("Sandbox was inserted");

        // Add a name if it is missing
        let name = format!("Sandbox {id}");
        world
            .commands()
            .entity(ctx.entity)
            .insert_if_new(Name::new(name));

        // Ensure all children are sandboxed
        Sandboxed::add_children(ctx.entity, ctx.entity, &mut world);
    }

    /// [On replace](bevy::ecs::lifecycle::ComponentHooks::on_replace) for [Sandbox]
    fn on_replace(mut world: DeferredWorld, ctx: HookContext) {
        let Self { component_id, .. } = world
            .entity(ctx.entity)
            .get()
            .expect("Sandbox was replaced");
        let component_id = *component_id;

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
        // A SandboxedEntities and Sandboxed cannot exist without a sandbox
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

        if let Some(Sandbox { component_id, .. }) = world.entity(*sandbox).get() {
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
        if let Some(Sandbox { component_id, .. }) = world.entity(*sandbox).get() {
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
pub struct SandboxSet(usize);

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;

    #[test]
    fn sandboxed_propagate_marker() {
        let mut app = App::new();
        app.add_plugins(SandboxPlugin);

        let world = app.world_mut();

        let component = Sandbox::new(world);
        let marker: ComponentId = component.component_id.clone();
        let sandbox = world.spawn(component).id();
        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        world.flush();

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
        let mut app = App::new();
        app.add_plugins(SandboxPlugin);

        let world = app.world_mut();

        let component = Sandbox::new(world);
        let sandbox = world.spawn(component).id();
        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();
        let nested_child = world.spawn_empty().insert(ChildOf(child)).id();

        world.flush();

        assert_eq!(
            world.entity(sandbox).get(),
            Some(&SandboxedEntities(vec![child, nested_child])),
            "All Children have the sandbox relation and were added to the SandboxedEntities"
        );
    }

    #[test]
    fn reparent_sandboxed() {
        let mut app = App::new();
        app.add_plugins(SandboxPlugin);

        let world = app.world_mut();

        let component = Sandbox::new(world);
        let marker1 = component.component_id;
        let sandbox1 = world.spawn(component).id();
        let component = Sandbox::new(world);
        let marker2 = component.component_id;
        let sandbox2 = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox1)).id();
        let nested_child = world.spawn_empty().insert(ChildOf(child)).id();

        world.flush();

        world.commands().entity(child).insert(ChildOf(sandbox2));

        world.flush();

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
        let mut app = App::new();
        app.add_plugins(SandboxPlugin);

        let world = app.world_mut();

        let component = Sandbox::new(world);
        let marker1 = component.component_id;
        let sandbox = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        world.flush();

        let component = Sandbox::new(world);
        let marker2 = component.component_id;
        world.commands().entity(sandbox).insert(component);

        world.flush();

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
        let mut app = App::new();
        app.add_plugins(SandboxPlugin);

        let world = app.world_mut();

        let component = Sandbox::new(world);
        let marker = component.component_id;
        let sandbox = world.spawn(component).id();

        let child = world.spawn_empty().insert(ChildOf(sandbox)).id();

        world.flush();

        world.commands().entity(sandbox).remove::<Sandbox>();

        world.flush();

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
        let mut app = App::new();
        app.add_plugins(SandboxPlugin);

        let world = app.world_mut();

        let component = Sandbox::new(world);
        let sandbox1 = world.spawn(component).id();
        let child1 = world.spawn(ChildOf(sandbox1)).id();
        let component = Sandbox::new(world);
        let sandbox2 = world.spawn((component, ChildOf(child1))).id();
        let child2 = world.spawn(ChildOf(sandbox2)).id();

        world.flush();

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
}
