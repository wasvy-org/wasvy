use std::sync::Mutex;
use wasvy_runtime::app_extend::AppExtend;
use wasvy_runtime::devtools;

pub use wasvy_macros::WasvyComponent;
pub use wasvy_runtime::prelude::*;
#[cfg(feature = "wasm")]
pub use wasvy_wasm::WasmBackendPlugin;

/// This plugin adds Wasvy modding support to the [`bevy_app::App`].
///
/// The high-level loader installs the runtime plugin and, when the `wasm`
/// feature is enabled, the WASM backend.
///
/// ```no_run
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::{prelude::*, schedule::ScheduleLabel};
/// # struct DefaultPlugins;
/// # impl Plugin for DefaultPlugins { fn build(&self, app: &mut App) {} }
/// use wasvy::prelude::*;
///
/// App::new()
///    .add_plugins(DefaultPlugins)
///    .add_plugins(ModLoaderPlugin::default())
/// #  .run();
///    // etc
/// ```
///
/// Looking for next steps? See: [`Mods`](wasvy_runtime::mods::Mods).
///
/// ## Examples
///
/// ### Run custom schedules
///
/// In this example, Wasvy is used to load mods that affect a physics simulation.
///
/// In the host:
///
/// ```no_run
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::{prelude::*, schedule::ScheduleLabel};
/// use wasvy::prelude::*;
///
/// #[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash, Default)]
/// struct SimulationStart;
///
/// # let mut app = App::new();
/// // The schedule must be added to the world's Schedules resource.
/// app.add_schedule(Schedule::new(SimulationStart));
///
/// app.add_plugins(
///   // We don't want mods to run systems in any other schedules.
///   ModLoaderPlugin::unscheduled()
///     .enable_schedule(ModSchedule::FixedUpdate)
///     .enable_schedule(ModSchedule::new_custom("simulation-start", SimulationStart))
/// );
/// ```
///
/// In the mod:
///
/// ```ignore
/// fn setup() {
///    // ..
///
///    app.add_systems(&Schedule::FixedUpdate, vec![..]);
///    app.add_systems(&Schedule::Custom("simulation-start".to_string()), vec![..]);
///
///    // This one will be ignored and will emit a warning.
///    app.add_systems(&Schedule::PreUpdate, vec![..]);
/// }
/// ```
pub struct ModLoaderPlugin(Mutex<Option<Inner>>);

struct Inner {
    runtime: ModRuntimePlugin,
    #[cfg(feature = "wasm")]
    wasm: WasmBackendPlugin,
}

impl Default for ModLoaderPlugin {
    fn default() -> Self {
        Self(Mutex::new(Some(Inner {
            runtime: Default::default(),
            #[cfg(feature = "wasm")]
            wasm: Default::default(),
        })))
    }
}

impl ModLoaderPlugin {
    /// Creates a new mod loader that runs mods during the provided schedules.
    pub fn new(schedules: ModSchedules) -> Self {
        Self(Mutex::new(Some(Inner {
            runtime: ModRuntimePlugin::new(schedules),
            #[cfg(feature = "wasm")]
            wasm: Default::default(),
        })))
    }

    /// Creates plugin with no schedules.
    ///
    /// Loaded mods will not run unless you enable schedules manually using
    /// [`ModLoaderPlugin::enable_schedule`].
    ///
    /// If you want Wasvy to run on the default mod schedules, use
    /// [`ModLoaderPlugin::default`] or [`ModLoaderPlugin::new`].
    pub fn unscheduled() -> Self {
        Self(Mutex::new(Some(Inner {
            runtime: ModRuntimePlugin::unscheduled(),
            #[cfg(feature = "wasm")]
            wasm: Default::default(),
        })))
    }

    /// Enables the devtools. The devtools feature must be enabled in your Cargo.toml.
    ///
    /// By default, devtools are enabled on debug (non-release) builds, so the
    /// CLI works without extra configuration. Disable the `devtools` feature to
    /// disable them completely.
    ///
    /// ```
    /// # use wasvy::prelude::*;
    /// # let modloader = ModLoaderPlugin::default();
    /// // Enable and use a custom name.
    /// let modloader = modloader.devtools("My Bevy app");
    ///
    /// // Host a custom WIT interface.
    /// let modloader = modloader.devtools(Devtools {
    ///     program_name: "Expose anything that you can dream of".into(),
    ///     interfaces: vec![],
    /// });
    /// # let _ = modloader;
    /// ```
    pub fn devtools(mut self, config: impl Into<devtools::Devtools>) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).devtools(config);
        self
    }

    /// Sets the despawn behaviour for when mods are despawned (or reloaded).
    ///
    /// The default behaviour is to despawn all entities the mod spawned.
    /// See [`ModDespawnBehaviour::DespawnEntities`].
    pub fn set_despawn_behaviour(mut self, despawn_behaviour: ModDespawnBehaviour) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).set_despawn_behaviour(despawn_behaviour);
        self
    }

    /// Enables a new schedule with the modloader.
    ///
    /// When mods add a system to this schedule, Wasvy automatically adds it to
    /// the host schedule.
    ///
    /// If a mod tries to add a system to a schedule that is not enabled, Wasvy
    /// emits a warning instead.
    ///
    /// In debug mode, this panics if the schedule is already enabled.
    pub fn enable_schedule(mut self, schedule: ModSchedule) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).enable_schedule(schedule);
        self
    }

    /// Configures during which schedule the modloader sets up new systems.
    ///
    /// Defaults to Bevy's [`First`](bevy_app::prelude::First) schedule.
    ///
    /// Schedules cannot be modified while in use, so the setup schedule cannot
    /// also be used to run mod systems.
    pub fn set_setup_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).set_setup_schedule(schedule);
        self
    }

    /// Applies a custom codec for serializing data to and from mods.
    ///
    /// Defaults to [`JsonCodec`](wasvy_runtime::serialize::JsonCodec) when the
    /// `serde_json` feature is enabled.
    pub fn with_codec(mut self, codec: impl WasvyCodec) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).with_codec(codec);
        self
    }

    /// Use this function to add custom functionality that will be passed to WASM modules.
    ///
    /// This is only available when the `wasm` feature is enabled.
    #[cfg(feature = "wasm")]
    pub fn add_functionality<F>(mut self, f: F) -> Self
    where
        F: FnMut(&mut wasvy_wasm::Linker),
    {
        let inner = self.inner();
        inner.wasm = std::mem::take(&mut inner.wasm).add_functionality(f);
        self
    }

    fn inner(&mut self) -> &mut Inner {
        self.0
            .get_mut()
            .expect("ModLoaderPlugin is not locked")
            .as_mut()
            .expect("ModLoaderPlugin is not built")
    }
}

impl bevy_app::Plugin for ModLoaderPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        let Inner {
            runtime,
            #[cfg(feature = "wasm")]
            wasm,
        } = self
            .0
            .lock()
            .expect("ModLoaderPlugin is not locked")
            .take()
            .expect("ModLoaderPlugin is not built");

        app.try_add_plugin(runtime);
        #[cfg(feature = "wasm")]
        app.try_add_plugin(wasm);
    }
}
