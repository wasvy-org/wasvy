use std::sync::Mutex;

use crate::{
    asset::{ModAsset, ModAssetLoader},
    component_registry::WasmComponentRegistry,
    engine::{Engine, Linker, create_linker},
    systems::run_setup,
};
use bevy::prelude::*;

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
}

impl Default for ModloaderPlugin {
    fn default() -> Self {
        let engine = Engine::new();
        let linker = create_linker(&engine);
        let inner = Inner { engine, linker };
        ModloaderPlugin(Mutex::new(Some(inner)))
    }
}

impl ModloaderPlugin {
    /// Use this function to add custom functionality that will be passed to the WASM module.
    pub fn add_functionality<F>(mut self, mut f: F) -> Self
    where
        F: FnMut(&mut Linker),
    {
        let inner = self.0.get_mut().unwrap().as_mut().unwrap();
        f(&mut inner.linker);
        self
    }
}

impl Plugin for ModloaderPlugin {
    fn build(&self, app: &mut App) {
        let Inner { engine, linker } = self.0.lock().unwrap().take().unwrap();

        app.init_asset::<ModAsset>()
            .register_asset_loader(ModAssetLoader { linker });

        app.insert_resource(engine)
            .init_resource::<WasmComponentRegistry>();

        app.add_systems(PreUpdate, run_setup);

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
                .unwrap()
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
