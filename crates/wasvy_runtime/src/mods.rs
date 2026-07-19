use std::fmt;

use bevy_asset::{AssetPath, AssetServer, Handle};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{
    change_detection::MaybeLocation, error::warn, lifecycle::HookContext, prelude::*,
    system::SystemParam, world::DeferredWorld,
};
use bevy_log::prelude::*;
use bevy_platform::collections::HashSet;
use bevy_reflect::Reflect;

use crate::{access::ModAccess, asset::ModAsset, cleanup::DisableSystemSet};

/// This system param provides an interface to load and manage Wasvy mods
#[derive(SystemParam)]
pub struct Mods<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, AssetServer>,
    mods: Query<'w, 's, Entity, With<Mod>>,
}

impl Mods<'_, '_> {
    /// Loads a single wasm file from the given path.
    ///
    /// This [spawns](Self::spawn) a new instance of the mod and configures it to run in the world.
    ///
    /// The mod will be given access to the entire World. See [docs for Global Sandbox](crate::sandbox::Sandbox)
    pub fn load<'a>(&mut self, path: impl Into<AssetPath<'a>>) {
        let mod_id = self.spawn(path, None);
        self.enable_access(mod_id, ModAccess::World);
    }

    /// Spawns a new instance of a mod from the given path. By default this mod will do nothing once loaded.
    ///
    /// Next, you might want to give this mod access via [Self::enable_access].
    pub fn spawn<'a>(&mut self, path: impl Into<AssetPath<'a>>, name: Option<String>) -> Entity {
        let path = path.into();
        let name = name.unwrap_or_else(|| {
            path.path()
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let asset = self.asset_server.load(path);

        info!("Loading mod \"{name}\"");
        self.commands.spawn((Mod::new(asset), Name::new(name))).id()
    }

    /// Despawns a mod, removing its systems from the sandboxes it was added to.
    ///
    /// This is equivalent to doing:
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
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
    /// schedule (which defaults to [First](bevy_app::First), see
    /// [ModRuntimePlugin::set_setup_schedule](crate::plugin::ModRuntimePlugin::set_setup_schedule)) runs.
    #[track_caller]
    pub fn despawn(&mut self, mod_id: Entity) {
        let caller = MaybeLocation::caller();
        let command = move |world: &mut World| -> Result<(), BevyError> {
            let entity = Mod::get_entity_mut(mod_id, world)
                .map_err(|error| format!("{error}, could not despawn\n{caller}"))?;
            let name = Mod::get_name(&entity);

            info!("Unloading mod \"{name}\"");
            entity.despawn();

            Ok(())
        };
        self.commands.queue_handled(command, warn);
    }

    /// Enable a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy_app::First), see
    /// [ModRuntimePlugin::set_setup_schedule](crate::plugin::ModRuntimePlugin::set_setup_schedule)) runs.
    #[track_caller]
    pub fn enable_access(&mut self, mod_id: Entity, access: ModAccess) {
        let caller = MaybeLocation::caller();
        let command = move |world: &mut World| -> Result<(), BevyError> {
            access
                .validate(world)
                .map_err(|error| format!("{error}\n{caller}"))?;

            let access_display = access.display(world);
            let mut entity = Mod::get_entity_mut(mod_id, world).map_err(|error| {
                format!("{error}, could not enable access {access_display}\n{caller}")
            })?;
            let name = Mod::get_name(&entity);
            info!("Enabling {access_display} access for mod \"{name}\"");

            entity
                .get_mut::<Mod>()
                .expect("checked by get_entity_mut")
                .enable_access(access);

            Ok(())
        };
        self.commands.queue_handled(command, warn);
    }

    /// Removes a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy_app::First), see
    /// [ModRuntimePlugin::set_setup_schedule](crate::plugin::ModRuntimePlugin::set_setup_schedule)) runs.
    #[track_caller]
    pub fn disable_access(&mut self, mod_id: Entity, access: ModAccess) {
        let caller = MaybeLocation::caller();
        let command = move |world: &mut World| -> Result<(), BevyError> {
            access
                .validate(world)
                .map_err(|error| format!("{error}\n{caller}"))?;

            let access_display = access.display(world);
            let mut entity = Mod::get_entity_mut(mod_id, world).map_err(|error| {
                format!("{error}, could not disable access {access_display}\n{caller}")
            })?;
            let name = Mod::get_name(&entity);
            info!("Disabling {access_display} access for mod \"{name}\"");

            entity
                .get_mut::<Mod>()
                .expect("checked by get_entity_mut")
                .disable_access(&access);

            Ok(())
        };
        self.commands.queue_handled(command, warn);
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
    /// schedule (which defaults to [First](bevy_app::First), see
    /// [ModRuntimePlugin::set_setup_schedule](crate::plugin::ModRuntimePlugin::set_setup_schedule)) runs.
    pub fn enable_access(&mut self, access: ModAccess) -> bool {
        self.access.insert(access)
    }

    /// Removes a [Mod]'s access to entities.
    ///
    /// See: [ModAccess]
    ///
    /// Note: The effect of this change is not immediate. This change will apply after the setup
    /// schedule (which defaults to [First](bevy_app::First), see
    /// [ModRuntimePlugin::set_setup_schedule](crate::plugin::ModRuntimePlugin::set_setup_schedule)) runs.
    pub fn disable_access(&mut self, access: &ModAccess) -> bool {
        self.access.remove(access)
    }

    /// Returns an iterator over the [Mod Accesses](ModAccess) of this mod
    pub fn accesses(&self) -> impl Iterator<Item = &ModAccess> {
        self.access.iter()
    }

    fn get_entity_mut<'a>(id: Entity, world: &'a mut World) -> Result<EntityWorldMut<'a>, String> {
        let Some(entity) = world.get_entity_mut(id).ok() else {
            return Err(format!("Entity ({id}) does not exist"));
        };

        if entity.get::<Mod>().is_none() {
            return Err(format!("Entity ({id}) is not a Mod"));
        }

        Ok(entity)
    }

    fn get_name<'a>(entity: &'a EntityWorldMut) -> ModName<'a> {
        entity
            .get::<Name>()
            .map(|n| ModName::Named(n.as_str()))
            .unwrap_or(ModName::Unknown(entity.id()))
    }

    /// [On despawn](bevy_ecs::lifecycle::ComponentHooks::on_despawn) for [Mod]
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
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub enum ModSystemSet {
    /// A system set for all mod systems
    ///
    /// Use this variant if you want to define systems that run before or after all mods.
    #[default]
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

    /// Creates the system set for a [Sandbox](crate::sandbox::Sandbox) (all sandboxed systems).
    ///
    /// All of the sandbox's systems (from any mod) will be included in this set.
    ///
    /// The provided sandbox_id should be an entity with a Sandbox component.
    pub const fn new_sandboxed(sandbox_id: Entity) -> Self {
        Self::Access(ModAccess::Sandbox(sandbox_id))
    }
}

