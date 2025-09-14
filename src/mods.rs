use bevy::{asset::AssetPath, ecs::system::SystemParam, prelude::*};

use crate::asset::ModAsset;

/// This system param provides an interface to load and manage Wasvy mods
#[derive(SystemParam)]
pub struct Mods<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, AssetServer>,
    mods: Query<'w, 's, Entity, With<Mod>>,
}

/// Bevy drops assets if there are no active handles
/// so this component exists to keep the handles alive.
#[derive(Component, Reflect)]
pub(crate) struct Mod {
    pub asset: Handle<ModAsset>,
}

impl Mods<'_, '_> {
    /// Load a single wasm file from the given path.
    pub fn load<'a>(&mut self, path: impl Into<AssetPath<'a>>) {
        let path: AssetPath = path.into();
        let name = path
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or("unknown".to_string());
        let asset = self.asset_server.load::<ModAsset>(path);
        self.commands.spawn((Name::new(name), Mod { asset }));
    }

    /// Unload all currently loaded mods.
    pub fn clear(&mut self) {
        for entity in self.mods.iter() {
            self.commands.entity(entity).despawn();
        }
    }
}
