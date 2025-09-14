use bevy::{ecs::system::SystemChangeTick, prelude::*};

use crate::{
    asset::ModAsset,
    engine::Engine,
    mods::Mod,
    runner::{ConfigSetup, Runner},
};

pub(crate) fn run_setup(
    tick: SystemChangeTick,
    mut events: MessageReader<AssetEvent<ModAsset>>,
    mut assets: ResMut<Assets<ModAsset>>,
    mut schedules: ResMut<Schedules>,
    engine: Res<Engine>,
    mut commands: Commands,
    mods: Query<(Entity, Option<&Name>, &Mod)>,
) {
    for event in events.read() {
        match event {
            AssetEvent::LoadedWithDependencies { id } => {
                let asset = assets.get_mut(*id).unwrap();

                // Find the mod entity matching this asset
                let Some((entity, name, _)) = mods.iter().find(|&(_, _, m)| m.asset.id() == *id)
                else {
                    warn!(
                        "Loaded wasm mod, but missing it's entity. Did you accidentally load a wasm asset?"
                    );
                    continue;
                };

                let name = name
                    .and_then(|name| Some(name.as_str()))
                    .unwrap_or("unknown");

                let asset_version = tick.this_run();
                asset.version = asset_version;

                let mut runner = Runner::new(&engine);
                match asset.setup(
                    &mut runner,
                    ConfigSetup {
                        schedules: &mut schedules,
                        asset_id: &id,
                        asset_version,
                        mod_name: &name,
                    },
                ) {
                    Ok(()) => info!("Successfully loaded mod \"{}\"", name),
                    Err(err) => {
                        commands.entity(entity).despawn();
                        error!("Error loading mod \"{}\":\n{:?}", name, err)
                    }
                }
            }
            _ => {}
        }
    }
}
