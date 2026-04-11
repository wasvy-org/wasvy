use wasmtime::component::Resource;
use anyhow::{Result, bail};
use crate::{bindings::wasvy::ecs::app::{HostResource, SerializedResource}, host::WasmHost};

pub struct WasmResource;

impl HostResource for WasmHost {
    #[doc = " Gets the value of a resource"]
    fn get(
        &mut self,
        resource: Resource<WasmResource>,
    ) -> Result<SerializedResource> {
        todo!()
    }

    #[doc = " Sets the value of a resource"]
    fn set(
        &mut self,
        resource: Resource<WasmResource>,
        value: SerializedResource,
    ) -> Result<()> {
        todo!()
    }

    fn drop(&mut self, resource: Resource<WasmResource>) -> wasmtime::Result<()> {
        let _ = self.table().delete(resource)?;

        Ok(())
    }
}
