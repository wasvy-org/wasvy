use anyhow::{Result, bail};
use bevy::{ecs::hierarchy::ChildOf, log::trace};
use wasmtime::component::Resource;

use crate::{
    access::ModAccess, bindings::wasvy::ecs::app::HostCommands, component::insert_component,
    host::WasmHost, runner::State,
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
            access,
            ..
        } = self.access()
        else {
            bail!("commands resource is only accessible when running systems")
        };

        // Make sure the entity is not spawned outside the sandbox
        // The mod can still override the ChildOf with its own value
        // Note: We can't currently prevent a mod from creating a component that has a relation to a component outside the sadnbox
        // TODO: Restrict what entities a mod can reference via permissions
        let entity = if let ModAccess::Sandbox(entity) = access {
            commands.spawn(ChildOf(*entity)).id()
        } else {
            commands.spawn_empty().id()
        };

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

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, commands: Resource<Commands>) -> Result<()> {
        let _ = self.table().delete(commands)?;

        Ok(())
    }
}
