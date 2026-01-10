use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use bevy_log::prelude::*;
use wasmtime::component::Resource;

use crate::{
    access::ModAccess, bindings::wasvy::ecs::app::HostCommands, cleanup::DespawnModEntity,
    component::insert_component, host::WasmHost, runner::State,
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
            insert_despawn_component,
            ..
        } = self.access()
        else {
            bail!("commands resource is only accessible when running systems")
        };

        let mut entity_commands = commands.spawn_empty();

        // Make sure the entity is not spawned outside the sandbox
        // The mod can still override the ChildOf with its own value
        // Note: We can't currently prevent a mod from creating a component that has a relation to a component outside the sandbox
        // TODO: Restrict what entities a mod can reference via permissions
        if let ModAccess::Sandbox(entity) = access {
            entity_commands.insert(ChildOf(*entity));
        };

        // Make sure this entity is despawned when the mod is despawned. See [ModDespawnBehaviour]
        if let Some(mod_id) = insert_despawn_component.0 {
            entity_commands.insert(DespawnModEntity(mod_id));
        }

        let entity = entity_commands.id();
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
