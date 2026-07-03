use std::sync::Mutex;
use wasvy_runtime::app_extend::AppExtend;
use wasvy_runtime::devtools;

pub use wasvy_macros::WasvyComponent;
pub use wasvy_runtime::prelude::*;
#[cfg(feature = "wasm")]
pub use wasvy_wasm::WasmBackendPlugin;

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
    /// Creates a new modloader that will schedule mods be run during the provided Schedules
    pub fn new(schedules: ModSchedules) -> Self {
        Self(Mutex::new(Some(Inner {
            runtime: ModRuntimePlugin::new(schedules),
            #[cfg(feature = "wasm")]
            wasm: Default::default(),
        })))
    }

    /// Creates plugin with no schedules.
    pub fn unscheduled() -> Self {
        Self(Mutex::new(Some(Inner {
            runtime: ModRuntimePlugin::unscheduled(),
            #[cfg(feature = "wasm")]
            wasm: Default::default(),
        })))
    }

    /// Enables the devtools. The devtools feature must be enabled in your Cargo.toml.
    pub fn devtools(mut self, config: impl Into<devtools::Devtools>) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).devtools(config);
        self
    }

    /// Sets the despawn behaviour for when mods are despawned (or reloaded).
    pub fn set_despawn_behaviour(mut self, despawn_behaviour: ModDespawnBehaviour) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).set_despawn_behaviour(despawn_behaviour);
        self
    }

    /// Enables a new schedule with the modloader.
    pub fn enable_schedule(mut self, schedule: ModSchedule) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).enable_schedule(schedule);
        self
    }

    /// Configures during which schedule the modloader sets up new systems.
    pub fn set_setup_schedule(mut self, schedule: impl ScheduleLabel) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).set_setup_schedule(schedule);
        self
    }

    /// Apply a custom codec for serializing data to/from mods
    pub fn with_codec(mut self, codec: impl WasvyCodec) -> Self {
        let inner = self.inner();
        inner.runtime = std::mem::take(&mut inner.runtime).with_codec(codec);
        self
    }

    /// Use this function to add custom functionality that will be passed to WASM modules.
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
