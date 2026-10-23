use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostSystem, QueryFor},
    host::WasmHost,
    runner::State,
    system::Param,
};

pub struct WasmSystem {
    pub(crate) name: String,
    pub(crate) params: Vec<Param>,
    pub(crate) after: Vec<u32>,
    pub(crate) before: Vec<u32>,
}

impl WasmSystem {
    fn new(name: String) -> Self {
        Self {
            name,
            params: Vec::new(),
            after: Vec::new(),
            before: Vec::new(),
        }
    }

    fn add_param(
        host: &mut WasmHost,
        system: Resource<WasmSystem>,
        param: Param,
    ) -> Result<(), wasmtime::Error> {
        let State::Setup { table, .. } = host.access() else {
            return Err(wasmtime::Error::msg(
                "Systems can only be modified in a setup function",
            ));
        };

        let system = table.get_mut(&system)?;
        system.params.push(param);

        Ok(())
    }
}

impl HostSystem for WasmHost {
    fn new(&mut self, name: String) -> Result<Resource<WasmSystem>, wasmtime::Error> {
        let State::Setup { table, .. } = self.access() else {
            return Err(wasmtime::Error::msg(
                "Systems can only be instantiated in a setup function",
            ));
        };

        let system = table.push(WasmSystem::new(name))?;
        Ok(system)
    }

    fn add_commands(
        &mut self,
        system: Resource<WasmSystem>,
    ) -> std::result::Result<(), wasmtime::Error> {
        WasmSystem::add_param(self, system, Param::Commands)
    }

    fn add_query(
        &mut self,
        system: Resource<WasmSystem>,
        query: Vec<QueryFor>,
    ) -> std::result::Result<(), wasmtime::Error> {
        WasmSystem::add_param(self, system, Param::Query(query))
    }

    fn after(
        &mut self,
        system: Resource<WasmSystem>,
        other: Resource<WasmSystem>,
    ) -> std::result::Result<(), wasmtime::Error> {
        let State::Setup { table, .. } = self.access() else {
            return Err(wasmtime::Error::msg(
                "Systems can only be modified in a setup function",
            ));
        };

        table.get(&other)?;
        let other = other.rep();
        let system = table.get_mut(&system)?;

        system.after.push(other);

        Ok(())
    }

    fn before(
        &mut self,
        system: Resource<WasmSystem>,
        other: Resource<WasmSystem>,
    ) -> std::result::Result<(), wasmtime::Error> {
        let State::Setup { table, .. } = self.access() else {
            return Err(wasmtime::Error::msg(
                "Systems can only be modified in a setup function",
            ));
        };

        table.get(&other)?;
        let other = other.rep();
        let system = table.get_mut(&system)?;

        system.before.push(other);

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, _: Resource<WasmSystem>) -> std::result::Result<(), wasmtime::Error> {
        // Don't drop! After running setup, wasvy will find and register all
        // [WasmSystems] via [AddSystems]. If they are dropped, they will not be
        // in the wasm resource table when [AddSystems::definitions] is called.

        Ok(())
    }
}
