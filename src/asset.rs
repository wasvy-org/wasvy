use anyhow::{Context, Result, anyhow};
use bevy::{
    asset::{Asset, AssetId, AssetLoader, Assets, LoadContext, io::Reader},
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
pub struct ModAsset {
    version: Option<Tick>,
    instance_pre: InstancePre<WasmHost>,
}

const SETUP: &'static str = "setup";

impl ModAsset {
    pub(crate) async fn new(loader: &ModAssetLoader, reader: &mut dyn Reader) -> Result<Self> {
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).await?;

        let component = Component::from_binary(&loader.linker.engine(), &bytes)?;
        let instance_pre = loader.linker.instantiate_pre(&component)?;

        Ok(Self {
            version: None,
            instance_pre,
        })
    }

    pub(crate) fn version(&self) -> Option<Tick> {
        self.version
    }

    /// Initiates mods by running their "setup" function
    ///
    /// Returns [None] if the mod could not be initialized because the asset is missing.
    pub(crate) fn initiate(
        world: &mut World,
        asset_id: &AssetId<ModAsset>,
        mod_id: Entity,
        mod_name: &str,
        sandbox_entities: &[Entity],
    ) -> Option<Result<()>> {
        let change_tick = world.change_tick();

        let mut assets = world
            .get_resource_mut::<Assets<Self>>()
            .expect("ModAssets be registered");

        // Will return None if the asset is not yet loaded
        // run_setup will re-run initiate when it is finally loaded
        let Some(asset) = assets.get_mut(*asset_id) else {
            return None;
        };

        // Gets the version of this asset or assign a new one if it doesn't exist yet
        let asset_version = match asset.version {
            Some(version) => version,
            None => {
                asset.version = Some(change_tick);
                change_tick
            }
        };

        // This is very cheap, since it's all Arcs
        let instance_pre = asset.instance_pre.clone();

        let engine = world
            .get_resource::<Engine>()
            .expect("Engine should never be removed from world");

        let mut runner = Runner::new(&engine);

        let config = Config::Setup(ConfigSetup {
            world,
            asset_id,
            asset_version,
            mod_id,
            mod_name,
            sandbox_entities,
        });

        Some(call(
            &mut runner,
            &instance_pre,
            config,
            SETUP,
            &[],
            &mut [],
        ))
    }

    pub(crate) fn run_system<'a, 'b, 'c, 'd, 'e, 'f, 'g>(
        &self,
        runner: &mut Runner,
        name: &str,
        config: ConfigRunSystem<'a, 'b, 'c, 'd, 'e, 'f, 'g>,
        params: &[Val],
    ) -> Result<()> {
        let config = Config::RunSystem(config);
        call(runner, &self.instance_pre, config, name, params, &mut [])
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
