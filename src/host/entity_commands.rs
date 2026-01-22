use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{Bundle, BundleTypes, HostEntityCommands},
    entity::{insert, map_entity, remove},
    host::{WasmEntity, WasmHost},
    runner::State,
};

pub struct WasmEntityCommands(pub(crate) Entity);

impl From<&WasmEntityCommands> for Entity {
    fn from(value: &WasmEntityCommands) -> Self {
        value.0
    }
}

impl From<Entity> for WasmEntityCommands {
    fn from(value: Entity) -> Self {
        Self(value)
    }
}

impl HostEntityCommands for WasmHost {
    fn id(
        &mut self,
        entity_commands: Resource<WasmEntityCommands>,
    ) -> Result<Resource<WasmEntity>> {
        map_entity(self, entity_commands)
    }

    fn insert(
        &mut self,
        entity_commands: Resource<WasmEntityCommands>,
        bundle: Bundle,
    ) -> Result<()> {
        insert(self, &entity_commands, bundle)
    }

    fn remove(
        &mut self,
        entity_commands: Resource<WasmEntityCommands>,
        bundle: BundleTypes,
    ) -> Result<()> {
        remove(self, entity_commands, bundle)
    }

    fn despawn(&mut self, entity_commands: Resource<WasmEntityCommands>) -> Result<()> {
        let mut entity_commands = access(self, entity_commands)?;
        entity_commands.despawn();

        Ok(())
    }

    fn try_despawn(&mut self, entity_commands: Resource<WasmEntityCommands>) -> Result<()> {
        let mut entity_commands = access(self, entity_commands)?;
        entity_commands.try_despawn();

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, entity_commands: Resource<WasmEntityCommands>) -> Result<()> {
        let _ = self.table().delete(entity_commands)?;

        Ok(())
    }
}

fn access(
    host: &mut WasmHost,
    entity_commands: Resource<WasmEntityCommands>,
) -> Result<EntityCommands<'_>> {
    let State::RunSystem {
        table, commands, ..
    } = host.access()
    else {
        bail!("EntityCommands resource is only accessible when running systems")
    };

    let entity_commands = table.get(&entity_commands)?;
    Ok(commands.entity(entity_commands.0))
}
