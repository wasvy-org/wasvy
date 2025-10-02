use anyhow::{Result, bail};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostComponent, SerializedComponent},
    host::WasmHost,
};

pub struct Component;

impl HostComponent for WasmHost {
    fn get(&mut self, _self: Resource<Component>) -> Result<SerializedComponent> {
        bail!("Unimplemented")
    }

    fn set(&mut self, _self: Resource<Component>, _value: SerializedComponent) -> Result<()> {
        bail!("Unimplemented")
    }

    fn drop(&mut self, _rep: Resource<Component>) -> Result<()> {
        Ok(())
    }
}
