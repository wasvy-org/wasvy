use std::collections::{BTreeMap, HashSet};

use error_collection::Errors;

use anyhow::Result;
use bevy_ecs::{
    error::Result as BevyResult,
    prelude::*,
    system::{BoxedSystem, LocalBuilder, ParamBuilder, ParamSetBuilder, SystemParam},
};
use bevy_log::prelude::*;
use wasmtime::component::{InstancePre, Resource, Val};
use wasmtime_wasi::ResourceTable;
use wasvy_runtime::{
    asset::ModDiscovery,
    commands::RuntimeCommands,
    component::WasmComponentRegistry,
    prelude::FunctionIndex,
    schedule::ModScheduleName,
    serialize::CodecResource,
    system::{ModSystemBuild, ModSystemContext, ModSystemDefinition, ModSystemName},
};

use crate::{
    bindings::wasvy::ecs::app::QueryFor,
    engine::Engine,
    host::{WasmCommands, WasmHost, WasmQuery, WasmSystem},
    query::{Queries, QueryId, QueryIdGenerator, QueryResolver, create_query_builder},
    runner::{Config, ConfigRunSystem, Runner},
    wasm_asset::call,
};

/// A helper struct that stores dynamic systems that a mod would like to register.
///
/// Wasvy only registers systems after mod's setup method has successfully run.
#[derive(Default)]
pub(crate) struct AddSystems(Vec<(ModScheduleName, Vec<Resource<WasmSystem>>)>);

impl AddSystems {
    pub(crate) fn push(&mut self, schedule: ModScheduleName, systems: Vec<Resource<WasmSystem>>) {
        self.0.push((schedule, systems));
    }

    pub(crate) fn definitions(
        self,
        table: &ResourceTable,
        instance_pre: &InstancePre<WasmHost>,
    ) -> ModDiscovery {
        let requests = self.group_schedules();
        let scheduled = requests.keys().copied().collect();
        let mut discovery = ModDiscovery::default();

        for (resource, schedules) in requests {
            let system = table.get(&Resource::<WasmSystem>::new_borrow(resource));
            let Some(system) = discovery.errors.collect(system) else {
                continue;
            };
            discovery.systems.push(system.definition(
                schedules,
                table,
                &scheduled,
                instance_pre,
                &mut discovery.errors,
            ));
        }
        discovery
    }

    fn group_schedules(self) -> BTreeMap<u32, HashSet<ModScheduleName>> {
        let mut requests: BTreeMap<_, HashSet<_>> = BTreeMap::new();
        for (schedule, systems) in self.0 {
            for system in systems {
                requests
                    .entry(system.rep())
                    .or_default()
                    .insert(schedule.clone());
            }
        }
        requests
    }
}

impl WasmSystem {
    fn definition(
        &self,
        schedules: HashSet<ModScheduleName>,
        table: &ResourceTable,
        scheduled: &HashSet<u32>,
        instance_pre: &InstancePre<WasmHost>,
        errors: &mut Errors,
    ) -> ModSystemDefinition {
        let mut definition = ModSystemDefinition::new(
            self.name.clone(),
            WasmSystemFactory {
                export_name: self.name.clone(),
                params: self.params.clone(),
                instance_pre: instance_pre.clone(),
            },
        )
        .in_schedules(schedules);
        definition.before = ordering_names(&self.before, table, scheduled, errors);
        definition.after = ordering_names(&self.after, table, scheduled, errors);
        definition
    }
}

fn ordering_names(
    references: &[u32],
    table: &ResourceTable,
    scheduled: &HashSet<u32>,
    errors: &mut Errors,
) -> HashSet<ModSystemName> {
    references
        .iter()
        .filter(|reference| scheduled.contains(reference))
        .filter_map(|reference| {
            errors.collect(table.get(&Resource::<WasmSystem>::new_borrow(*reference)))
        })
        .map(|system| system.name.clone().into())
        .collect()
}

struct WasmSystemFactory {
    export_name: String,
    params: Vec<Param>,
    instance_pre: InstancePre<WasmHost>,
}

