use bevy::{
    ecs::system::{SystemParam, SystemState},
    prelude::*,
};

use crate::{asset::ModAsset, mods::Mod};

/// Group all the system params we neeed to allow shared access from one &mut world
#[derive(SystemParam)]
pub struct Setup<'w, 's> {
    events: MessageReader<'w, 's, AssetEvent<ModAsset>>,
    assets: ResMut<'w, Assets<ModAsset>>,
    mods: Query<'w, 's, (Entity, Option<&'static Name>, &'static Mod)>,
}

pub(crate) fn run_setup(mut world: &mut World, param: &mut SystemState<Setup>) {
    let Setup {
        mut events,
        mut assets,
        mods,
    } = param.get_mut(world);

    // We need exclusive world access in order to setup mods, so store them here
    let mut setup = Vec::new();

    for event in events.read() {
        // Load both new assets and hot-reloaded ones
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };

        let Some(asset) = assets.get_mut_untracked(*id).map(ModAsset::take) else {
            continue;
        };

        // Find the mod entity matching this asset
        let Some((entity, name, _)) = mods.iter().find(|&(_, _, m)| m.asset.id() == *id) else {
            warn!(
                "Loaded wasm mod asset, but missing its entity. Did you accidentally load a wasm asset?"
            );
            continue;
        };

        let name = name
            .map(|name| name.as_str())
            .unwrap_or("unknown")
            .to_string();

        setup.push((asset, *id, entity, name));
    }

    for (asset, asset_id, entity, name) in setup {
        // Setup mods with exclusive world access
        let result = asset.initiate(&mut world, &asset_id, &name);

        let Setup { mut assets, .. } = param.get_mut(world);
        match result {
            Ok(initiated_asset) => {
                info!("Successfully loaded mod \"{}\"", name);

                // Replace placeholder
                assets
                    .get_mut(asset_id)
                    .expect("asset placeholder not to have moved")
                    .put(initiated_asset);
            }
            Err(err) => {
                error!("Error loading mod \"{}\":\n{:?}", name, err);

                // Remove placeholder asset and the entity holding a handle to it
                assets.remove(asset_id);
                world.despawn(entity);
            }
        }
    }
}
