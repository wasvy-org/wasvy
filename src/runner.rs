use std::ptr::NonNull;

use anyhow::Result;
use bevy_ecs::{prelude::*, reflect::AppTypeRegistry, world::FilteredEntityMut};
use wasmtime::component::ResourceAny;
use wasmtime_wasi::ResourceTable;

use crate::{
    access::ModAccess,
    cleanup::InsertDespawnComponent,
    component::WasmComponentRegistry,
    engine::Engine,
    host::WasmHost,
    query::{Queries, QueryResolver},
    send_sync_ptr::SendSyncPtr,
    system::AddSystems,
};

pub(crate) type Store = wasmtime::Store<WasmHost>;

/// Used to contruct a [`Store`] in order to run mods
pub(crate) struct Runner {
    store: Store,
}

impl Runner {
    pub(crate) fn new(engine: &Engine) -> Self {
        let host = WasmHost::new();
        let store = Store::new(&engine.inner(), host);

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

    pub(crate) fn use_store<'a, 'b, 'c, 'd, 'e, 'f, 'g, F, R>(
        &mut self,
        config: Config<'a, 'b, 'c, 'd, 'e, 'f, 'g>,
        mut f: F,
    ) -> R
    where
        F: FnMut(&mut Store) -> R,
    {
        self.store.data_mut().set_data(Data(match config {
            Config::Setup(ConfigSetup {
                world,
                add_systems: systems,
            }) => Inner::Setup {
                world: SendSyncPtr::new(world.into()),
                add_systems: SendSyncPtr::new(systems.into()),
            },
            Config::RunSystem(ConfigRunSystem {
                commands,
                type_registry,
                wasm_registry,
                queries,
                query_resolver,
                access,
                insert_despawn_component,
            }) => Inner::RunSystem {
                commands: SendSyncPtr::new(NonNull::from_mut(commands).cast()),
                type_registry: SendSyncPtr::new(NonNull::from_ref(type_registry)),
                wasm_registry: SendSyncPtr::new(NonNull::from_ref(wasm_registry)),
                queries: SendSyncPtr::new(NonNull::from_ref(queries).cast()),
                query_resolver: SendSyncPtr::new(NonNull::from_ref(query_resolver)),
                access,
                insert_despawn_component,
            },
        }));

        let ret = f(&mut self.store);

        // Avoid storing invalid pointers in WasmHost data (such as ConfigSetup::schedules) which have a lifetime of 'a
        // If we didn't reset the data before this function returns, Data::access could access an invalid ref
        self.store.data_mut().clear();

        ret
    }
}

/// Data stored in [`WasmHost`]
pub(crate) struct Data(Inner);

enum Inner {
    Uninitialized,
    Setup {
        world: SendSyncPtr<World>,
        add_systems: SendSyncPtr<AddSystems>,
    },
    RunSystem {
        commands: SendSyncPtr<Commands<'static, 'static>>,
        type_registry: SendSyncPtr<AppTypeRegistry>,
        wasm_registry: SendSyncPtr<WasmComponentRegistry>,
        queries: SendSyncPtr<Queries<'static, 'static>>,
        query_resolver: SendSyncPtr<QueryResolver>,
        access: ModAccess,
        insert_despawn_component: InsertDespawnComponent,
    },
}

impl Data {
    pub(crate) fn uninitialized() -> Self {
        Self(Inner::Uninitialized)
    }

    /// A helper so [`WasmHost`] can expose access to the [`Data`] it stores
    ///
    /// The resource table from the host is passed through this for convenience
    pub(crate) fn access<'a>(&'a mut self, table: &'a mut ResourceTable) -> Option<State<'a>> {
        match &mut self.0 {
            Inner::Setup {
                world,
                add_systems: systems,
            } => Some(State::Setup {
                // Safety: Runner::use_store ensures that this always contains a valid reference
                // See the rules here: https://doc.rust-lang.org/stable/core/ptr/index.html#pointer-to-reference-conversion
                world: unsafe { world.as_mut() },
                table,
                add_systems: unsafe { systems.as_mut() },
            }),
            Inner::RunSystem {
                commands,
                type_registry,
                wasm_registry,
                queries,
                query_resolver,
                access,
                insert_despawn_component,
            } =>
            // Safety: Runner::use_store ensures that this always contains a valid reference
            // See the rules here: https://doc.rust-lang.org/stable/core/ptr/index.html#pointer-to-reference-conversion
            unsafe {
                Some(State::RunSystem {
                    commands: commands.cast().as_mut(),
                    type_registry: type_registry.as_ref(),
                    wasm_registry: wasm_registry.as_ref(),
                    queries: queries.cast().as_mut(),
                    query_resolver: query_resolver.as_ref(),
                    insert_despawn_component,
                    access,
                    table,
                })
            },
            Inner::Uninitialized => None,
        }
    }
}

pub(crate) enum State<'a> {
    Setup {
        world: &'a mut World,
        table: &'a mut ResourceTable,
        add_systems: &'a mut AddSystems,
    },
    RunSystem {
        table: &'a mut ResourceTable,
        commands: &'a mut Commands<'a, 'a>,
        type_registry: &'a AppTypeRegistry,
        wasm_registry: &'a WasmComponentRegistry,
        queries: &'a mut Queries<'a, 'a>,
        query_resolver: &'a QueryResolver,
        access: &'a ModAccess,
        insert_despawn_component: &'a InsertDespawnComponent,
    },
}

pub(crate) enum Config<'a, 'b, 'c, 'd, 'e, 'f, 'g> {
    Setup(ConfigSetup<'a>),
    RunSystem(ConfigRunSystem<'a, 'b, 'c, 'd, 'e, 'f, 'g>),
}

pub(crate) struct ConfigSetup<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) add_systems: &'a mut AddSystems,
}

pub(crate) struct ConfigRunSystem<'a, 'b, 'c, 'd, 'e, 'f, 'g> {
    pub(crate) commands: &'a mut Commands<'b, 'c>,
    pub(crate) type_registry: &'a AppTypeRegistry,
    pub(crate) wasm_registry: &'a WasmComponentRegistry,
    pub(crate) queries:
        &'a mut ParamSet<'d, 'e, Vec<Query<'f, 'g, FilteredEntityMut<'static, 'static>>>>,
    pub(crate) query_resolver: &'a QueryResolver,
    pub(crate) access: ModAccess,
    pub(crate) insert_despawn_component: InsertDespawnComponent,
}
