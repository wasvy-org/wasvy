use bevy::{
    ecs::{entity::Entity, query::FilteredAccess, world::World},
    reflect::Reflect,
};

use crate::prelude::{ModSchedules, Sandbox};

/// Represents the access a mod can be given to run in.
///
/// Mods can run in the world and/or in [sandboxes](Sandbox) defined by their entity.
///
/// See: [Mods::enable_access](crate::mods::Mods::enable_access)
#[derive(Reflect, Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum ModAccess {
    World,
    Sandbox(Entity),
}

impl ModAccess {
    /// Resolves the schedules configured to run for this mod
    pub fn schedules(&self, world: &World) -> ModSchedules {
        if let Self::Sandbox(entity) = self {
            if let Some(sandbox) = world.get::<Sandbox>(*entity) {
                sandbox.schedules().clone()
            } else {
                // The sandbox doesn't exist, so no schedules
                ModSchedules::empty()
            }
        } else {
            world
                .get_resource::<ModSchedules>()
                .map(Clone::clone)
                .expect("ModSchedules be registered")
        }
    }

    /// Returns world access to only the entities granted by this access.
    ///
    /// This is used by Wasvy to build mod systems that don't conflict (can run in parallel) between different accesses.
    pub fn filtered_access(&self, world: &World) -> FilteredAccess {
        if let Self::Sandbox(entity) = self {
            if let Some(sandbox) = world.get::<Sandbox>(*entity) {
                sandbox.access(world)
            } else {
                // The sandbox doesn't exist, so there is nothing to match
                FilteredAccess::matches_nothing()
            }
        } else {
            Sandbox::access_non_sandboxed(world)
        }
    }
}
