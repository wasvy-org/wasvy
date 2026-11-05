use anyhow::{Result, anyhow};
use bevy_asset::{AssetLoader, LoadContext, io::Reader};
use bevy_reflect::TypePath;
use wasmtime::{
    component::{Component, InstancePre, Val},
    error::Context,
};
use wasvy_runtime::asset::{ModAsset, ModDiscovery, ModLoaderSettings};

use crate::{
    engine::Linker,
    host::{WasmApp, WasmHost},
    runner::{Config, ConfigSetup, Runner},
    system::AddSystems,
};

fn discover(instance_pre: &InstancePre<WasmHost>) -> ModDiscovery {
    let mut runner = Runner::new(instance_pre.engine());

    let mut systems = AddSystems::default();
    let config = Config::Setup(ConfigSetup {
        add_systems: &mut systems,
    });

    let mut errors = error_collection::Errors::new();
    let Some(app) = errors.collect(runner.new_resource(WasmApp)) else {
        return ModDiscovery {
            errors,
            ..Default::default()
        };
    };
    errors.append(call(
        &mut runner,
        instance_pre,
        config,
        "setup",
        &[Val::Resource(app)],
        &mut [],
    ));

    let mut discovery = systems.definitions(runner.table(), instance_pre);
    discovery.errors.append(errors);
    discovery
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
        let instance = Context::context(
            instance_pre.instantiate(&mut store),
            "Failed to instantiate component",
        )?;

        let func = instance
            .get_func(&mut store, name)
            .ok_or(anyhow!("Missing {name} function"))?;

        Context::context(
            func.call(&mut store, params, results),
            "Failed to run the desired wasm function",
        )?;

        Ok(())
    })
}

/// A Bevy [`AssetLoader`] for wasm files resolving to a [`ModAsset`]s values.
#[derive(TypePath)]
pub struct WasmAssetLoader {
    pub(crate) linker: Linker,
}

impl WasmAssetLoader {
    pub fn new(linker: Linker) -> Self {
        Self { linker }
    }
}

impl AssetLoader for WasmAssetLoader {
    type Asset = ModAsset;
    type Settings = ModLoaderSettings;
    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset> {
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).await?;
        let component = Component::from_binary(self.linker.engine(), &bytes)?;
        let instance_pre = self.linker.instantiate_pre(&component)?;
        let namespace = load_context
            .path()
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("Mod asset requires a UTF-8 file stem"))?;
        Ok(ModAsset::new(namespace.to_owned(), discover(&instance_pre))
            .with_source_retention()
            .with_settings(settings.clone()))
    }

    fn extensions(&self) -> &[&str] {
        &["wasm"]
    }
}

#[cfg(test)]
mod tests;
