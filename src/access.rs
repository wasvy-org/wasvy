use bevy_ecs::{prelude::*, query::FilteredAccess};
use bevy_reflect::Reflect;

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
        match self {
            Self::Sandbox(entity) => world
                .get::<Sandbox>(*entity)
                .map(|sandbox| sandbox.schedules().clone())
                // The sandbox doesn't exist, so no schedules
                .unwrap_or_else(ModSchedules::empty),
            Self::World => world
                .get_resource::<ModSchedules>()
                .cloned()
                .expect("ModSchedules be registered"),
        }
    }

    /// Returns world access to only the entities granted by this access.
    ///
    /// This is used by Wasvy to build mod systems that don't conflict (can run in parallel) between different accesses.
    pub fn filtered_access(&self, world: &World) -> FilteredAccess {
        match self {
            Self::Sandbox(entity) => world
                .get::<Sandbox>(*entity)
                .map(|sandbox| sandbox.access().clone())
                // The sandbox doesn't exist, so there is nothing to match
                .unwrap_or_else(FilteredAccess::matches_nothing),
            Self::World => Sandbox::access_non_sandboxed(world),
        }
    }
}
