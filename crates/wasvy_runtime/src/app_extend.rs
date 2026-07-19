use std::any::type_name;

use anyhow::Result;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

#[cfg(feature = "devtools")]
use bevy_remote::{BrpError, RemoteMethodSystemId, RemoteMethods};
#[cfg(feature = "devtools")]
use serde_json::Value;

pub trait AppExtend {
    /// Adds the given plugin if it doesn't exist in the app yet
    fn try_add_plugin<T>(&mut self, plugin: T) -> &mut Self
    where
        T: Plugin;

    /// Adds a method to bevy remote
    #[cfg(feature = "devtools")]
    fn add_remote<M>(
        &mut self,
        method_name: impl Into<String>,
        handler: impl IntoSystem<In<Option<Value>>, Result<Value>, M> + 'static,
    ) -> &mut Self;

    /// Retrieves a registered plugin
    ///
    /// Panic if the plugin is not added
    fn plugin<T>(&self) -> &T
    where
        T: Plugin;
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

    #[cfg(feature = "devtools")]
    fn add_remote<M>(
        &mut self,
        method_name: impl Into<String>,
        handler: impl IntoSystem<In<Option<Value>>, Result<Value>, M> + 'static,
    ) -> &mut Self {
        // Map anyhow::Result to BrpResult
        let handler =
            handler.pipe(|In(result): In<Result<Value>>| result.map_err(BrpError::internal));

        // Add handler to remote methods
        let system_id = self.world_mut().register_system(handler);
        self.world_mut()
            .resource_mut::<RemoteMethods>()
            .insert(method_name, RemoteMethodSystemId::Instant(system_id));

        self
    }

    fn plugin<T>(&self) -> &T
    where
        T: Plugin,
    {
        match self.get_added_plugins::<T>().into_iter().next() {
            Some(plugin) => plugin,
            None => panic!("Expected plugin \"{}\" to be added.", type_name::<T>()),
        }
    }
}
