use crate::{
    bindings::wasvy::ecs::app::*,
    runner::{Data, State},
};

mod app;
mod commands;
mod component;
mod query;
mod system;

pub use app::*;
pub use commands::*;
pub use component::*;
pub use query::*;
pub use system::*;

use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

pub struct WasmHost {
    data: Data,
    table: ResourceTable,
    ctx: WasiCtx,
}

impl WasmHost {
    pub(crate) fn new() -> Self {
        let data = Data::uninitialized();
        let table = ResourceTable::new();
        let ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_network()
            .allow_ip_name_lookup(true)
            .build();

        Self { data, table, ctx }
    }

    pub(crate) fn set_data(&mut self, data: Data) {
        self.data = data;
    }

    pub(crate) fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    pub(crate) fn clear(&mut self) {
        self.set_data(Data::uninitialized());
        self.table = ResourceTable::new();
    }

    /// Access to the data contained in the [`WasmHost`]
    pub(crate) fn access(&mut self) -> State<'_> {
        let table = &mut self.table;
        self.data.access(table).expect("WasmHost to be initialized")
    }
}

impl Host for WasmHost {}

impl WasiView for WasmHost {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}
