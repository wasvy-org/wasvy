use std::sync::Mutex;

use bevy_app::prelude::*;
use bevy_asset::prelude::*;
use bevy_ecs::{intern::Interned, schedule::ScheduleLabel};
use bevy_log::prelude::*;

use crate::{
    authoring::register_all,
    asset::{ModAsset, ModAssetLoader},
    cleanup::{DespawnModEntities, DisableSystemSet, disable_mod_system_sets},
    component::WasmComponentRegistry,
    engine::{Engine, Linker, create_linker},
    methods::MethodRegistry,
    mods::{Mod, ModDespawnBehaviour},
    sandbox::Sandboxed,
    schedule::{ModSchedule, ModSchedules, ModStartup},
    setup::run_setup,
};

/// This plugin adds Wasvy modding support to [`App`]
///
/// ```no_run
/// # use bevy_ecs::{prelude::*, schedule::ScheduleLabel};
/// # use bevy_app::prelude::*;
/// # struct DefaultPlugins;
/// # impl Plugin for DefaultPlugins { fn build(&self, app: &mut App){} }
/// use wasvy::prelude::*;
///
/// App::new()
///    .add_plugins(DefaultPlugins)
///    .add_plugins(ModloaderPlugin::default())
/// #  .run();
///    // etc
/// ```
///
/// Looking for next steps? See: [`Mods`](crate::mods::Mods)
///
/// ## Examples
///
/// ### Run custom schedules
///
/// In this example, Wasvy is used to load mods that affect a physics simulation.
///
/// In the host:
/// ```no_run
/// # use bevy_ecs::{prelude::*, schedule::ScheduleLabel};
/// # use bevy_app::prelude::*;
/// use wasvy::prelude::*;
///
/// #[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash, Default)]
/// struct SimulationStart;
///
/// # let mut app = App::new();
/// // The schedule must be added to the world's Schedules Resource
/// app.add_schedule(Schedule::new(SimulationStart));
///
/// app.add_plugins(
///   // We don't want mods to run systems in any other schedules
///   ModloaderPlugin::unscheduled()
///     .enable_schedule(ModSchedule::FixedUpdate)
///     .enable_schedule(ModSchedule::new_custom("simulation-start", SimulationStart))
/// );
/// ```
///
/// In the mod:
/// ```ignore
/// fn setup(){
///    ..
///
///    app.add_systems(&Schedule::FixedUpdate, vec![..]);
///    app.add_systems(&Schedule::Custom("simulation-start".to_string()), vec![..]);
///
///    // This one will be ignored and throw a warning
///    app.add_systems(&Schedule::PreUpdate, vec![..]);
/// }
/// ```
pub struct ModloaderPlugin(Mutex<Option<Inner>>);

struct Inner {
    engine: Engine,
    linker: Linker,
    schedules: ModSchedules,
    setup_schedule: Interned<dyn ScheduleLabel>,
    despawn_behaviour: ModDespawnBehaviour,
}

impl Default for ModloaderPlugin {
    fn default() -> Self {
        Self::new(ModSchedules::default())
    }
}

impl ModloaderPlugin {
    /// Creates a new modloader that will schedule mods be run during the provided Schedules
    pub fn new(schedules: ModSchedules) -> Self {
        let engine = Engine::new();
        let linker = create_linker(&engine);
        let setup_schedule = First.intern();
        let despawn_behaviour = ModDespawnBehaviour::default();
        let inner = Inner {
            engine,
            linker,
            schedules,
            setup_schedule,
            despawn_behaviour,
        };
        ModloaderPlugin(Mutex::new(Some(inner)))
    }

    /// Creates plugin with no schedules.
    ///
    /// This means that by default loaded mods will not run unless you enable schedules manually using [ModloaderPlugin::enable_schedule]
    ///
    /// If you want wasvy to run on all schedules use `ModloaderPlugin::default()` or [ModloaderPlugin::new]
    pub fn unscheduled() -> Self {
        Self::new(ModSchedules::empty())
    }

