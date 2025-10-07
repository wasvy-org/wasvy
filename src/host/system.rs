use anyhow::{Result, bail};
use bevy::{
    asset::AssetId,
    ecs::{
        component::Tick,
        error::Result as BevyResult,
        reflect::AppTypeRegistry,
        system::{
            BoxedSystem, Commands as BevyCommands, IntoSystem, Local, LocalBuilder, ParamBuilder,
            ParamSet, ParamSetBuilder, Query as BevyQuery, SystemParamBuilder,
        },
        world::{FilteredEntityMut, FromWorld, World},
    },
    log::trace,
    prelude::{Assets, Res},
};
use wasmtime::component::{Resource, Val};

use crate::{
    asset::ModAsset,
    bindings::wasvy::ecs::app::{HostSystem, QueryFor},
    engine::Engine,
    host::{Commands, Query, QueryForComponent, WasmHost, create_query_builder},
    runner::{ConfigRunSystem, Runner, State},
};

pub struct System {
    name: String,
    params: Vec<Param>,
    built: bool,
}

impl System {
    pub(crate) fn build(
        &mut self,
        mut world: &mut World,
        mod_name: &str,
        asset_id: &AssetId<ModAsset>,
        asset_version: &Tick,
    ) -> Result<BoxedSystem> {
        if self.built {
            bail!("System was already added to the app");
        }
        self.built = true;

        let mut built_params = Vec::new();
        for param in self.params.iter() {
            built_params.push(param.build(world)?);
        }

        // Used internally by the system
        let input = Input {
            mod_name: mod_name.to_string(),
            system_name: self.name.clone(),
            asset_id: asset_id.clone(),
            asset_version: asset_version.clone(),
            built_params,
        };

        // Generate the queries necessary to run this system
        let mut queries = Vec::with_capacity(self.params.len());
        for items in self.params.iter().filter_map(Param::filter_query) {
            queries.push(create_query_builder(items, world)?);
        }

        let system = (
            LocalBuilder(input),
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            ParamBuilder,
            // TODO: FilteredResourcesMutParamBuilder::new(|builder| {}),
            ParamSetBuilder(queries),
        )
            .build_state(&mut world)
            .build_system(system_runner)
            .with_name(format!("wasvy[{mod_name}]::{}", self.name));

        let boxed_system = Box::new(IntoSystem::into_system(system));

        Ok(boxed_system)
    }

    fn add_param(host: &mut WasmHost, system: Resource<System>, param: Param) -> Result<()> {
        let State::Setup { table, .. } = host.access() else {
            bail!("Systems can only be modified in a setup function")
        };

        let system = table.get_mut(&system)?;
        system.params.push(param);

        Ok(())
    }
}

#[derive(FromWorld)]
struct Input {
    mod_name: String,
    system_name: String,
    asset_id: AssetId<ModAsset>,
    asset_version: Tick,
    built_params: Vec<BuiltParam>,
}

fn system_runner(
    input: Local<Input>,
    assets: Res<Assets<ModAsset>>,
    engine: Res<Engine>,
    type_registry: Res<AppTypeRegistry>,
    mut commands: BevyCommands,
    // TODO: mut resources: FilteredResourcesMut,
    mut queries: ParamSet<Vec<BevyQuery<FilteredEntityMut>>>,
) -> BevyResult {
    // Skip no longer loaded mods
    let Some(asset) = assets.get(input.asset_id) else {
        return Ok(());
    };

    // Skip mismatching system versions
    if asset.version() != input.asset_version {
        return Ok(());
    }

    let mut runner = Runner::new(&engine);

    let params = initialize_params(&input.built_params, &mut runner)?;

    trace!(
        "Running system \"{}\" from \"{}\"",
        input.system_name, input.mod_name
    );
    asset.run_system(
        &mut runner,
        &input.system_name,
        ConfigRunSystem {
            commands: &mut commands,
            type_registry: &type_registry,
            queries: &mut queries,
        },
        &params,
    )?;

    Ok(())
}

/// A system param (what a mod system requests as parameters)
enum Param {
    Commands,
    Query(Vec<QueryFor>),
}

impl Param {
    fn build(&self, world: &mut World) -> Result<BuiltParam> {
        Ok(match self {
            Param::Commands => BuiltParam::Commands,
            Param::Query(original_items) => {
                let mut items = Vec::new();
                for original in original_items {
                    if let Some(item) = QueryForComponent::new(original, world)? {
                        items.push(item);
                    }
                }
                BuiltParam::Query(items)
            }
        })
    }

    fn filter_query(&self) -> Option<&Vec<QueryFor>> {
        match self {
            Param::Query(items) => Some(items),
            _ => None,
        }
    }
}

/// A system param containing all the info needed by the system at runtime
enum BuiltParam {
    Commands,
    Query(Vec<QueryForComponent>),
}

fn initialize_params(source: &[BuiltParam], runner: &mut Runner) -> Result<Vec<Val>> {
    let mut params = Vec::with_capacity(source.len());
    let mut query_index = 0;
    for param in source.iter() {
        let resource = match param {
            BuiltParam::Commands => runner.new_resource(Commands),
            BuiltParam::Query(components) => {
                let index = query_index;
                query_index += 1;
                runner.new_resource(Query::new(index, components.clone()))
            }
        }?;
        params.push(Val::Resource(resource));
    }
    Ok(params)
}

impl HostSystem for WasmHost {
    fn new(&mut self, name: String) -> Result<Resource<System>> {
        let State::Setup { table, .. } = self.access() else {
            bail!("Systems can only be instantiated in a setup function")
        };

        Ok(table.push(System {
            built: false,
            name,
            params: Vec::new(),
        })?)
    }

    fn add_commands(&mut self, system: Resource<System>) -> Result<()> {
        System::add_param(self, system, Param::Commands)
    }

    fn add_query(&mut self, system: Resource<System>, query: Vec<QueryFor>) -> Result<()> {
        System::add_param(self, system, Param::Query(query))
    }

    fn drop(&mut self, system: Resource<System>) -> Result<()> {
        let _ = self.table().delete(system)?;

        Ok(())
    }
}
