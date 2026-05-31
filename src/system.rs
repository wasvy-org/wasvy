use std::ptr::NonNull;

use anyhow::Result;
use bevy_asset::{AssetId, Assets};
use bevy_ecs::{
    change_detection::Tick,
    error::Result as BevyResult,
    prelude::*,
    resource::Resource as BevyResource,
    schedule::{ScheduleConfigs, ScheduleLabel},
    system::{BoxedSystem, Commands, LocalBuilder, ParamBuilder, ParamSetBuilder, Query},
    world::{FilteredEntityMut, FilteredResourcesMut},
};
use bevy_log::prelude::*;
use wasmtime::component::{InstancePre, Resource, Val};
use wasmtime_wasi::ResourceTable;

use crate::{
    access::ModAccess,
    asset::{ModAsset, run_system_instance_pre},
    bindings::wasvy::ecs::app::{QueryFor, Schedule},
    cleanup::InsertDespawnComponent,
    component::WasmComponentRegistry,
    engine::Engine,
    host::{WasmCommands, WasmQuery, WasmSystem},
    methods::FunctionIndex,
    mods::ModSystemSet,
    modules::{ModuleGeneration, ModuleId, ModuleSystemSet},
    query::{QueryId, QueryIdGenerator, QueryResolver, create_query_builder},
    resource::{ResourceId, ResourceIdGenerator, ResourceResolver, create_resource_builder},
    runner::{ConfigRunSystem, Runner},
    send_sync_ptr::SendSyncPtr,
    serialize::CodecResource,
};

/// A helper struct that stores dynamic systems that a mod would like to register.
///
/// Wasvy only registers systems after mod's setup method has successfully run.
#[derive(Default)]
pub(crate) struct AddSystems(Vec<(Schedule, Vec<Resource<WasmSystem>>)>);

#[derive(Default, Clone)]
pub(crate) struct PlannedSystems(Vec<(Schedule, Vec<WasmSystem>)>);

impl AddSystems {
    pub(crate) fn push(&mut self, schedule: Schedule, systems: Vec<Resource<WasmSystem>>) {
        self.0.push((schedule, systems));
    }

    pub(crate) fn add_systems(
        self,
        world: &mut World,
        accesses: &[ModAccess],
        table: &ResourceTable,
        mod_id: Entity,
        mod_name: &str,
        asset_id: &AssetId<ModAsset>,
        asset_version: &Tick,
    ) -> Result<()> {
        // Each access needs dedicated systems that run inside it
        for access in accesses {
            let mod_schedules = access.schedules(world);
            for (schedule, systems) in self.0.iter() {
                // Validate that the schedule requested by the mod is enabled
                let Some(schedule) = mod_schedules
                    .evaluate(schedule)
                    .map(|schedule| schedule.schedule_label())
                else {
                    warn!(
                        "Mod tried adding systems to schedule {schedule:?}, but that schedule is not enabled. See ModSchedules docs."
                    );
                    continue;
                };

                for system in systems
                    .iter()
                    .map(|system| table.get(system).expect("Resource not be dropped"))
                {
                    Self::add_system(
                        schedule,
                        system,
                        world,
                        mod_id,
                        mod_name,
                        asset_id,
                        asset_version,
                        access,
                    )?;
                }
            }
        }

        Ok(())
    }

    pub(crate) fn into_planned(self, table: &ResourceTable) -> Result<PlannedSystems> {
        let mut planned = Vec::with_capacity(self.0.len());
        for (schedule, systems) in self.0 {
            let mut owned = Vec::with_capacity(systems.len());
            for system in systems {
                owned.push(table.get(&system)?.clone());
            }
            planned.push((schedule, owned));
        }

        Ok(PlannedSystems(planned))
    }

    fn add_system(
        schedule: impl ScheduleLabel,
        system: &WasmSystem,
        world: &mut World,
        mod_id: Entity,
        mod_name: &str,
        asset_id: &AssetId<ModAsset>,
        asset_version: &Tick,
        access: &ModAccess,
    ) -> Result<()> {
        let schedule_config = Self::schedule(
            system,
            world,
            mod_id,
            mod_name,
            asset_id,
            asset_version,
            access,
        )?
        .in_set(ModSystemSet::All)
        .in_set(ModSystemSet::Mod(mod_id))
        .in_set(ModSystemSet::Access(*access));

        world
            .get_resource_mut::<Schedules>()
            .expect("running in an App")
            .add_systems(schedule, schedule_config);

        Ok(())
    }

