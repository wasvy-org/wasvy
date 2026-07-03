use std::{fmt, sync::Arc};

use anyhow::Result;
use bevy_asset::{Asset, AssetId, Assets};
use bevy_ecs::{change_detection::Tick, prelude::*};
use bevy_reflect::TypePath;

use crate::{access::ModAccess, cleanup::DespawnModEntities, mods::ModDespawnBehaviour};

/// Backend implementation driving a [ModAsset]
///
/// For example, one backend may execute wasm files, and another might execute native systems
pub trait ModBackend: Send + Sync + 'static {
    /// Responsible for registering the systems exported by the mod.
    fn setup(
        &self,
        world: &mut World,
        mod_id: Entity,
        mod_name: &str,
        accesses: &[ModAccess],
    ) -> Result<()>;
}

/// An asset representing a loaded wasvy mod.
#[derive(Asset, TypePath)]
pub struct ModAsset {
    version: Option<Tick>,
    backend: Arc<dyn ModBackend>,
}

impl ModAsset {
    /// Creates a mod asset from an arbitrary backend.
    pub fn new(backend: impl ModBackend) -> Self {
        Self {
            version: None,
            backend: Arc::new(backend),
        }
    }

    pub fn version(&self) -> Option<Tick> {
        self.version
    }

    /// Initiates mods by asking the asset backend to run setup and register systems.
    pub(crate) fn initiate(
        world: &mut World,
        asset_id: &AssetId<ModAsset>,
        mod_id: Entity,
        mod_name: &str,
        accesses: &[ModAccess],
    ) -> Result<()> {
        let assets = world
            .get_resource::<Assets<Self>>()
            .expect("ModAssets be registered");
        let asset = assets.get(*asset_id).ok_or(AssetNotFound)?;
        let backend = Arc::clone(&asset.backend);

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

        backend.setup(world, mod_id, mod_name, accesses)
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
