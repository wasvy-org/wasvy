use crate::{
    bindings::wasvy::ecs::app::HostSerialize,
    host::WasmHost,
    runner::State,
    serialize::{WasvyCodec, WasvyCodecImpl},
};
use anyhow::{Result, bail};
use wasmtime::component::Resource;

#[derive(Default)]
pub struct WasmSerialize;

impl HostSerialize for WasmHost {
    fn get_type(&mut self, _self: Resource<WasmSerialize>) -> Result<String> {
        return Ok(WasvyCodec::get_type());
    }

    fn drop(&mut self, serialize: Resource<WasmSerialize>) -> Result<()> {
        let _ = self.table().delete(serialize)?;
        Ok(())
    }

    fn new(&mut self) -> Result<Resource<WasmSerialize>> {
        let State::RunSystem { table, .. } = self.access() else {
            bail!("Serialize can only be instantiated in system")
        };

        Ok(table.push(WasmSerialize::default())?)
    }
}
