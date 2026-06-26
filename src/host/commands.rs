use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{Bundle, HostCommands},
    entity::{insert, map_entity, spawn_empty},
    host::{WasmEntity, WasmEntityCommands, WasmHost},
};

pub struct WasmCommands;

impl HostCommands for WasmHost {
    fn spawn_empty(
        &mut self,
        _: Resource<WasmCommands>,
    ) -> Result<Resource<WasmEntityCommands>, wasmtime::Error> {
        spawn_empty(self)
    }

    fn spawn(
        &mut self,
        _: Resource<WasmCommands>,
        bundle: Bundle,
    ) -> Result<Resource<WasmEntityCommands>, wasmtime::Error> {
        let entity_commands = spawn_empty(self)?;
        insert(self, &entity_commands, bundle)?;
        Ok(entity_commands)
    }

    fn entity(
        &mut self,
        _: Resource<WasmCommands>,
        entity: Resource<WasmEntity>,
    ) -> Result<Resource<WasmEntityCommands>, wasmtime::Error> {
        map_entity(self, entity)
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, commands: Resource<WasmCommands>) -> Result<(), wasmtime::Error> {
        let _ = self.table().delete(commands)?;

        Ok(())
    }
}