    pub(crate) fn schedule(
        sys: &WasmSystem,
        world: &mut World,
        mod_id: Entity,
        mod_name: &str,
        asset_id: &AssetId<ModAsset>,
        _asset_version: &Tick,
        access: &ModAccess,
    ) -> Result<ScheduleConfigs<BoxedSystem>> {
        // The input struct contains various data used at runtime
        let built_params = BuiltParam::new_vec(&sys.params);
        let query_resolver = QueryResolver::new(&sys.params, world)?;
        let resource_resolver = ResourceResolver::new(&sys.params, world)?;
        let insert_despawn_component = InsertDespawnComponent::new(mod_id, world);
        let instance_pre = world
            .resource::<Assets<ModAsset>>()
            .get(*asset_id)
            .expect("mod asset exists while registering systems")
            .instance_pre();
        let input = Input {
            mod_name: mod_name.to_string(),
            system_name: sys.name.clone(),
            asset_id: *asset_id,
            instance_pre,
            built_params,
            query_resolver,
            resource_resolver,
            access: *access,
            insert_despawn_component,
        };

        // Generate the queries necessary to run this system
        let filtered_access = access.filtered_access(world);
        let mut queries = Vec::with_capacity(sys.params.len());
        for items in sys.params.iter().filter_map(Param::filter_query) {
            queries.push(create_query_builder(items, world, filtered_access.clone())?);
        }
        let resources = create_resource_builder(&sys.params, world)?;

        let system = (
            LocalBuilder(input),
            LocalBuilder(Vec::with_capacity(queries.len())),
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            resources,
            ParamSetBuilder(queries),
        )
            .build_state(world)
            .build_system(dynamic_system)
            .with_name(format!("wasvy[{mod_name}]::{}", sys.name));

        let boxed_system = Box::new(IntoSystem::into_system(system));

        let mut schedule_config = boxed_system
            // See docs for [SystemIdentifier]
            .in_set(sys.id);

        // Implement system ordering
        for after in sys.after.iter() {
            schedule_config = schedule_config.after(*after);
        }

        Ok(schedule_config)
    }
}

struct Input {
    mod_name: String,
    system_name: String,
    asset_id: AssetId<ModAsset>,
    instance_pre: InstancePre<crate::host::WasmHost>,
    built_params: Vec<BuiltParam>,
    query_resolver: QueryResolver,
    resource_resolver: ResourceResolver,
    access: ModAccess,
    insert_despawn_component: InsertDespawnComponent,
}

impl FromWorld for Input {
    fn from_world(_: &mut World) -> Self {
        unreachable!("Input is created with LocalBuilder")
    }
}

/// Since mod systems are by their very nature dynamic, they require a
/// flexible dynamic equivalent at runtime that can adjust to access
/// just what that mod system needs.
fn dynamic_system(
    input: Local<Input>,
    mut params: Local<Vec<Val>>,
    assets: Res<Assets<ModAsset>>,
    engine: Res<Engine>,
    type_registry: Res<AppTypeRegistry>,
    codec: Res<CodecResource>,
    wasm_registry: Res<WasmComponentRegistry>,
    function_index: Res<FunctionIndex>,
    mut commands: Commands,
    mut resources: FilteredResourcesMut,
    mut queries: ParamSet<Vec<Query<FilteredEntityMut>>>,
) -> BevyResult {
    // Skip no longer loaded mods
    let Some(asset) = assets.get(input.asset_id) else {
        return Ok(());
    };

    let _ = asset;

    let mut runner = Runner::new(&engine);
    initialize_params(&mut params, &input.built_params, &mut runner)?;

    trace!(
        "Running system \"{}\" from \"{}\"",
        input.system_name, input.mod_name
    );
    run_system_instance_pre(
        &input.instance_pre,
        &mut runner,
        &input.system_name,
        ConfigRunSystem {
            commands: SendSyncPtr::new(NonNull::from_mut(&mut commands).cast()),
            type_registry: &type_registry,
            codec: &codec,
            wasm_registry: &wasm_registry,
            function_index: &function_index,
            queries: SendSyncPtr::new(NonNull::from_mut(&mut queries).cast()),
            query_resolver: &input.query_resolver,
            resources: SendSyncPtr::new(NonNull::from_mut(&mut resources).cast()),
            resource_resolver: &input.resource_resolver,
            access: input.access,
            insert_despawn_component: input.insert_despawn_component,
        },
        &params[..],
    )?;

    Ok(())
}

impl PlannedSystems {
    pub(crate) fn referenced_type_paths(&self) -> Vec<String> {
        let mut types = Vec::new();
        for (_, systems) in &self.0 {
            for system in systems {
                for param in &system.params {
                    types.extend(param.referenced_type_paths());
                }
            }
        }
        types.sort();
        types.dedup();
        types
    }

