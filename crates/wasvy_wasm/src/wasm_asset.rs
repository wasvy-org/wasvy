use anyhow::{Result, anyhow};
use bevy_asset::{AssetLoader, LoadContext, io::Reader};
use bevy_ecs::prelude::*;
use bevy_reflect::TypePath;
use wasmtime::{
    component::{Component, InstancePre, Val},
    error::Context,
};
use wasvy_runtime::{
    access::ModAccess,
    asset::{ModAsset, ModBackend},
};

use crate::{
    engine::{Engine, Linker},
    host::{WasmApp, WasmHost},
    runner::{Config, ConfigSetup, Runner},
    system::AddSystems,
};

/// Wasmtime-backed implementation for a loaded WebAssembly mod.
pub struct WasmModBackend {
    instance_pre: InstancePre<WasmHost>,
}

impl WasmModBackend {
    pub async fn new(loader: &ModAssetLoader, reader: &mut dyn Reader) -> Result<Self> {
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).await?;

        let component = Component::from_binary(loader.linker.engine(), &bytes)?;
        let instance_pre = loader.linker.instantiate_pre(&component)?;

        Ok(Self { instance_pre })
    }
}

impl ModBackend for WasmModBackend {
    fn setup(
        &self,
        world: &mut World,
        mod_id: Entity,
        mod_name: &str,
        accesses: &[ModAccess],
    ) -> Result<()> {
        let engine = world
            .get_resource::<Engine>()
            .expect("Engine should never be removed from world");

        let mut runner = Runner::new(engine);

        let mut systems = AddSystems::default();
        let config = Config::Setup(ConfigSetup {
            world,
            add_systems: &mut systems,
        });

        let app = runner.new_resource(WasmApp).expect("Table has space left");
        call(
            &mut runner,
            &self.instance_pre,
            config,
            "setup",
            &[Val::Resource(app)],
            &mut [],
        )?;

        systems.add_systems(
            world,
            accesses,
            runner.table(),
            mod_id,
            mod_name,
            &self.instance_pre,
        )
    }
}

pub(crate) fn call(
    runner: &mut Runner,
    instance_pre: &InstancePre<WasmHost>,
    config: Config,
    name: &str,
    params: &[Val],
    results: &mut [Val],
) -> Result<()> {
    runner.use_store(config, move |mut store| {
        let instance = instance_pre
            .instantiate(&mut store)
            .context("Failed to instantiate component")?;

        let func = instance
            .get_func(&mut store, name)
            .ok_or(anyhow!("Missing {name} function"))?;

        func.call(&mut store, params, results)
            .context("Failed to run the desired wasm function")?;

        Ok(())
    })
}

/// The Bevy [`AssetLoader`] for WebAssembly-backed [`ModAsset`] values.
#[derive(TypePath)]
pub struct ModAssetLoader {
    pub(crate) linker: Linker,
}

impl ModAssetLoader {
    pub fn new(linker: Linker) -> Self {
        Self { linker }
    }
}

impl AssetLoader for ModAssetLoader {
    type Asset = ModAsset;
    type Settings = ();
    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset> {
        Ok(ModAsset::new(WasmModBackend::new(self, reader).await?))
    }

    fn extensions(&self) -> &[&str] {
        &["wasm"]
    }
}
