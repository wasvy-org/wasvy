use std::mem::replace;

use anyhow::{Context, Result, anyhow, bail};
use bevy::{
    asset::{Asset, AssetId, AssetLoader, LoadContext, io::Reader},
    ecs::{component::Tick, entity::Entity, world::World},
    reflect::TypePath,
};
use wasmtime::component::{Component, InstancePre, Val};

use crate::{
    engine::{Engine, Linker},
    host::WasmHost,
    runner::{Config, ConfigRunSystem, ConfigSetup, Runner},
};

/// An asset representing a loaded wasvy Mod
#[derive(Asset, TypePath)]
pub struct ModAsset(Inner);

enum Inner {
    /// See [ModAsset::take]
    Placeholder,

    /// The asset is loaded, but the [SETUP] function has **not** yet run
    Loaded { instance_pre: InstancePre<WasmHost> },

    /// The asset is loaded and the [SETUP] function has been run
    Initiated {
        version: Tick,
        instance_pre: InstancePre<WasmHost>,
    },
}

const SETUP: &'static str = "setup";

impl ModAsset {
    pub(crate) async fn new(loader: &ModAssetLoader, reader: &mut dyn Reader) -> Result<Self> {
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).await?;

        let component = Component::from_binary(&loader.linker.engine(), &bytes)?;
        let instance_pre = loader.linker.instantiate_pre(&component)?;

        Ok(Self(Inner::Loaded { instance_pre }))
    }

    pub(crate) fn version(&self) -> Tick {
        match &self.0 {
            Inner::Initiated { version, .. } => version.clone(),
            _ => Tick::MAX,
        }
    }

    /// Take ownership of this asset and leave a placeholder behind
    pub(crate) fn take(&mut self) -> Self {
        replace(self, Self(Inner::Placeholder))
    }

    /// Replace this asset with another
    pub(crate) fn put(&mut self, value: Self) {
        let _ = replace(self, value);
    }

    /// Initiates mods by running their "setup" function
    pub(crate) fn initiate(
        self,
        world: &mut World,
        asset_id: &AssetId<ModAsset>,
        mod_name: &str,
        sandbox_entities: &[Entity],
    ) -> Result<Self> {
        let instance_pre = match self.0 {
            Inner::Loaded { instance_pre } => instance_pre,
            Inner::Initiated { instance_pre, .. } => instance_pre,
            Inner::Placeholder => unreachable!(),
        };

        // Assign a version based on the world tick
        // This is useful for `system_runner`s to know they should no longer run
        let asset_version = world.change_tick();

        let engine = world
            .get_resource::<Engine>()
            .expect("Engine should never be removed from world");

        let mut runner = Runner::new(&engine);

        let config = Config::Setup(ConfigSetup {
            world,
            asset_id,
            asset_version,
            mod_name,
            sandbox_entities,
        });
        call(&mut runner, &instance_pre, config, SETUP, &[], &mut [])?;

        Ok(Self(Inner::Initiated {
            version: asset_version,
            instance_pre,
        }))
    }

    pub(crate) fn run_system<'a, 'b, 'c, 'd, 'e, 'f, 'g>(
        &self,
        runner: &mut Runner,
        name: &str,
        config: ConfigRunSystem<'a, 'b, 'c, 'd, 'e, 'f, 'g>,
        params: &[Val],
    ) -> Result<()> {
        let Inner::Initiated { instance_pre, .. } = &self.0 else {
            bail!("Mod is not in Ready state");
        };

        let config = Config::RunSystem(config);
        call(runner, instance_pre, config, name, params, &mut [])
    }
}

fn call(
    runner: &mut Runner,
    instance_pre: &InstancePre<WasmHost>,
    config: Config,
    name: &str,
    params: &[Val],
    mut results: &mut [Val],
) -> Result<()> {
    runner.use_store(config, move |mut store| {
        let instance = instance_pre
            .instantiate(&mut store)
            .context("Failed to instantiate component")?;

        let func = instance
            .get_func(&mut store, name)
            .ok_or(anyhow!("Missing {name} function"))?;

        func.call(&mut store, params, &mut results)
            .context("Failed to run the desired wasm function")?;

        Ok(())
    })
}

/// The bevy [`AssetLoader`] for [`ModAsset`]
pub struct ModAssetLoader {
    pub(crate) linker: Linker,
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
        let asset = ModAsset::new(self, reader).await?;

        Ok(asset)
    }

    fn extensions(&self) -> &[&str] {
        &["wasm"]
    }
}
