use anyhow::{Result, bail};
use bevy::log::trace;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::HostCommands, component::insert_component, host::WasmHost,
    runner::State,
};

pub struct Commands;

impl HostCommands for WasmHost {
    fn spawn(
        &mut self,
        _self: Resource<Commands>,
        components: Vec<(String, String)>,
    ) -> Result<()> {
        let State::RunSystem {
            mut commands,
            type_registry,
            ..
        } = self.access()
        else {
            bail!("commands resource is only accessible when running systems")
        };

        let entity = commands.spawn_empty().id();
        trace!("Spawn empty {entity}, with components:");

        for (type_path, serialized_component) in components {
            trace!("- {type_path}: {serialized_component}");
            insert_component(
                &mut commands,
                type_registry,
                entity,
                type_path,
                serialized_component,
            )?;
        }

        Ok(())
    }

    fn drop(&mut self, _rep: Resource<Commands>) -> Result<()> {
        Ok(())
    }
}
