use bevy_asset::prelude::*;
use bevy_ecs::{
    prelude::*,
    system::{SystemParam, SystemState},
};
use bevy_log::prelude::*;
use bevy_platform::collections::HashSet;

use crate::{access::ModAccess, asset::ModAsset, mods::Mod, schedule::ModStartup};

/// Group all the system params we neeed to allow shared access from one &mut world
#[derive(SystemParam)]
pub(crate) struct Setup<'w, 's> {
    events: MessageReader<'w, 's, AssetEvent<ModAsset>>,
    mods: Query<'w, 's, (Entity, Ref<'static, Mod>, Option<&'static Name>)>,
}

#[derive(PartialEq, Eq, Hash)]
pub(crate) struct RanWith {
    mod_id: Entity,
    access: ModAccess,
}

pub(crate) fn run_setup(
    mut world: &mut World,
    param: &mut SystemState<Setup>,
    mut ran_with: Local<HashSet<RanWith>>,
) {
    let Setup { mut events, mods } = param.get_mut(world);

    // Mod ids who's asset has been loaded (or hot-reloaded)
    let mut loaded_mods = Vec::new();
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };

        // Find the mod entity matching this asset
        let Some((mod_id, mod_component, name)) =
            mods.iter().find(|(_, m, _)| m.asset().id() == *id)
        else {
            warn!(
                "Loaded wasm mod asset, but missing its entity. Did you accidentally load a wasm asset?"
            );
            continue;
        };

        let name = name
            .map(|name| name.as_str())
            .unwrap_or("unknown")
            .to_string();
        info!("Loaded mod \"{name}\"");

        // The mod must be setup again for all of its sandboxes
        for access in mod_component.into_inner().accesses().map(Clone::clone) {
            ran_with.remove(&RanWith { mod_id, access });
        }

        loaded_mods.push(mod_id);
    }

    // We need exclusive world access later in order to setup mods, so store refs to them in a vec while we still have access to the Setup system params
    let mut setup: Vec<(AssetId<ModAsset>, Entity, String, Vec<ModAccess>)> = Vec::new();
    for (mod_id, mod_component, name) in mods.iter().filter(|(mod_id, mod_component, _)| {
        // We only need to setup mods that have changed (such as sandboxes were added) or those that have loaded
        mod_component.is_changed() || loaded_mods.contains(mod_id)
    }) {
        let asset_id = mod_component.asset().id();

        let name = name
            .map(|name| name.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Each mod needs to be setup once for all its accesses
        let accesses: Vec<ModAccess> = mod_component
            .into_inner()
            .accesses()
            // Skip mods that have already been setup before
            .filter(|access| {
                !ran_with.contains(&RanWith {
                    mod_id,
                    access: **access,
                })
            })
            .map(Clone::clone)
            .collect();

        if !accesses.is_empty() {
            setup.push((asset_id, mod_id, name, accesses));
        }
    }

    // Initiate mods with exclusive world access (runs the mod setup)
    let mut run_startup_schedule = false;
    for (asset_id, mod_id, name, accesses) in setup {
        let Some(result) = ModAsset::initiate(&mut world, &asset_id, mod_id, &name, &accesses[..])
        else {
            continue;
        };

        for access in accesses {
            ran_with.insert(RanWith { mod_id, access });
        }

        if let Err(err) = result {
            error!("Error initializing mod \"{name}\":\n{err:?}");
        } else {
            info!("Successfully initialized mod \"{name}\"");

            run_startup_schedule = true;
        }
    }

    if run_startup_schedule {
        ModStartup::run(world);
    }
}
