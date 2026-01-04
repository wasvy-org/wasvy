use anyhow::{Result, anyhow, bail};
use bevy_asset::{AssetId, Assets};
use bevy_ecs::{
    change_detection::Tick,
    error::Result as BevyResult,
    prelude::*,
    resource::Resource as BevyResource,
    schedule::ScheduleConfigs,
    system::{
        BoxedSystem, Commands as BevyCommands, LocalBuilder, ParamBuilder, ParamSetBuilder,
        Query as BevyQuery,
    },
    world::FilteredEntityMut,
};
use bevy_log::prelude::*;
use wasmtime::component::{Resource, Val};

use crate::{
    access::ModAccess,
    asset::ModAsset,
    bindings::wasvy::ecs::app::{HostSystem, QueryFor},
    engine::Engine,
    host::{Commands, Query, QueryForComponent, WasmHost, create_query_builder},
    runner::{ConfigRunSystem, Runner, State},
};

pub struct System {
    name: String,
    params: Vec<Param>,
    scheduled: bool,
    identifier: SystemIdentifier,
    after: Vec<SystemIdentifier>,
}

impl System {
    fn new(name: String, identifier: SystemIdentifier) -> Self {
        Self {
            name,
            params: Vec::new(),
            scheduled: false,
            identifier,
            after: Vec::new(),
        }
    }

    pub(crate) fn schedule(
        &mut self,
        mut world: &mut World,
        mod_name: &str,
        asset_id: &AssetId<ModAsset>,
        asset_version: &Tick,
        access: &ModAccess,
    ) -> Result<ScheduleConfigs<BoxedSystem>> {
        self.scheduled = true;

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
            access: *access,
        };

        // Generate the queries necessary to run this system
        let filtered_access = access.filtered_access(&mut world);
        let mut queries = Vec::with_capacity(self.params.len());
        for items in self.params.iter().filter_map(Param::filter_query) {
            queries.push(create_query_builder(items, world, filtered_access.clone())?);
        }

        // Dynamic
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

        let mut schedule_config = boxed_system
            // See docs for [SystemIdentifier]
            .in_set(self.identifier);

        // Implement system ordering
        for after in self.after.iter() {
            schedule_config = schedule_config.after(*after);
        }

        Ok(schedule_config)
    }

    fn editable(&self) -> Result<()> {
        if self.scheduled {
            Err(anyhow!(
                "System \"{}\" was already scheduled and thus can no longer be changed",
                self.name
            ))
        } else {
            Ok(())
        }
    }

    fn add_param(host: &mut WasmHost, system: Resource<System>, param: Param) -> Result<()> {
        let State::Setup { table, .. } = host.access() else {
            bail!("Systems can only be modified in a setup function")
        };

        let system = table.get_mut(&system)?;
        system.editable()?;

        system.params.push(param);

        Ok(())
    }
}

struct Input {
    mod_name: String,
    system_name: String,
    asset_id: AssetId<ModAsset>,
    asset_version: Tick,
    built_params: Vec<BuiltParam>,
    access: ModAccess,
}

impl FromWorld for Input {
    fn from_world(_world: &mut World) -> Self {
        unreachable!("Input is created with LocalBuilder")
    }
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
    if asset.version() != Some(input.asset_version) {
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
            access: input.access.clone(),
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

/// Bevy doesn't return an identifier for systems added directly to the scheduler. There is
/// [NodeId](bevy::ecs::schedule::NodeId) but that has no clear way of being used for system ordering.
///
/// So instead we take inspiration from bevy's [AnonymousSet](bevy::ecs::schedule::AnonymousSet)
/// and we identify each system with an extra [SystemSet] all to itself.
// Note: Using an AnonymousSet could work but unfortunately the method used to create one is private.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct SystemIdentifier(usize);

impl SystemIdentifier {
    /// Initialize a unique identifier in the world
    fn new(world: &mut World) -> Self {
        world.init_resource::<SystemIdentifierCount>();
        let mut count = world
            .get_resource_mut::<SystemIdentifierCount>()
            .expect("SystemIdentifierCount to be initialized");
        let identifier = SystemIdentifier(count.0);
        count.0 += 1;
        identifier
    }
}

impl SystemSet for SystemIdentifier {
    // As of bevy 0.17.2 this function's only purpose is for debugging
    fn is_anonymous(&self) -> bool {
        // This is technically incorrect, but it makes bevy use the system name as node name instead of SystemIdentifier(usize)
        true
    }

    fn dyn_clone(&self) -> Box<dyn SystemSet> {
        Box::new(*self)
    }
}

/// An tracker to ensure unique [SystemIdentifier]s in the world
#[derive(Default, BevyResource)]
struct SystemIdentifierCount(usize);

impl HostSystem for WasmHost {
    fn new(&mut self, name: String) -> Result<Resource<System>> {
        let State::Setup { table, world, .. } = self.access() else {
            bail!("Systems can only be instantiated in a setup function")
        };

        // A unique identifier for this system in the world
        let identifier = SystemIdentifier::new(world);

        Ok(table.push(System::new(name, identifier))?)
    }

    fn add_commands(&mut self, system: Resource<System>) -> Result<()> {
        System::add_param(self, system, Param::Commands)
    }

    fn add_query(&mut self, system: Resource<System>, query: Vec<QueryFor>) -> Result<()> {
        System::add_param(self, system, Param::Query(query))
    }

    fn after(&mut self, system: Resource<System>, other: Resource<System>) -> Result<()> {
        let State::Setup { table, .. } = self.access() else {
            bail!("Systems can only be modified in a setup function")
        };

        let other = table.get(&other)?.identifier;

        let system = table.get_mut(&system)?;
        system.editable()?;

        system.after.push(other);

        Ok(())
    }

    fn before(&mut self, system: Resource<System>, other: Resource<System>) -> Result<()> {
        // In bevy, `a.before(b)` is logically equivalent to `b.after(a)`
        HostSystem::after(self, other, system)
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, system: Resource<System>) -> Result<()> {
        let _ = self.table().delete(system)?;

        Ok(())
    }
}
