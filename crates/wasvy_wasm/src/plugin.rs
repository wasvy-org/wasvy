use std::sync::Mutex;

use bevy_app::Plugin;
use bevy_asset::prelude::*;
use wasvy_runtime::asset::ModAsset;

use crate::{
    engine::{Engine, Linker, create_linker},
    wasm_asset::WasmAssetLoader,
};

/// Adds the Wasm mod loader for [`wasvy_runtime`].
pub struct WasmLoaderPlugin(Mutex<Option<Inner>>);

struct Inner {
    engine: Engine,
    linker: Linker,
}

impl Default for WasmLoaderPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmLoaderPlugin {
    pub fn new() -> Self {
        let engine = Engine::default();
        let linker = create_linker(&engine);
        Self(Mutex::new(Some(Inner { engine, linker })))
    }

    /// Use this function to add custom functionality that will be passed to WASM modules.
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
            .expect("WasmLoaderPlugin is not locked")
            .as_mut()
            .expect("WasmLoaderPlugin is not built")
    }
}

impl Plugin for WasmLoaderPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        let Inner { engine, linker } = self
            .0
            .lock()
            .expect("WasmLoaderPlugin is not locked")
            .take()
            .expect("WasmLoaderPlugin is not built");

        app.init_asset::<ModAsset>()
            .register_asset_loader(WasmAssetLoader::new(linker))
            .insert_resource(engine);
    }
}
