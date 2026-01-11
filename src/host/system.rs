use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostSystem, QueryFor},
    host::WasmHost,
    runner::State,
    system::{DynamicSystemId, Param},
};

pub struct WasmSystem {
    pub(crate) id: DynamicSystemId,
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) after: Vec<DynamicSystemId>,
}

impl WasmSystem {
    fn new(name: String, world: &mut World) -> Self {
        Self {
            id: DynamicSystemId::new(world),
            name,
            params: Vec::new(),
            after: Vec::new(),
        }
    }

    fn add_param(host: &mut WasmHost, system: Resource<WasmSystem>, param: Param) -> Result<()> {
        let State::Setup { table, .. } = host.access() else {
            bail!("Systems can only be modified in a setup function")
        };

        let system = table.get_mut(&system)?;
        system.params.push(param);

        Ok(())
    }
}

impl HostSystem for WasmHost {
    fn new(&mut self, name: String) -> Result<Resource<WasmSystem>> {
        let State::Setup { table, world, .. } = self.access() else {
            bail!("Systems can only be instantiated in a setup function")
        };

        Ok(table.push(WasmSystem::new(name, world))?)
    }

    fn add_commands(&mut self, system: Resource<WasmSystem>) -> Result<()> {
        WasmSystem::add_param(self, system, Param::Commands)
    }

    fn add_query(&mut self, system: Resource<WasmSystem>, query: Vec<QueryFor>) -> Result<()> {
        WasmSystem::add_param(self, system, Param::Query(query))
    }

    fn after(&mut self, system: Resource<WasmSystem>, other: Resource<WasmSystem>) -> Result<()> {
        let State::Setup { table, .. } = self.access() else {
            bail!("Systems can only be modified in a setup function")
        };

        let other = table.get(&other)?.id;
        let system = table.get_mut(&system)?;

        system.after.push(other);

        Ok(())
    }

    fn before(&mut self, system: Resource<WasmSystem>, other: Resource<WasmSystem>) -> Result<()> {
        // In bevy, `a.before(b)` is logically equivalent to `b.after(a)`
        HostSystem::after(self, other, system)
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, _: Resource<WasmSystem>) -> Result<()> {
        // Don't drop! After running setup, wasvy will find and register all WasmSystems
        // via WasmSystemParent and

        Ok(())
    }
}
