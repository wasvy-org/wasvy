use anyhow::Result;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_remote::{
    BrpError, RemoteMethodSystemId, RemoteMethods, RemotePlugin, http::RemoteHttpPlugin,
};
use serde_json::Value;

use crate::devtools::{Config, DevtoolsPlugin, mods::*};

impl Plugin for DevtoolsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .try_add_plugin(RemotePlugin::default())
            .try_add_plugin(RemoteHttpPlugin::default());
    }

    fn finish(&self, app: &mut App) {
        app.add_remote("wasvy/metadata", metadata)
            .add_remote("wasvy/mods/list", list)
            .add_remote("wasvy/mods/spawn", spawn)
            .add_remote("wasvy/mods/despawn", despawn);
    }
}

pub fn metadata(_: In<Option<Value>>, config: Res<Config>) -> Result<Value> {
    Ok(serde_json::to_value(&*config)?)
}

pub trait AppExtend {
    fn try_add_plugin<T>(&mut self, plugins: T) -> &mut Self
    where
        T: Plugin;

    fn add_remote<M>(
        &mut self,
        method_name: impl Into<String>,
        handler: impl IntoSystem<In<Option<Value>>, Result<Value>, M> + 'static,
    ) -> &mut Self;
}

impl AppExtend for App {
    fn try_add_plugin<T>(&mut self, plugins: T) -> &mut Self
    where
        T: Plugin,
    {
        if !self.is_plugin_added::<T>() {
            self.add_plugins(plugins);
        }
        self
    }

    fn add_remote<M>(
        &mut self,
        method_name: impl Into<String>,
        handler: impl IntoSystem<In<Option<Value>>, Result<Value>, M> + 'static,
    ) -> &mut Self {
        // Map anyhow::Result to BrpResult
        let handler = handler.pipe(|In(result): In<Result<Value>>| {
            result.map_err(|error| BrpError::internal(error))
        });

        // Add handler to remote methods
        let system_id = self.world_mut().register_system(handler);
        self.world_mut()
            .resource_mut::<RemoteMethods>()
            .insert(method_name, RemoteMethodSystemId::Instant(system_id));

        self
    }
}
