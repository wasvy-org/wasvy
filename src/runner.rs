use std::ptr::NonNull;

use anyhow::Result;
use bevy_ecs::{prelude::*, reflect::AppTypeRegistry, world::FilteredResourcesMut};
use wasmtime::component::ResourceAny;
use wasmtime_wasi::ResourceTable;

use crate::{
    access::ModAccess,
    cleanup::InsertDespawnComponent,
    component::WasmComponentRegistry,
    engine::Engine,
    host::WasmHost,
    methods::FunctionIndex,
    query::{Queries, QueryResolver},
    resource::ResourceResolver,
    send_sync_ptr::SendSyncPtr,
    serialize::CodecResource,
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
        let store = Store::new(engine.inner(), host);

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
        resource.try_into_resource_any(&mut self.store)
    }

    pub(crate) fn use_store<'a, 'b, 'c, F, R>(&mut self, config: Config<'a, 'b, 'c>, mut f: F) -> R
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
            Config::Init(ConfigInit {
                commands,
                type_registry,
                codec,
                wasm_registry,
                access,
                insert_despawn_component,
            }) => Inner::Init {
                commands: SendSyncPtr::new(NonNull::from_mut(commands).cast()),
                type_registry: SendSyncPtr::new(NonNull::from_ref(type_registry)),
                codec: SendSyncPtr::new(NonNull::from_ref(codec)),
                wasm_registry: SendSyncPtr::new(NonNull::from_ref(wasm_registry)),
                access,
                insert_despawn_component,
            },
            Config::RunSystem(ConfigRunSystem {
                commands,
                type_registry,
                codec,
                wasm_registry,
                function_index,
                queries,
                query_resolver,
                resources,
                resource_resolver,
                access,
                insert_despawn_component,
            }) => Inner::RunSystem {
                commands,
                type_registry: SendSyncPtr::new(NonNull::from_ref(type_registry)),
                codec: SendSyncPtr::new(NonNull::from_ref(codec)),
                wasm_registry: SendSyncPtr::new(NonNull::from_ref(wasm_registry)),
                function_index: SendSyncPtr::new(NonNull::from_ref(function_index)),
                queries,
                query_resolver: SendSyncPtr::new(NonNull::from_ref(query_resolver)),
                resources,
                resource_resolver: SendSyncPtr::new(NonNull::from_ref(resource_resolver)),
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
    Init {
        commands: SendSyncPtr<()>,
        type_registry: SendSyncPtr<AppTypeRegistry>,
        codec: SendSyncPtr<CodecResource>,
        wasm_registry: SendSyncPtr<WasmComponentRegistry>,
        access: ModAccess,
        insert_despawn_component: InsertDespawnComponent,
    },
    RunSystem {
        commands: SendSyncPtr<()>,
        type_registry: SendSyncPtr<AppTypeRegistry>,
        codec: SendSyncPtr<CodecResource>,
        wasm_registry: SendSyncPtr<WasmComponentRegistry>,
        function_index: SendSyncPtr<FunctionIndex>,
        queries: SendSyncPtr<()>,
        query_resolver: SendSyncPtr<QueryResolver>,
        resources: SendSyncPtr<()>,
        resource_resolver: SendSyncPtr<ResourceResolver>,
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
            Inner::Init {
                commands,
                type_registry,
                codec,
                wasm_registry,
                access,
                insert_despawn_component,
            } => unsafe {
                Some(State::Init {
                    commands: commands.cast::<Commands<'static, 'static>>().as_mut(),
                    type_registry: type_registry.as_ref(),
                    codec: codec.as_ref(),
                    wasm_registry: wasm_registry.as_ref(),
                    insert_despawn_component,
                    access,
                    table,
                })
            },
            Inner::RunSystem {
                commands,
                type_registry,
                codec,
                wasm_registry,
                function_index,
                queries,
                query_resolver,
                resources,
                resource_resolver,
                access,
                insert_despawn_component,
            } =>
            // Safety: Runner::use_store ensures that this always contains a valid reference
            // See the rules here: https://doc.rust-lang.org/stable/core/ptr/index.html#pointer-to-reference-conversion
            unsafe {
                Some(State::RunSystem {
                    commands: commands.cast::<Commands<'static, 'static>>().as_mut(),
                    type_registry: type_registry.as_ref(),
                    codec: codec.as_ref(),
                    wasm_registry: wasm_registry.as_ref(),
                    function_index: function_index.as_ref(),
                    queries: queries.cast::<Queries<'static, 'static>>().as_mut(),
                    query_resolver: query_resolver.as_ref(),
                    resources: resources
                        .cast::<FilteredResourcesMut<'static, 'static>>()
                        .as_mut(),
                    resource_resolver: resource_resolver.as_ref(),
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
    Init {
        table: &'a mut ResourceTable,
        commands: &'a mut Commands<'a, 'a>,
        type_registry: &'a AppTypeRegistry,
        codec: &'a CodecResource,
        wasm_registry: &'a WasmComponentRegistry,
        access: &'a ModAccess,
        insert_despawn_component: &'a InsertDespawnComponent,
    },
    RunSystem {
        table: &'a mut ResourceTable,
        commands: &'a mut Commands<'a, 'a>,
        type_registry: &'a AppTypeRegistry,
        codec: &'a CodecResource,
        wasm_registry: &'a WasmComponentRegistry,
        function_index: &'a FunctionIndex,
        queries: &'a mut Queries<'a, 'a>,
        query_resolver: &'a QueryResolver,
        resources: &'a mut FilteredResourcesMut<'a, 'a>,
        resource_resolver: &'a ResourceResolver,
        access: &'a ModAccess,
        insert_despawn_component: &'a InsertDespawnComponent,
    },
}

pub(crate) enum Config<'a, 'b, 'c> {
    Setup(ConfigSetup<'a>),
    Init(ConfigInit<'a, 'b, 'c>),
    RunSystem(ConfigRunSystem<'a>),
}

pub(crate) struct ConfigSetup<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) add_systems: &'a mut AddSystems,
}

pub(crate) struct ConfigInit<'a, 'b, 'c> {
    pub(crate) commands: &'a mut Commands<'b, 'c>,
    pub(crate) type_registry: &'a AppTypeRegistry,
    pub(crate) codec: &'a CodecResource,
    pub(crate) wasm_registry: &'a WasmComponentRegistry,
    pub(crate) access: ModAccess,
    pub(crate) insert_despawn_component: InsertDespawnComponent,
}

pub(crate) struct ConfigRunSystem<'a> {
    pub(crate) commands: SendSyncPtr<()>,
    pub(crate) type_registry: &'a AppTypeRegistry,
    pub(crate) codec: &'a CodecResource,
    pub(crate) wasm_registry: &'a WasmComponentRegistry,
    pub(crate) function_index: &'a FunctionIndex,
    pub(crate) queries: SendSyncPtr<()>,
    pub(crate) query_resolver: &'a QueryResolver,
    pub(crate) resources: SendSyncPtr<()>,
    pub(crate) resource_resolver: &'a ResourceResolver,
    pub(crate) access: ModAccess,
    pub(crate) insert_despawn_component: InsertDespawnComponent,
}
