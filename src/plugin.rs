use std::sync::Mutex;

use bevy::{
    ecs::{intern::Interned, schedule::ScheduleLabel},
    prelude::*,
};

use crate::{
    asset::{ModAsset, ModAssetLoader},
    component::WasmComponentRegistry,
    engine::{Engine, Linker, create_linker},
    schedule::{ModStartup, Schedule, Schedules},
    systems::run_setup,
};

/// This plugin adds Wasvy modding support to [`App`]
///
/// ```rust
///  App::new()
///    .add_plugins(DefaultPlugins)
///    .add_plugins(ModloaderPlugin::default())
///    // etc
/// ```
///
/// Looking for next steps? See: [`Mods`](crate::mods::Mods)
/// ```
pub struct ModloaderPlugin(Mutex<Option<Inner>>);

struct Inner {
    engine: Engine,
    linker: Linker,
    schedules: Schedules,
    setup_schedule: Interned<dyn ScheduleLabel>,
}

impl Default for ModloaderPlugin {
    fn default() -> Self {
        Self::new(Schedules::default())
    }
}

impl ModloaderPlugin {
    /// Creates plugin with no schedules.
    ///
    /// This means that by default loaded mods will not run unless you add schedules manually using [ModloaderPlugin::add_schedule]
    ///
    /// If you want wasvy to run on all default schedules use `ModloaderPlugin::default()`
    pub fn unscheduled() -> Self {
        Self::new(Schedules::empty())
    }

    /// Adds a new schedule to the modloader.
    ///
    /// If mods add a system to this schedule, then wasvy will run them.
    pub fn add_schedule(mut self, schedule: Schedule) -> Self {
        let inner = self.inner();
        inner.schedules.push(schedule);
        self
    }

    /// Configures during which schedule the modloader sets up new systems.
    ///
    /// Default's to Bevy's [First] schedule.
    ///
    /// Due to technical limitations a schedule can't both be used to setup mods and run mod systems.
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

    fn new(schedules: Schedules) -> Self {
        let engine = Engine::new();
        let linker = create_linker(&engine);
        let setup_schedule = First.intern();
        let inner = Inner {
            engine,
            linker,
            schedules,
            setup_schedule,
        };
        ModloaderPlugin(Mutex::new(Some(inner)))
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
        } = self
            .0
            .lock()
            .expect("ModloaderPlugin is not locked")
            .take()
            .expect("ModloaderPlugin is not built");

        app.init_asset::<ModAsset>()
            .register_asset_loader(ModAssetLoader { linker })
            .insert_resource(engine)
            .insert_resource(schedules)
            .init_resource::<WasmComponentRegistry>()
            .add_schedule(ModStartup::new_schedule())
            .add_systems(setup_schedule, run_setup);

        let asset_plugins = app.get_added_plugins::<AssetPlugin>();
        let asset_plugin = asset_plugins
            .get(0)
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
