use bevy::{
    asset::AssetPath, ecs::system::SystemParam, platform::collections::HashSet, prelude::*,
};

use crate::{asset::ModAsset, sandbox::Sandbox};

/// This system param provides an interface to load and manage Wasvy mods
#[derive(SystemParam)]
pub struct Mods<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, AssetServer>,
    mods: Query<'w, 's, Entity, With<Mod>>,
    sandboxes: Query<'w, 's, (Entity, &'static Sandbox)>,
}

impl Mods<'_, '_> {
    /// Loads a single wasm file from the given path.
    ///
    /// This [spawns](Self::spawn) a new instance of the mod and configures it to run in the world.
    ///
    /// The mod will be given access to the entire World. See [docs for Global Sandbox](Sandbox)
    pub fn load<'a>(&mut self, path: impl Into<AssetPath<'a>>) {
        let mod_id = self.spawn(path);
        let sandbox_id = self.global_sandbox();
        self.add_to_sandbox(mod_id, sandbox_id);
    }

    /// Spawns a new mod from the given path.
    ///
    /// By default this mod will do nothing. See [Self::add_to_sandbox].
    pub fn spawn<'a>(&mut self, path: impl Into<AssetPath<'a>>) -> Entity {
        let path = path.into();
        let name = path
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or("unknown".to_string());
        let asset = self.asset_server.load::<ModAsset>(path);

        info!("Loading mod \"{name}\"");

        self.commands.spawn((Mod::new(asset), Name::new(name))).id()
    }

    /// Despawns a mod, removing its systems from the sandboxes it was added to.
    ///
    /// This is equivalent to doing:
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// fn system(mut commands: Commands) {
    /// #   let my_mod = commands.spawn_empty().id();
    ///     commands.entity(my_mod).despawn()
    /// }
    /// ```
    ///
    /// If all Mods with handles to the same wasm asset is dropped, then it will be unloaded. If this is undesirable,
    /// then simply keep a [handle](Mod::asset) to it or spawn an extra mod without adding it to a sandbox.
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn despawn(&mut self, entity: Entity) {
        debug_assert!(self.mods.contains(entity));
        self.commands.entity(entity).despawn();
    }

    /// Adds a mod to the given sandbox
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn add_to_sandbox<'a>(&mut self, mod_id: Entity, sandbox: Entity) {
        self.commands.queue(move |world: &mut World| {
            if let Some(mut mod_id) = world.get_mut::<Mod>(mod_id) {
                mod_id.add_to_sandbox(sandbox);
            }
        });
    }

    /// Removes a mod from the given sandbox
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn remove_from_sandbox<'a>(&mut self, mod_id: Entity, sandbox: Entity) {
        self.commands.queue(move |world: &mut World| {
            if let Some(mut mod_id) = world.get_mut::<Mod>(mod_id) {
                mod_id.add_to_sandbox(sandbox);
            }
        });
    }

    /// Returns the entity for the [Global Sandbox](Sandbox)
    pub fn global_sandbox(&self) -> Entity {
        self.sandboxes
            .iter()
            .find(|(_, sandbox)| sandbox.is_global())
            .map(|(entity, _)| entity)
            .expect("global sandbox to have been spawned")
    }

    /// Unload all currently loaded mods.
    pub fn clear(&mut self) {
        for entity in self.mods.iter() {
            self.commands.entity(entity).despawn();
        }
    }
}

/// A Bevy wasm mod.
///
/// Note: Bevy drops assets if there are no active handles so
/// this component holds a reference to it in order to keep it alive.
#[derive(Component, Reflect)]
pub struct Mod {
    /// A handle to wasm file for this mod
    asset: Handle<ModAsset>,

    /// All of the [Sandbox] this mod should be running in
    sandboxes: HashSet<Entity>,
}

impl Mod {
    /// Creates a new instance of a mod.
    ///
    /// Hint: Get a value for asset by calling [Self::asset] on an already spawned mod.
    pub fn new(asset: Handle<ModAsset>) -> Self {
        Self {
            asset,
            sandboxes: HashSet::new(),
        }
    }

    /// Returns the asset handle of this mod
    pub fn asset(&self) -> Handle<ModAsset> {
        self.asset.clone()
    }

    /// Adds a mod to the given sandbox
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn add_to_sandbox<'a>(&mut self, sandbox: Entity) -> bool {
        self.sandboxes.insert(sandbox)
    }

    /// Removes a mod from the given sandbox
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn remove_from_sandbox<'a>(&mut self, sandbox: Entity) -> bool {
        self.sandboxes.remove(&sandbox)
    }

    /// Returns an iterator over the sandboxes contained by this mod
    pub fn sandboxes(&self) -> impl Iterator<Item = &Entity> {
        self.sandboxes.iter()
    }

    /// Retrieves the system set for a Mod.
    ///
    /// All of the mod's systems will be included in this set.
    ///
    /// The entity should be an entity with a Mod component.
    pub fn system_set(mod_id: Entity) -> ModSet {
        ModSet(mod_id)
    }
}

/// A unique set containing all the systems for a specific Mod
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub struct ModSet(Entity);
