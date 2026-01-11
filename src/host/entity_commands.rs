use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{Bundle, BundleTypes, HostEntityCommands},
    entity::{FromEntity, ToEntity, insert, map_entity, remove},
    host::{WasmEntity, WasmHost},
    runner::State,
};

pub struct WasmEntityCommands {
    entity: Entity,
}

impl ToEntity for WasmEntityCommands {
    fn entity(&self) -> Entity {
        self.entity
    }
}

impl FromEntity for WasmEntityCommands {
    fn from_entity(entity: Entity) -> Self {
        Self { entity }
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
        let State::RunSystem {
            table, commands, ..
        } = self.access()
        else {
            bail!("EntityCommands resource is only accessible when running systems")
        };

        let entity_commands = table.get(&entity_commands)?;
        commands.entity(entity_commands.entity).despawn();

        Ok(())
    }

    fn try_despawn(&mut self, entity_commands: Resource<WasmEntityCommands>) -> Result<()> {
        let State::RunSystem {
            table, commands, ..
        } = self.access()
        else {
            bail!("EntityCommands resource is only accessible when running systems")
        };

        let entity_commands = table.get(&entity_commands)?;
        commands.entity(entity_commands.entity).try_despawn();

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, entity_commands: Resource<WasmEntityCommands>) -> Result<()> {
        let _ = self.table().delete(entity_commands)?;

        Ok(())
    }
}
