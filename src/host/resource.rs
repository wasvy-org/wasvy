use anyhow::{Result, bail};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostWorldResource, SerializedComponent},
    host::WasmHost,
    resource::{ResourceId, get_resource, set_resource},
    runner::State,
};

pub struct WasmResource {
    id: ResourceId,
}

impl WasmResource {
    pub(crate) fn new(id: ResourceId) -> Self {
        Self { id }
    }
}

impl HostWorldResource for WasmHost {
    fn get(&mut self, resource: Resource<WasmResource>) -> Result<SerializedComponent> {
        let State::RunSystem {
            table,
            resources,
            resource_resolver,
            type_registry,
            codec,
            ..
        } = self.access()
        else {
            bail!("Resource can only be accessed in systems")
        };

        let resource = table.get(&resource)?;
        let descriptor = resource_resolver.get(resource.id)?;
        get_resource(resources, descriptor.resource(), type_registry, codec)
    }

    fn set(&mut self, resource: Resource<WasmResource>, value: SerializedComponent) -> Result<()> {
        let State::RunSystem {
            table,
            resources,
            resource_resolver,
            type_registry,
            codec,
            ..
        } = self.access()
        else {
            bail!("Resource can only be accessed in systems")
        };

        let resource = table.get(&resource)?;
        let descriptor = resource_resolver.get(resource.id)?;
        if !descriptor.mutable() {
            bail!("Resource is not mutable")
        }
        set_resource(
            resources,
            descriptor.resource(),
            value,
            type_registry,
            codec,
        )
    }

    fn drop(&mut self, resource: Resource<WasmResource>) -> Result<()> {
        let _ = self.table().delete(resource)?;
        Ok(())
    }
}
