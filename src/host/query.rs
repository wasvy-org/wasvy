use anyhow::{Result, bail};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::HostQuery,
    host::{Component, WasmHost},
};

pub struct Query;

impl HostQuery for WasmHost {
    fn iter(&mut self, __self: Resource<Query>) -> Result<Option<Vec<Resource<Component>>>> {
        bail!("Unimplemented")
    }

    fn drop(&mut self, _rep: Resource<Query>) -> Result<()> {
        Ok(())
    }
}
