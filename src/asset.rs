use anyhow::{Context, Result, anyhow};
use bevy_asset::{Asset, AssetId, AssetLoader, Assets, LoadContext, io::Reader};
use bevy_ecs::{change_detection::Tick, prelude::*};
use bevy_reflect::TypePath;
use wasmtime::component::{Component, InstancePre, Val};

use crate::{
    access::ModAccess,
    cleanup::DespawnModEntities,
    engine::{Engine, Linker},
    host::{App, WasmHost},
    mods::ModDespawnBehaviour,
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
        accesses: &[ModAccess],
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

        // This is very cheap, since it's just Arcs
        let instance_pre = asset.instance_pre.clone();

        // The mod might have reloaded. It's necessary we perform cleanup
        // if the mod has spawned entities before.
        if ModDespawnBehaviour::should_despawn_entities(world) {
            let (entities, mut commands) = world.entities_and_commands();
            let despawn = entities
                .get(mod_id)
                .expect("Mod entity exists")
                .get::<DespawnModEntities>()
                .expect(
                    "DespawnModEntities should have been registered as a required componet for Mod",
                );
            for source_entity in despawn.iter() {
                commands.entity(source_entity).try_despawn();
            }
        }

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
            accesses,
        });

        let app = runner.new_resource(App).expect("Table has space left");
        Some(call(
            &mut runner,
            &instance_pre,
            config,
            SETUP,
            &[Val::Resource(app)],
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
#[derive(TypePath)]
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
