use anyhow::Result;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{Bundle, HostCommands, SerializedComponent},
    entity::{insert, map_entity, spawn_empty},
    host::{WasmEntity, WasmEntityCommands, WasmHost},
    resource::{queue_insert_resource, queue_remove_resource},
    runner::State,
};

pub struct WasmCommands;

impl HostCommands for WasmHost {
    fn spawn_empty(&mut self, _: Resource<WasmCommands>) -> Result<Resource<WasmEntityCommands>> {
        spawn_empty(self)
    }

    fn spawn(
        &mut self,
        _: Resource<WasmCommands>,
        bundle: Bundle,
    ) -> Result<Resource<WasmEntityCommands>> {
        let entity_commands = spawn_empty(self)?;
        insert(self, &entity_commands, bundle)?;
        Ok(entity_commands)
    }

    fn insert_resource(
        &mut self,
        _: Resource<WasmCommands>,
        type_path: String,
        value: SerializedComponent,
    ) -> Result<()> {
        let (State::Init { commands, .. } | State::RunSystem { commands, .. }) = self.access()
        else {
            anyhow::bail!(
                "Commands resource can only queue resources during systems or first-load init"
            )
        };

        queue_insert_resource(commands, type_path, value);
        Ok(())
    }

    fn remove_resource(&mut self, _: Resource<WasmCommands>, type_path: String) -> Result<()> {
        let (State::Init { commands, .. } | State::RunSystem { commands, .. }) = self.access()
        else {
            anyhow::bail!(
                "Commands resource can only queue resources during systems or first-load init"
            )
        };

        queue_remove_resource(commands, type_path);
        Ok(())
    }

    fn entity(
        &mut self,
        _: Resource<WasmCommands>,
        entity: Resource<WasmEntity>,
    ) -> Result<Resource<WasmEntityCommands>> {
        map_entity(self, entity)
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, commands: Resource<WasmCommands>) -> Result<()> {
        let _ = self.table().delete(commands)?;

        Ok(())
    }
}
