use std::ptr::NonNull;

use anyhow::Result;
use bevy_ecs::reflect::AppTypeRegistry;
use fragile::Fragile;
use wasmtime::component::ResourceAny;
use wasmtime_wasi::ResourceTable;
use wasvy_runtime::{
    commands::RuntimeCommands, component::WasmComponentRegistry, prelude::FunctionIndex,
    serialize::CodecResource,
};

use crate::{
    host::WasmHost,
    query::{Queries, QueryResolver},
    system::{AddSystems, RunSystemParams},
};

pub(crate) type Store = wasmtime::Store<WasmHost>;

/// Used to contruct a [`Store`] in order to run mods
pub(crate) struct Runner {
    store: Store,
}

impl Runner {
    pub(crate) fn new(engine: &wasmtime::Engine) -> Self {
        let host = WasmHost::new();
        let store = Store::new(engine, host);

        Self { store }
    }

    pub fn table(&mut self) -> &mut ResourceTable {
        self.store.data_mut().table()
    }

    pub(crate) fn new_resource<T>(&mut self, entry: T) -> Result<ResourceAny>
    where
        T: Send + 'static,
    {
        let resource = self.table().push(entry)?;
        Ok(resource.try_into_resource_any(&mut self.store)?)
    }

    pub(crate) fn use_store<F, R>(&mut self, mut config: Config<'_, '_, '_>, mut f: F) -> R
    where
        F: FnMut(&mut Store) -> R,
    {
        self.store.data_mut().set_data(Data(Some(Fragile::new(
            NonNull::from_mut(&mut config).cast(),
        ))));

        let ret = f(&mut self.store);

        // Avoid storing a dangling pointer to `config` in WasmHost data
        // If we didn't reset the data before this function returns, Data::access could access an invalid ref
        self.store.data_mut().clear();

        ret
    }
}

/// Data stored in [`WasmHost`]
///
/// Holds a type-erased pointer to the [`Config`] passed to [`Runner::use_store`].
/// [`Fragile`] ensures it is only ever accessed from the thread that created it.
#[derive(Default)]
pub(crate) struct Data(Option<Fragile<NonNull<Config<'static, 'static, 'static>>>>);

impl Data {
    /// A helper so [`WasmHost`] can expose access to the [`Data`] it stores
    ///
    /// The resource table from the host is passed through this for convenience
    pub(crate) fn access<'a>(&'a mut self, table: &'a mut ResourceTable) -> Option<State<'a>> {
        let ptr = self.0.as_mut()?.get_mut();

        // Safety: Runner::use_store ensures that this always points to a valid Config,
        // and the returned borrows cannot outlive `self`, which is cleared before the Config is dropped.
        // See the rules here: https://doc.rust-lang.org/stable/core/ptr/index.html#pointer-to-reference-conversion
        let config = unsafe { ptr.cast::<Config<'a, 'a, 'a>>().as_mut() };

        Some(match config {
            Config::Setup(ConfigSetup { add_systems }) => State::Setup { table, add_systems },
            Config::RunSystem(ConfigRunSystem {
                params,
                query_resolver,
            }) => State::RunSystem {
                table,
                commands: &mut params.commands,
                type_registry: &params.type_registry,
                codec: &params.codec,
                wasm_registry: &params.wasm_registry,
                function_index: &params.function_index,
                queries: &mut params.queries,
                query_resolver,
            },
        })
    }
}

pub(crate) enum State<'a> {
    Setup {
        table: &'a mut ResourceTable,
        add_systems: &'a mut AddSystems,
    },
    RunSystem {
        table: &'a mut ResourceTable,
        commands: &'a mut RuntimeCommands<'a, 'a>,
        type_registry: &'a AppTypeRegistry,
        codec: &'a CodecResource,
        wasm_registry: &'a WasmComponentRegistry,
        function_index: &'a FunctionIndex,
        queries: &'a mut Queries<'a, 'a>,
        query_resolver: &'a QueryResolver,
    },
}

pub(crate) enum Config<'a, 'w, 's> {
    Setup(ConfigSetup<'a>),
    RunSystem(ConfigRunSystem<'a, 'w, 's>),
}

pub(crate) struct ConfigSetup<'a> {
    pub(crate) add_systems: &'a mut AddSystems,
}

pub(crate) struct ConfigRunSystem<'a, 'w, 's> {
    pub(crate) params: &'a mut RunSystemParams<'w, 's>,
    pub(crate) query_resolver: &'a QueryResolver,
}
