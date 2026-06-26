use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use super::*;

/// Host state for wit generation
///
/// This implements the Wasvy WIT host traits used by guest modules.
pub struct Host {
    pub table: ResourceTable,
    pub ctx: WasiCtx,
    pub systems: Vec<WasmSystem>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            ctx: WasiCtxBuilder::default()
                .inherit_stdout()
                .inherit_stderr()
                .build(),
            systems: Default::default(),
            table: Default::default(),
        }
    }
}

impl bindings::Host for Host {}

impl WasiView for Host {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl bindings::HostComponent for Host {
    fn get(
        &mut self,
        _: Resource<bindings::Component>,
    ) -> Result<bindings::SerializedComponent, wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Component::get"))
    }

    fn set(
        &mut self,
        _: Resource<bindings::Component>,
        _: bindings::SerializedComponent,
    ) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Component::set"))
    }

    fn drop(&mut self, _: Resource<bindings::Component>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Component::drop"))
    }

    fn invoke(
        &mut self,
        _: Resource<bindings::Component>,
        _: String,
        _: bindings::SerializedComponent,
    ) -> Result<bindings::SerializedComponent, wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Component::invoke"))
    }
}

impl bindings::HostCommands for Host {
    fn entity(
        &mut self,
        _: Resource<bindings::Commands>,
        _: Resource<bindings::Entity>,
    ) -> Result<Resource<bindings::EntityCommands>, wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Commands::entity"))
    }

    fn drop(&mut self, _: Resource<bindings::Commands>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Commands::drop"))
    }

    fn spawn(
        &mut self,
        _: Resource<bindings::Commands>,
        _: bindings::Bundle,
    ) -> Result<Resource<bindings::EntityCommands>, wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Commands::spawn"))
    }

    fn spawn_empty(
        &mut self,
        _: Resource<bindings::Commands>,
    ) -> Result<Resource<bindings::EntityCommands>, wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to Commands::spawn_empty",
        ))
    }
}

impl bindings::HostEntityCommands for Host {
    fn despawn(&mut self, _: Resource<bindings::EntityCommands>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to EntityCommands::despawn",
        ))
    }

    fn drop(&mut self, _: Resource<bindings::EntityCommands>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to EntityCommands::drop",
        ))
    }

    fn id(
        &mut self,
        _: Resource<bindings::EntityCommands>,
    ) -> Result<Resource<bindings::Entity>, wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to EntityCommands::id",
        ))
    }

    fn insert(
        &mut self,
        _: Resource<bindings::EntityCommands>,
        _: bindings::Bundle,
    ) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to EntityCommands::insert",
        ))
    }

    fn remove(
        &mut self,
        _: Resource<bindings::EntityCommands>,
        _: bindings::BundleTypes,
    ) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to EntityCommands::remove",
        ))
    }

    fn try_despawn(
        &mut self,
        _: Resource<bindings::EntityCommands>,
    ) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to EntityCommands::try_despawn",
        ))
    }
}

impl bindings::HostEntity for Host {
    fn drop(&mut self, _: Resource<bindings::Entity>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Entity::drop"))
    }
}

impl bindings::HostQueryResult for Host {
    fn component(
        &mut self,
        _: Resource<bindings::QueryResult>,
        _: bindings::ComponentIndex,
    ) -> Result<Resource<bindings::Component>, wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to QueryResult::component",
        ))
    }

    fn drop(&mut self, _: Resource<bindings::QueryResult>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to QueryResult::drop"))
    }

    fn entity(
        &mut self,
        _: Resource<bindings::QueryResult>,
    ) -> Result<Resource<bindings::Entity>, wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to QueryResult::entity",
        ))
    }
}

impl bindings::HostQuery for Host {
    fn drop(&mut self, _: Resource<bindings::Query>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Query::drop"))
    }

    fn iter(
        &mut self,
        _: Resource<bindings::Query>,
    ) -> Result<Option<Resource<bindings::QueryResult>>, wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Query::iter"))
    }
}

impl bindings::HostSerialize for Host {
    fn drop(&mut self, _: Resource<bindings::Serialize>) -> Result<(), wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Serialize::drop"))
    }

    fn get_type(&mut self, _: Resource<bindings::Serialize>) -> Result<String, wasmtime::Error> {
        Err(wasmtime::Error::msg(
            "Unexpected call to Serialize::get_type",
        ))
    }

    fn new(&mut self) -> Result<Resource<bindings::Serialize>, wasmtime::Error> {
        Err(wasmtime::Error::msg("Unexpected call to Serialize::new"))
    }
}