    pub(crate) fn add_module_systems(
        self,
        world: &mut World,
        accesses: &[ModAccess],
        module_id: &ModuleId,
        module_entity: Entity,
        generation: ModuleGeneration,
        asset_id: &AssetId<ModAsset>,
        asset_version: &Tick,
    ) -> Result<()> {
        for access in accesses {
            let mod_schedules = access.schedules(world);
            for (schedule, systems) in self.0.iter() {
                let Some(schedule) = mod_schedules
                    .evaluate(schedule)
                    .map(|schedule| schedule.schedule_label())
                else {
                    warn!(
                        "Module tried adding systems to schedule {schedule:?}, but that schedule is not enabled. See ModSchedules docs."
                    );
                    continue;
                };

                for system in systems {
                    let schedule_config = AddSystems::schedule(
                        system,
                        world,
                        module_entity,
                        module_id.as_str(),
                        asset_id,
                        asset_version,
                        access,
                    )?
                    .in_set(ModuleSystemSet::All)
                    .in_set(ModuleSystemSet::Module(module_id.clone()))
                    .in_set(ModuleSystemSet::Generation {
                        id: module_id.clone(),
                        generation,
                    })
                    .in_set(ModuleSystemSet::Access(*access));

                    world
                        .get_resource_mut::<Schedules>()
                        .expect("running in an App")
                        .add_systems(schedule, schedule_config);
                }
            }
        }

        Ok(())
    }
}

/// A system param (what a mod system requests as parameters)
#[derive(Clone)]
pub(crate) enum Param {
    Commands,
    Query(Vec<QueryFor>),
    Res(String),
    ResMut(String),
}

pub(crate) struct ResourceParam<'a> {
    type_path: &'a str,
    mutable: bool,
}

impl<'a> ResourceParam<'a> {
    pub(crate) fn type_path(&self) -> &'a str {
        self.type_path
    }

    pub(crate) fn mutable(&self) -> bool {
        self.mutable
    }
}

impl Param {
    pub(crate) fn filter_query(&self) -> Option<&Vec<QueryFor>> {
        match self {
            Param::Query(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn filter_resource(&self) -> Option<ResourceParam<'_>> {
        match self {
            Param::Res(type_path) => Some(ResourceParam {
                type_path,
                mutable: false,
            }),
            Param::ResMut(type_path) => Some(ResourceParam {
                type_path,
                mutable: true,
            }),
            _ => None,
        }
    }

    pub(crate) fn referenced_type_paths(&self) -> Vec<String> {
        match self {
            Param::Commands => Vec::new(),
            Param::Query(items) => items
                .iter()
                .filter_map(|item| match item {
                    QueryFor::Ref(type_path)
                    | QueryFor::Mut(type_path)
                    | QueryFor::With(type_path)
                    | QueryFor::Without(type_path) => Some(type_path.clone()),
                })
                .collect(),
            Param::Res(type_path) | Param::ResMut(type_path) => vec![type_path.clone()],
        }
    }
}

/// Each time a system runs, these are used to generate the wasi resources passed to the mod (system params)
enum BuiltParam {
    Commands,
    Query(QueryId),
    Resource(ResourceId),
}

impl BuiltParam {
    fn new_vec(params: &[Param]) -> Vec<Self> {
        let mut query_ids = QueryIdGenerator::default();
        let mut resource_ids = ResourceIdGenerator::default();
        params
            .iter()
            .map(|param| match param {
                Param::Commands => BuiltParam::Commands,
                Param::Query(_) => BuiltParam::Query(query_ids.generate()),
                Param::Res(_) | Param::ResMut(_) => BuiltParam::Resource(resource_ids.generate()),
            })
            .collect()
    }
}

fn initialize_params(
    params: &mut Vec<Val>,
    source: &[BuiltParam],
    runner: &mut Runner,
) -> Result<()> {
    params.clear();
    for param in source.iter() {
        let resource = match param {
            BuiltParam::Commands => runner.new_resource(WasmCommands),
            BuiltParam::Query(id) => runner.new_resource(WasmQuery::new(*id)),
            BuiltParam::Resource(id) => runner.new_resource(crate::host::WasmResource::new(*id)),
        }?;
        params.push(Val::Resource(resource));
    }
    Ok(())
}

/// Bevy doesn't return an identifier for systems added directly to the scheduler. There is
/// [NodeId](bevy_ecs::schedule::NodeId) but that has no clear way of being used for system ordering.
///
/// So instead we take inspiration from bevy's [AnonymousSet](bevy_ecs::schedule::AnonymousSet)
/// and we identify each system with an extra [SystemSet] all to itself.
// Note: Using an AnonymousSet could work but unfortunately the method used to create one is private.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct DynamicSystemId(usize);

impl DynamicSystemId {
    /// Initialize a unique identifier in the world
    pub(crate) fn new(world: &mut World) -> Self {
        world.init_resource::<DynamicSystemSetCount>();
        let mut count = world
            .get_resource_mut::<DynamicSystemSetCount>()
            .expect("SystemIdentifierCount to be initialized");
        let identifier = DynamicSystemId(count.0);
        count.0 += 1;
        identifier
    }
}

impl SystemSet for DynamicSystemId {
    // As of bevy 0.18 this function's only purpose is for debugging
    fn is_anonymous(&self) -> bool {
        // This is technically incorrect, but it makes bevy use the system name as node name instead of DynamicSystemId(usize)
        true
    }

    fn dyn_clone(&self) -> Box<dyn SystemSet> {
        Box::new(*self)
    }
}

/// An tracker to ensure unique [DynamicSystemId]s in the world
#[derive(Default, BevyResource)]
struct DynamicSystemSetCount(usize);