impl ModSystemBuild for WasmSystemFactory {
    fn build(&self, context: ModSystemContext<'_>) -> Result<BoxedSystem> {
        let ModSystemContext {
            world,
            mod_name,
            access,
        } = context;
        // The input struct contains various data used at runtime
        let built_params = BuiltParam::new_vec(&self.params);
        let query_resolver = QueryResolver::new(&self.params, world)?;
        let input = Input {
            mod_name: mod_name.to_string(),
            system_name: self.export_name.clone(),
            built_params,
            query_resolver,
            instance_pre: self.instance_pre.clone(),
        };

        // Generate the queries necessary to run this system
        let mut queries = Vec::with_capacity(self.params.len());
        for items in self.params.iter().filter_map(Param::filter_query) {
            queries.push(create_query_builder(items, world, access.clone())?);
        }

        let system = (
            LocalBuilder(input),
            LocalBuilder(Vec::with_capacity(queries.len())),
            ParamBuilder,
            RunSystemParamsBuilder {
                commands: ParamBuilder,
                type_registry: ParamBuilder,
                codec: ParamBuilder,
                wasm_registry: ParamBuilder,
                function_index: ParamBuilder,
                // TODO: QueryParamBuilder for filtered resource access.
                queries: ParamSetBuilder(queries),
            },
        )
            .build_state(world)
            .build_system(dynamic_system)
            .with_name(format!("wasvy[{mod_name}]::{}", self.export_name));

        let boxed_system: BoxedSystem = Box::new(IntoSystem::into_system(system));
        Ok(boxed_system)
    }
}

struct Input {
    mod_name: String,
    system_name: String,
    built_params: Vec<BuiltParam>,
    query_resolver: QueryResolver,
    instance_pre: InstancePre<WasmHost>,
}

impl FromWorld for Input {
    fn from_world(_: &mut World) -> Self {
        unreachable!("Input is created with LocalBuilder")
    }
}

/// The system params a mod system can access while it runs
#[derive(SystemParam)]
#[system_param(builder)]
pub(crate) struct RunSystemParams<'w, 's> {
    pub(crate) commands: RuntimeCommands<'w, 's>,
    pub(crate) type_registry: Res<'w, AppTypeRegistry>,
    pub(crate) codec: Res<'w, CodecResource>,
    pub(crate) wasm_registry: Res<'w, WasmComponentRegistry>,
    pub(crate) function_index: Res<'w, FunctionIndex>,
    pub(crate) queries: Queries<'w, 's>,
}

/// Since mod systems are by their very nature dynamic, they require a
/// flexible dynamic equivalent at runtime that can adjust to access
/// just what that mod system needs.
fn dynamic_system(
    input: Local<Input>,
    mut params: Local<Vec<Val>>,
    engine: Res<Engine>,
    mut system_params: RunSystemParams,
) -> BevyResult {
    let mut runner = Runner::new(&engine);
    initialize_params(&mut params, &input.built_params, &mut runner)?;

    trace!(
        "Running system \"{}\" from \"{}\"",
        input.system_name, input.mod_name
    );

    let config = ConfigRunSystem {
        params: &mut system_params,
        query_resolver: &input.query_resolver,
    };
    call(
        &mut runner,
        &input.instance_pre,
        Config::RunSystem(config),
        &input.system_name,
        &params[..],
        &mut [],
    )?;

    Ok(())
}

/// A system param (what a mod system requests as parameters)
#[derive(Clone)]
pub(crate) enum Param {
    Commands,
    Query(Vec<QueryFor>),
}

impl Param {
    pub(crate) fn filter_query(&self) -> Option<&Vec<QueryFor>> {
        match self {
            Param::Query(items) => Some(items),
            _ => None,
        }
    }
}

/// Each time a system runs, these are used to generate the wasi resources passed to the mod (system params)
enum BuiltParam {
    Commands,
    Query(QueryId),
}

impl BuiltParam {
    fn new_vec(params: &[Param]) -> Vec<Self> {
        let mut ids = QueryIdGenerator::default();
        params
            .iter()
            .map(|param| match param {
                Param::Commands => BuiltParam::Commands,
                Param::Query(_) => BuiltParam::Query(ids.generate()),
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
        }?;
        params.push(Val::Resource(resource));
    }
    Ok(())
}