    /// Sets the despawn behaviour for when mods are despawned (or reloaded).
    ///
    /// The default behaviour is to despawn all entities the mod spawned.
    /// See [DespawnEntities](ModDespawnBehaviour::DespawnEntities).
    pub fn set_despawn_behaviour(mut self, despawn_behaviour: ModDespawnBehaviour) -> Self {
        let inner = self.inner();
        inner.despawn_behaviour = despawn_behaviour;
        self
    }

    /// Enables a new schedule with the modloader.
    ///
    /// When mods add a system to this schedule, then wasvy will automatically add them to the schedule.
    ///
    /// If a mod tries to call add_system with an schedule that isn't enabled this will just produce a warning.
    ///
    /// In debug mode, this will panic if the schedule is already added.
    pub fn enable_schedule(mut self, schedule: ModSchedule) -> Self {
        let inner = self.inner();
        inner.schedules.push(schedule);
        self
    }

    /// Configures during which schedule the modloader sets up new systems.
    ///
    /// Defaults to Bevy's [First] schedule.
    ///
    /// Schedules can't be modified while in use, therefore a schedule can't both be used to setup mods and run mod systems simultaneously.
    pub fn set_setup_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        let inner = self.inner();
        inner.setup_schedule = schedule.intern();
        self
    }

    /// Use this function to add custom functionality that will be passed to the WASM module.
    pub fn add_functionality<F>(mut self, mut f: F) -> Self
    where
        F: FnMut(&mut Linker),
    {
        let inner = self.inner();
        f(&mut inner.linker);
        self
    }

    fn inner(&mut self) -> &mut Inner {
        self.0
            .get_mut()
            .expect("ModloaderPlugin is not locked")
            .as_mut()
            .expect("ModloaderPlugin is not built")
    }
}

impl Plugin for ModloaderPlugin {
    fn build(&self, app: &mut App) {
        let Inner {
            engine,
            linker,
            schedules,
            setup_schedule,
            despawn_behaviour,
        } = self
            .0
            .lock()
            .expect("ModloaderPlugin is not locked")
            .take()
            .expect("ModloaderPlugin is not built");

        if despawn_behaviour == ModDespawnBehaviour::DespawnEntities {
            // Registers a component that tracks mod entities and despawns them when the mod despawns
            app.register_required_components::<Mod, DespawnModEntities>();
        }

        app.init_asset::<ModAsset>()
            .register_asset_loader(ModAssetLoader { linker })
            .insert_resource(engine)
            .insert_resource(despawn_behaviour)
            .init_resource::<WasmComponentRegistry>()
            .init_resource::<MethodRegistry>()
            .insert_resource(schedules)
            .add_schedule(ModStartup::new_schedule())
            .add_message::<DisableSystemSet>()
            .add_systems(setup_schedule, (run_setup, disable_mod_system_sets));

        register_all(app);

        app.world_mut().register_component::<Sandboxed>();

        let asset_plugins = app.get_added_plugins::<AssetPlugin>();
        let asset_plugin = asset_plugins
            .first()
            .expect("ModloaderPlugin requires AssetPlugin to be loaded.");

        // Warn a user running the App in debug; they probably want hot-reloading
        if cfg!(debug_assertions) {
            let user_overrode_watch_setting = asset_plugin.watch_for_changes_override.is_some();
            let resolved_watch_setting = app
                .world()
                .get_resource::<AssetServer>()
                .expect("ModloaderPlugin requires AssetPlugin to be loaded.")
                .watching_for_changes();

            if !user_overrode_watch_setting && !resolved_watch_setting {
                warn!(
                    "Enable Bevy's watch feature to enable hot-reloading Wasvy mods.\
                    You can do this by running the command `cargo run --features bevy/file_watcher`.\
                    In order to hide this message, set the `watch_for_changes_override` to\
                    `Some(true)` or `Some(false)` in the AssetPlugin."
                );
            }
        }
    }
}
