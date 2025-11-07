use bevy::{
    ecs::system::{SystemParam, SystemState},
    platform::collections::HashSet,
    prelude::*,
};

use crate::{asset::ModAsset, mods::Mod, schedule::ModStartup};

/// Group all the system params we neeed to allow shared access from one &mut world
#[derive(SystemParam)]
pub(crate) struct Setup<'w, 's> {
    events: MessageReader<'w, 's, AssetEvent<ModAsset>>,
    mods: Query<'w, 's, (Entity, Ref<'static, Mod>, Option<&'static Name>)>,
}

#[derive(PartialEq, Eq, Hash)]
pub(crate) struct RanFor {
    mod_id: Entity,
    sandbox_id: Entity,
}

pub(crate) fn run_setup(
    mut world: &mut World,
    param: &mut SystemState<Setup>,
    mut ran_for: Local<HashSet<RanFor>>,
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
        for sandbox in mod_component.into_inner().sandboxes() {
            ran_for.remove(&RanFor {
                mod_id,
                sandbox_id: *sandbox,
            });
        }

        loaded_mods.push(mod_id);
    }

    // We need exclusive world access later in order to setup mods, so store refs to them in a vec while we still have access to the Setup system params
    let mut setup = Vec::new();
    for (mod_id, mod_component, name) in mods.iter().filter(|(mod_id, mod_component, _)| {
        // We only need to setup mods that have changed (such as sandboxes were added) or those that have loaded
        mod_component.is_changed() || loaded_mods.contains(mod_id)
    }) {
        let asset_id = mod_component.asset().id();

        let name = name
            .map(|name| name.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Each mod needs to be setup for all the sandboxes it is running in
        let sandboxes: Vec<Entity> = mod_component
            .into_inner()
            .sandboxes()
            .map(|entity| *entity)
            // Skip mods that have already been setup before
            .filter(|entity| {
                !ran_for.contains(&RanFor {
                    mod_id,
                    sandbox_id: *entity,
                })
            })
            .collect();

        if !sandboxes.is_empty() {
            setup.push((asset_id, mod_id, name, sandboxes));
        }
    }

    // Initiate mods with exclusive world access (runs the mod setup)
    let mut run_starup_schedule = false;
    for (asset_id, mod_id, name, sandboxed_entities) in setup {
        match ModAsset::initiate(
            &mut world,
            &asset_id,
            mod_id,
            &name,
            &sandboxed_entities[..],
        ) {
            None => {
                info!("Loading mod \"{}\"", name);
            }
            Some(Ok(())) => {
                info!("Successfully initialized mod \"{}\"", name);

                run_starup_schedule = true;

                for sandbox_id in sandboxed_entities {
                    ran_for.insert(RanFor { mod_id, sandbox_id });
                }
            }
            Some(Err(err)) => {
                error!("Error initializing mod \"{}\":\n{:?}", name, err);

                // Remove placeholder asset and the entity holding a handle to it
                world
                    .get_resource_mut::<Assets<ModAsset>>()
                    .expect("ModAssets be registered")
                    .remove(asset_id);
                world.despawn(mod_id);
            }
        }
    }

    if run_starup_schedule {
        ModStartup::run(world);
    }
}
