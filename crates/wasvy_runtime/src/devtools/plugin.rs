use bevy_app::prelude::*;
use bevy_remote::{RemotePlugin, http::RemoteHttpPlugin};

use crate::{
    app_extend::AppExtend,
    devtools::{DevtoolsPlugin, remote::*},
};

impl Plugin for DevtoolsPlugin {
    fn build(&self, app: &mut App) {
        app.try_add_plugin(RemotePlugin::default())
            .try_add_plugin(RemoteHttpPlugin::default());
    }

    fn finish(&self, app: &mut App) {
        let metadata_res = Metadata::new(self.0.clone(), app.plugin());
        app.insert_resource(metadata_res)
            .add_remote("wasvy.metadata", metadata)
            .add_remote("wasvy.mods.list", list)
            .add_remote("wasvy.mods.spawn", spawn)
            .add_remote("wasvy.mods.despawn", despawn)
            .add_remote("wasvy.mods.despawn_all", despawn_all);
    }
}
