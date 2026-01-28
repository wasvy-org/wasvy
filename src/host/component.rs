use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{ComponentIndex, HostComponent, SerializedComponent},
    component::{with_component_mut, with_component_ref},
    host::WasmHost,
    methods::{MethodTarget},
    query::QueryId,
    runner::State,
};

/// Host-side handle for a WIT `component` resource.
///
/// This stores the query index + entity so dynamic method dispatch can resolve
/// the underlying Bevy component.
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

    fn invoke(
        &mut self,
        component: Resource<WasmComponent>,
        method: String,
        params: SerializedComponent,
    ) -> Result<SerializedComponent> {
        invoke_component_method(self, component, &method, &params)
    }
}

/// Invoke a reflected component method using JSON-encoded arguments.
///
/// This is used by the auto-generated host bindings to implement WIT methods.
pub fn invoke_component_method(
    host: &mut WasmHost,
    component: Resource<WasmComponent>,
    method: &str,
    params: &str,
) -> Result<SerializedComponent> {
    let State::RunSystem {
        table,
        queries,
        query_resolver,
        type_registry,
        method_registry,
        ..
    } = host.access()
    else {
        bail!("Component can only be accessed in systems")
    };

    let component = table.get(&component)?;
    let query_for = query_resolver.query_for(component.id, component.index)?;
    let component_ref = query_for.component();

    let mut query = queries.get_mut(component.id.index());
    let output = if query_for.mutable() {
        let mut entity = query.get_mut(component.entity)?;
        with_component_mut(&mut entity, component_ref, type_registry, |reflect| {
            method_registry.invoke(
                component_ref.type_path(),
                method,
                MethodTarget::Write(reflect),
                params,
            )
        })?
    } else {
        let entity = query.get(component.entity)?;
        with_component_ref(&entity, component_ref, type_registry, |reflect| {
            method_registry.invoke(
                component_ref.type_path(),
                method,
                MethodTarget::Read(reflect),
                params,
            )
        })?
    };

    Ok(output)
}
