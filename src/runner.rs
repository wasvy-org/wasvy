use std::ptr::NonNull;

use anyhow::Result;
use bevy::{
    asset::AssetId,
    ecs::{
        component::Tick,
        entity::Entity,
        reflect::AppTypeRegistry,
        system::{Commands, ParamSet, Query},
        world::{FilteredEntityMut, World},
    },
};
use wasmtime::component::ResourceAny;
use wasmtime_wasi::ResourceTable;

use crate::{asset::ModAsset, engine::Engine, host::WasmHost, send_sync_ptr::SendSyncPtr};

pub(crate) type Store = wasmtime::Store<WasmHost>;

/// Used to contruct a [`Store`] in order to run mods
pub(crate) struct Runner {
    store: Store,
}

impl Runner {
    pub(crate) fn new(engine: &Engine) -> Self {
        let host = WasmHost::new();
        let store = Store::new(&engine, host);

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
                asset_id,
                asset_version,
                mod_id,
                mod_name,
                sandbox_entities,
            }) => Inner::Setup {
                world: SendSyncPtr::new(world.into()),
                app_init: false,
                asset_id: *asset_id,
                asset_version,
                mod_id,
                mod_name: mod_name.to_string(),
                sandbox_entities: SendSyncPtr::new(sandbox_entities.into()),
            },
            Config::RunSystem(ConfigRunSystem {
                commands,
                type_registry,
                queries,
            }) => Inner::RunSystem {
                commands: SendSyncPtr::new(NonNull::from_mut(commands).cast()),
                type_registry: SendSyncPtr::new(NonNull::from_ref(type_registry)),
                queries: SendSyncPtr::new(NonNull::from_ref(queries).cast()),
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
        app_init: bool,
        mod_id: Entity,
        mod_name: String,
        asset_id: AssetId<ModAsset>,
        asset_version: Tick,
        sandbox_entities: SendSyncPtr<[Entity]>,
    },
    RunSystem {
        commands: SendSyncPtr<Commands<'static, 'static>>,
        type_registry: SendSyncPtr<AppTypeRegistry>,
        queries: SendSyncPtr<Queries<'static, 'static>>,
    },
}

type Queries<'w, 's> =
    ParamSet<'w, 's, Vec<Query<'static, 'static, FilteredEntityMut<'static, 'static>>>>;

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
                app_init,
                asset_id,
                asset_version,
                mod_id,
                mod_name,
                sandbox_entities,
            } => Some(State::Setup {
                // Safety: Runner::use_store ensures that this always contains a valid reference
                // See the rules here: https://doc.rust-lang.org/stable/core/ptr/index.html#pointer-to-reference-conversion
                world: unsafe { world.as_mut() },
                app_init,
                asset_id,
                asset_version,
                mod_id: *mod_id,
                mod_name,
                sandbox_entities: unsafe { sandbox_entities.as_ref() },
                table,
            }),
            Inner::RunSystem {
                commands,
                type_registry,
                queries,
            } =>
            // Safety: Runner::use_store ensures that this always contains a valid reference
            // See the rules here: https://doc.rust-lang.org/stable/core/ptr/index.html#pointer-to-reference-conversion
            unsafe {
                Some(State::RunSystem {
                    commands: commands.cast().as_mut(),
                    type_registry: type_registry.as_ref(),
                    queries: queries.cast().as_mut(),
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
        app_init: &'a mut bool,
        mod_id: Entity,
        mod_name: &'a str,
        asset_id: &'a AssetId<ModAsset>,
        asset_version: &'a Tick,
        sandbox_entities: &'a [Entity],
    },
    RunSystem {
        table: &'a mut ResourceTable,
        commands: &'a mut Commands<'a, 'a>,
        type_registry: &'a AppTypeRegistry,
        queries: &'a mut Queries<'a, 'a>,
    },
}

pub(crate) enum Config<'a, 'b, 'c, 'd, 'e, 'f, 'g> {
    Setup(ConfigSetup<'a>),
    RunSystem(ConfigRunSystem<'a, 'b, 'c, 'd, 'e, 'f, 'g>),
}

pub(crate) struct ConfigSetup<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) asset_id: &'a AssetId<ModAsset>,
    pub(crate) asset_version: Tick,
    pub(crate) mod_id: Entity,
    pub(crate) mod_name: &'a str,
    pub(crate) sandbox_entities: &'a [Entity],
}

pub(crate) struct ConfigRunSystem<'a, 'b, 'c, 'd, 'e, 'f, 'g> {
    pub(crate) commands: &'a mut Commands<'b, 'c>,
    pub(crate) type_registry: &'a AppTypeRegistry,
    pub(crate) queries:
        &'a mut ParamSet<'d, 'e, Vec<Query<'f, 'g, FilteredEntityMut<'static, 'static>>>>,
}
