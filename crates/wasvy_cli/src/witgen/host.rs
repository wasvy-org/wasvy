use anyhow::bail;
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
    fn get(&mut self, _: Resource<bindings::Component>) -> Result<bindings::SerializedComponent> {
        bail!("Unexpected call to Component::get");
    }

    fn set(
        &mut self,
        _: Resource<bindings::Component>,
        _: bindings::SerializedComponent,
    ) -> Result<()> {
        bail!("Unexpected call to Component::set");
    }

    fn drop(&mut self, _: Resource<bindings::Component>) -> Result<()> {
        bail!("Unexpected call to Component::drop");
    }

    fn invoke(
        &mut self,
        _: Resource<bindings::Component>,
        _: String,
        _: bindings::SerializedComponent,
    ) -> Result<bindings::SerializedComponent> {
        bail!("Unexpected call to Component::invoke");
    }
}

impl bindings::HostCommands for Host {
    fn entity(
        &mut self,
        _: Resource<bindings::Commands>,
        _: Resource<bindings::Entity>,
    ) -> Result<Resource<bindings::EntityCommands>> {
        bail!("Unexpected call to Commands::entity");
    }

    fn drop(&mut self, _: Resource<bindings::Commands>) -> Result<()> {
        bail!("Unexpected call to Commands::drop");
    }

    fn spawn(
        &mut self,
        _: Resource<bindings::Commands>,
        _: bindings::Bundle,
    ) -> Result<Resource<bindings::EntityCommands>> {
        bail!("Unexpected call to Commands::spawn");
    }

    fn spawn_empty(
        &mut self,
        _: Resource<bindings::Commands>,
    ) -> Result<Resource<bindings::EntityCommands>> {
        bail!("Unexpected call to Commands::spawn_empty");
    }
}

impl bindings::HostEntityCommands for Host {
    fn despawn(&mut self, _: Resource<bindings::EntityCommands>) -> Result<()> {
        bail!("Unexpected call to EntityCommands::despawn");
    }

    fn drop(&mut self, _: Resource<bindings::EntityCommands>) -> Result<()> {
        bail!("Unexpected call to EntityCommands::drop");
    }

    fn id(&mut self, _: Resource<bindings::EntityCommands>) -> Result<Resource<bindings::Entity>> {
        bail!("Unexpected call to EntityCommands::id");
    }

    fn insert(&mut self, _: Resource<bindings::EntityCommands>, _: bindings::Bundle) -> Result<()> {
        bail!("Unexpected call to EntityCommands::insert");
    }

    fn remove(
        &mut self,
        _: Resource<bindings::EntityCommands>,
        _: bindings::BundleTypes,
    ) -> Result<()> {
        bail!("Unexpected call to EntityCommands::remove");
    }

    fn try_despawn(&mut self, _: Resource<bindings::EntityCommands>) -> Result<()> {
        bail!("Unexpected call to EntityCommands::try_despawn");
    }
}

impl bindings::HostEntity for Host {
    fn drop(&mut self, _: Resource<bindings::Entity>) -> Result<()> {
        bail!("Unexpected call to Entity::drop");
    }
}

impl bindings::HostQueryResult for Host {
    fn component(
        &mut self,
        _: Resource<bindings::QueryResult>,
        _: bindings::ComponentIndex,
    ) -> Result<Resource<bindings::Component>> {
        bail!("Unexpected call to QueryResult::component");
    }

    fn drop(&mut self, _: Resource<bindings::QueryResult>) -> Result<()> {
        bail!("Unexpected call to QueryResult::drop");
    }

    fn entity(&mut self, _: Resource<bindings::QueryResult>) -> Result<Resource<bindings::Entity>> {
        bail!("Unexpected call to QueryResult::entity");
    }
}

impl bindings::HostQuery for Host {
    fn drop(&mut self, _: Resource<bindings::Query>) -> Result<()> {
        bail!("Unexpected call to Query::drop");
    }

    fn iter(
        &mut self,
        _: Resource<bindings::Query>,
    ) -> Result<Option<Resource<bindings::QueryResult>>> {
        bail!("Unexpected call to Query::iter");
    }
}

impl bindings::HostSerialize for Host {
    fn drop(&mut self, _: Resource<bindings::Serialize>) -> Result<()> {
        bail!("Unexpected call to Serialize::drop");
    }

    fn get_type(&mut self, _: Resource<bindings::Serialize>) -> Result<String> {
        bail!("Unexpected call to Serialize::get_type");
    }

    fn new(&mut self) -> Result<Resource<bindings::Serialize>> {
        bail!("Unexpected call to Serialize::new");
    }
}
