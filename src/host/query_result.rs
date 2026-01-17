use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{ComponentIndex, HostQueryResult},
    entity::map_entity,
    host::{WasmComponent, WasmEntity, WasmHost},
    query::QueryId,
    runner::State,
};

#[derive(Clone, Copy)]
pub struct WasmQueryResult {
    id: QueryId,
    entity: Entity,
}

impl WasmQueryResult {
    pub(crate) fn new(id: QueryId, entity: Entity) -> Self {
        Self { id, entity }
    }
}

impl Into<Entity> for &WasmQueryResult {
    fn into(self) -> Entity {
        self.entity
    }
}

impl HostQueryResult for WasmHost {
    fn entity(&mut self, query_result: Resource<WasmQueryResult>) -> Result<Resource<WasmEntity>> {
        map_entity(self, query_result)
    }

    fn component(
        &mut self,
        query_result: Resource<WasmQueryResult>,
        index: ComponentIndex,
    ) -> Result<Resource<WasmComponent>> {
        let State::RunSystem { table, .. } = self.access() else {
            bail!("QueryResult can only be accessed in systems")
        };

        let query_result = table.get(&query_result)?;
        let component = WasmComponent::new(index, query_result.id, query_result.entity);
        let component = table.push(component)?;
        Ok(component)
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, query_result: Resource<WasmQueryResult>) -> Result<()> {
        let _ = self.table().delete(query_result)?;

        Ok(())
    }
}
