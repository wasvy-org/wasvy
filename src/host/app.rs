use anyhow::{Result, bail};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostApp, Schedule, SerializedResource},
    host::{WasmHost, WasmSystem},
    resource::WasmResource,
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

    #[doc = " Inserts a new resource to the mod"]
    fn insert_resource(
        &mut self,
        _: Resource<WasmApp>,
        resource: (String, SerializedResource),
    ) -> wasmtime::Result<()> {
        let State::Setup {
            insert_resources, ..
        } = self.access()
        else {
            bail!("App can only be modified in a setup function")
        };

        insert_resources.push((
            resource.0,
            WasmResource {
                serialized_value: resource.1,
            },
        ));
        Ok(())
    }
}
