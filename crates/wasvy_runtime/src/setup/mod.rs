use bevy_asset::{AssetEvent, AssetServer, Assets};
use bevy_ecs::{
    prelude::*,
    system::{SystemParam, SystemState},
};

use crate::{asset::ModAsset, mods::Mod, schedule::ModStartup, system::ModSystemAsset};

mod install;
use install::{Installed, remove_orphaned_installations, update_installation};

mod publish;
use publish::Publications;

#[cfg(test)]
mod tests;

pub(crate) fn run_setup(
    mut publications: Local<Publications>,
    world: &mut World,
    params: &mut SystemState<SetupParams>,
) {
    let mut params = params.get_mut(world).expect("valid setup parameters");
    publications.track_asset_loads(params.events.read());
    let active = publications.remove_unused_sources(&params.mods);
    publications.publish_assets(&mut params, &active);
    let mods: Vec<_> = params.mods.iter().map(|(entity, ..)| entity).collect();
    let orphaned: Vec<_> = params.orphaned.iter().collect();
    remove_orphaned_installations(world, orphaned);

    let mut additions = Vec::new();
    for entity in mods {
        update_installation(world, entity, &publications, &mut additions);
    }

    let startup = additions
        .iter()
        .any(|copy| copy.schedule == bevy_ecs::schedule::ScheduleLabel::intern(&ModStartup));
    for copy in additions {
        world
            .resource_mut::<Schedules>()
            .add_systems(copy.schedule, copy.config);
    }
    if startup {
        ModStartup::run(world);
    }
}

#[derive(SystemParam)]
pub(crate) struct SetupParams<'w, 's> {
    events: MessageReader<'w, 's, AssetEvent<ModAsset>>,
    assets: ResMut<'w, Assets<ModAsset>>,
    systems: ResMut<'w, Assets<ModSystemAsset>>,
    server: Option<Res<'w, AssetServer>>,
    mods: Query<'w, 's, (Entity, &'static Mod, Option<&'static Installed>)>,
    orphaned: Query<'w, 's, Entity, (With<Installed>, Without<Mod>)>,
}
