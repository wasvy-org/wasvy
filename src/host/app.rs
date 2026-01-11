use anyhow::{Result, bail};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostApp, Schedule},
    host::{WasmHost, WasmSystem},
    runner::State,
};

pub struct WasmApp;

impl HostApp for WasmHost {
    fn add_systems(
        &mut self,
        _: Resource<WasmApp>,
        schedule: Schedule,
        systems: Vec<Resource<WasmSystem>>,
    ) -> Result<()> {
        if systems.is_empty() {
            return Ok(());
        }

        let State::Setup { add_systems, .. } = self.access() else {
            bail!("App can only be modified in a setup function")
        };

        add_systems.push(schedule, systems);

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, app: Resource<WasmApp>) -> Result<()> {
        let _ = self.table().delete(app)?;

        Ok(())
    }
}
