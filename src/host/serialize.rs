use crate::{bindings::wasvy::ecs::app::HostSerialize, host::WasmHost, runner::State};
use anyhow::{Result, bail};
use wasmtime::component::Resource;

#[derive(Default)]
pub struct WasmSerialize;

impl HostSerialize for WasmHost {
    fn get_type(&mut self, _: Resource<WasmSerialize>) -> Result<String> {
        let State::RunSystem { codec, .. } = self.access() else {
            bail!("Codec can only be instantiated in system")
        };
        Ok(codec.get_type())
    }

    fn drop(&mut self, serialize: Resource<WasmSerialize>) -> Result<()> {
        let _ = self.table().delete(serialize)?;
        Ok(())
    }

    fn new(&mut self) -> Result<Resource<WasmSerialize>> {
        let State::RunSystem { table, .. } = self.access() else {
            bail!("Serialize can only be instantiated in system")
        };

        Ok(table.push(WasmSerialize)?)
    }
}
