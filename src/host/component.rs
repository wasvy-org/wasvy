use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{ComponentIndex, HostComponent, SerializedComponent},
    host::WasmHost,
    query::QueryId,
    runner::State,
};

pub struct WasmComponent {
    index: ComponentIndex,
    id: QueryId,
    entity: Entity,
}

impl WasmComponent {
    pub(crate) fn new(index: ComponentIndex, id: QueryId, entity: Entity) -> Self {
        Self { index, id, entity }
    }
}

impl HostComponent for WasmHost {
    fn get(&mut self, component: Resource<WasmComponent>) -> Result<SerializedComponent> {
        let State::RunSystem {
            table,
            queries,
            query_resolver,
            type_registry,
            ..
        } = self.access()
        else {
            bail!("Component can only be accessed in systems")
        };

        let component = table.get(&component)?;
        query_resolver.get(
            component.id,
            component.entity,
            component.index,
            queries,
            type_registry,
        )
    }

    fn set(
        &mut self,
        component: Resource<WasmComponent>,
        value: SerializedComponent,
    ) -> Result<()> {
        let State::RunSystem {
            table,
            queries,
            query_resolver,
            type_registry,
            ..
        } = self.access()
        else {
            bail!("Component can only be accessed in systems")
        };

        let component = table.get(&component)?;
        query_resolver.set(
            component.id,
            component.entity,
            component.index,
            value,
            queries,
            type_registry,
        )
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, component: Resource<WasmComponent>) -> Result<()> {
        let _ = self.table().delete(component)?;

        Ok(())
    }
}