/// An enum that defines what happens when a mod is despawned (or reloaded)
///
/// Set this value during plugin instantiation via
/// [ModRuntimePlugin::set_despawn_behaviour](crate::plugin::ModRuntimePlugin::set_despawn_behaviour).
///
/// The default behaviour is to despawn all entities this mod spawned.
/// See [DespawnEntities](ModDespawnBehaviour::DespawnEntities).
#[derive(Resource, Debug, Default, PartialEq, Eq)]
#[component(immutable)]
pub enum ModDespawnBehaviour {
    /// Do nothing when a mod is despawned
    None,

    /// The default. Despawn all entities this mod spawned.
    ///
    /// So for example if your mod spawns a cube in the center of the scene,
    /// when this mod is hot reloaded the cube is despawned, and the newest
    /// version of the mod spawns a new cube in its place.
    #[default]
    DespawnEntities,
}

impl ModDespawnBehaviour {
    pub(crate) fn should_despawn_entities(world: &World) -> bool {
        match world.get_resource() {
            None | Some(ModDespawnBehaviour::DespawnEntities) => true,
            Some(ModDespawnBehaviour::None) => false,
        }
    }
}

/// Determines whether `DespawnModEntities` should be inserted to entities spawned by mods
#[derive(Clone, Copy, Deref, DerefMut)]
pub struct InsertDespawnComponent(Option<Entity>);

impl InsertDespawnComponent {
    pub fn new(mod_id: Entity, world: &World) -> Self {
        Self(if ModDespawnBehaviour::should_despawn_entities(world) {
            Some(mod_id)
        } else {
            None
        })
    }
}

enum ModName<'a> {
    Named(&'a str),
    Unknown(Entity),
}

impl<'a> fmt::Display for ModName<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModName::Named(name) => f.write_str(name),
            ModName::Unknown(entity) => write!(f, "unknown ({entity})"),
        }
    }
}
