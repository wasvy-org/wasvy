use bevy::{
    asset::AssetPath,
    ecs::{lifecycle::HookContext, system::SystemParam, world::DeferredWorld},
    platform::collections::HashSet,
    prelude::*,
};

use crate::{access::ModAccess, asset::ModAsset, cleanup::DisableSystemSet, prelude::Sandbox};

/// This system param provides an interface to load and manage Wasvy mods
#[derive(SystemParam)]
pub struct Mods<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, AssetServer>,
    mods: Query<'w, 's, Entity, With<Mod>>,
    #[cfg(debug_assertions)]
    sandboxes: Query<'w, 's, Entity, With<Sandbox>>,
}

impl Mods<'_, '_> {
    /// Loads a single wasm file from the given path.
    ///
    /// This [spawns](Self::spawn) a new instance of the mod and configures it to run in the world.
    ///
    /// The mod will be given access to the entire World. See [docs for Global Sandbox](Sandbox)
    pub fn load<'a>(&mut self, path: impl Into<AssetPath<'a>>) {
        let mod_id = self.spawn(path);
        self.enable_access(mod_id, ModAccess::World);
    }

    /// Spawns a new instance of a mod from the given path. By default this mod will do nothing once loaded.
    ///
    /// Next, you might want to give this mod access via [Self::enable_access].
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

    /// Enable a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn enable_access<'a>(&mut self, mod_id: Entity, access: ModAccess) {
        #[cfg(debug_assertions)]
        if let ModAccess::Sandbox(entity) = access {
            assert!(
                self.sandboxes.contains(entity),
                "ModAccess::Sandbox should contain a valid entity"
            );
        }
        self.commands.queue(move |world: &mut World| {
            if let Some(mut mod_id) = world.get_mut::<Mod>(mod_id) {
                mod_id.enable_access(access);
            }
        });
    }

    /// Removes a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn disable_sandbox_access<'a>(&mut self, mod_id: Entity, access: ModAccess) {
        #[cfg(debug_assertions)]
        if let ModAccess::Sandbox(entity) = access {
            assert!(
                self.sandboxes.contains(entity),
                "ModAccess::Sandbox should contain a valid entity"
            );
        }
        self.commands.queue(move |world: &mut World| {
            if let Some(mut mod_id) = world.get_mut::<Mod>(mod_id) {
                mod_id.disable_access(&access);
            }
        });
    }

    /// Unload all currently loaded mods.
    pub fn despawn_all(&mut self) {
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
#[component(on_despawn = Self::on_despawn)]
pub struct Mod {
    /// A handle to wasm file for this mod
    asset: Handle<ModAsset>,

    /// All of the [accesses](ModAccess) this mod should have
    ///
    /// A mod will run in the world or in a sandbox, only when it is given
    /// explicit access to do so by adding them to this set.
    access: HashSet<ModAccess>,
}

impl Mod {
    /// Creates a new instance of a mod.
    ///
    /// Hint: Get a value for asset by calling [Self::asset] on an already spawned mod.
    pub fn new(asset: Handle<ModAsset>) -> Self {
        Self {
            asset,
            access: HashSet::new(),
        }
    }

    /// Returns the asset handle of this mod
    pub fn asset(&self) -> Handle<ModAsset> {
        self.asset.clone()
    }

    /// Enable a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn enable_access<'a>(&mut self, access: ModAccess) -> bool {
        self.access.insert(access)
    }

    /// Removes a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy::app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)) runs.
    pub fn disable_access<'a>(&mut self, access: &ModAccess) -> bool {
        self.access.remove(access)
    }

    /// Returns an iterator over the accesses of this mod
    pub(crate) fn accesses(&self) -> impl Iterator<Item = &ModAccess> {
        self.access.iter()
    }

    /// [On despawn](bevy::ecs::lifecycle::ComponentHooks::on_despawn) for [Mod]
    fn on_despawn(mut world: DeferredWorld, ctx: HookContext) {
        let mod_component = world
            .entity(ctx.entity)
            .get::<Self>()
            .expect("Mod was removed");

        // After a mod is removed, its systems should no longer run
        // The effects of DisableSystemSet are permanent, so we can only call it when this entity is despawned from the world
        for access in mod_component.access.clone() {
            let schedules = access.schedules(&world);
            world.commands().queue(DisableSystemSet {
                set: ModSystemSet::Mod(ctx.entity),
                schedules,
            });
        }
    }
}

/// SystemSets for systems from mod added to the schedule graph by wasvy
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub enum ModSystemSet {
    /// A system set for all mod systems
    ///
    /// Use this variant if you want to define systems that run before or after all mods.
    All,

    /// A system set for all the systems of a specific Mod
    ///
    /// Use this variant if you always want to schedule systems run before or after a mod's own systems
    Mod(Entity),

    /// A system set for all the systems (belonging to any mod) that were given specific access
    ///
    /// See the [Self::new_world] and [Self::new_sandboxed] docs for use cases.
    Access(ModAccess),
}

impl ModSystemSet {
    /// Creates the system set for all Mods.
    ///
    /// All mod systems will be included in this set.
    ///
    /// This is useful if you want to define systems that run before or after all mods.
    pub const fn new() -> Self {
        Self::All
    }

    /// Creates the system set for a Mod.
    ///
    /// All of the mod's systems will be included in this set.
    ///
    /// The provided mod_id should be an entity with a Mod component.
    pub const fn new_mod(mod_id: Entity) -> Self {
        Self::Mod(mod_id)
    }

    /// Creates the system set for all mods accessing the world.
    ///
    /// All of the systems that are not sandboxed will be included in this set.
    ///
    /// This is useful if you want to define systems that run before or after all mods running in the world.
    pub const fn new_world() -> Self {
        Self::Access(ModAccess::World)
    }

    /// Creates the system set for a [Sandbox] (all snaboxed systems).
    ///
    /// All of the sandbox's systems (from any mod) will be included in this set.
    ///
    /// The provided sandbox_id should be an entity with a Sandbox component.
    pub const fn new_sandboxed(sandbox_id: Entity) -> Self {
        Self::Access(ModAccess::Sandbox(sandbox_id))
    }
}
