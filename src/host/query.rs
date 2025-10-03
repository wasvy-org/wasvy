use anyhow::{Result, bail};
use bevy::ecs::{
    component::ComponentId,
    query::QueryBuilder,
    system::QueryParamBuilder,
    world::{FilteredEntityMut, World},
};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostQuery, QueryFor},
    component::get_component_id,
    host::{Component, WasmHost},
    runner::State,
};

pub struct Query {
    index: usize,
    position: usize,
    components: Vec<QueryForComponent>,
}

impl Query {
    /// Generate a new query.
    ///
    /// Pass the index of the query that should be used from the param set, and components
    pub(crate) fn new(index: usize, components: Vec<QueryForComponent>) -> Self {
        Self {
            index,
            position: 0,
            components,
        }
    }
}

impl HostQuery for WasmHost {
    fn iter(&mut self, query_res: Resource<Query>) -> Result<Option<Vec<Resource<Component>>>> {
        let State::RunSystem {
            table,
            queries,
            type_registry,
            component_registry,
            ..
        } = self.access()
        else {
            bail!("Query can only be accessed in systems")
        };

        let query = table.get_mut(&query_res)?;

        let position = query.position;
        query.position += 1;

        let bevy_query = queries.get_mut(query.index);
        let Some(entity) = bevy_query.iter().nth(position) else {
            return Ok(None);
        };

        // query must be dropped in order for us to be able to push new resources onto the table
        let query_index = query.index;
        let components = query.components.clone();

        let mut resources = Vec::with_capacity(components.len());
        for component in components.iter() {
            let resource = Component::new(
                query_index,
                &entity,
                component,
                type_registry,
                component_registry,
            )?;
            let resource = table.push_child(resource, &query_res)?;
            resources.push(resource);
        }

        Ok(Some(resources))
    }

    fn drop(&mut self, query: Resource<Query>) -> Result<()> {
        // Will produce an error if any Components returned from iter() are still in use
        self.table().delete(query)?;

        Ok(())
    }
}

/// Needed to at runtime to construct the Components returned from iter() on a query
///
/// Missing query filters (with and without) since these are not relevant
#[derive(Clone)]
pub(crate) enum QueryForComponent {
    Ref { id: ComponentId, type_path: String },
    Mut { id: ComponentId, type_path: String },
}

impl QueryForComponent {
    pub(crate) fn new(original: &QueryFor, world: &mut World) -> Result<Option<Self>> {
        Ok(match original {
            QueryFor::Ref(type_path) => Some(Self::Ref {
                id: get_component_id(type_path, world)?,
                type_path: type_path.clone(),
            }),
            QueryFor::Mut(type_path) => Some(Self::Mut {
                id: get_component_id(type_path, world)?,
                type_path: type_path.clone(),
            }),
            QueryFor::With(_) => None,
            QueryFor::Without(_) => None,
        })
    }
}

pub(crate) fn create_query_builder(
    original_items: &[QueryFor],
    world: &mut World,
) -> Result<
    QueryParamBuilder<Box<dyn FnOnce(&mut QueryBuilder<FilteredEntityMut<'static, 'static>>)>>,
> {
    let mut items = Vec::with_capacity(original_items.len());
    for original in original_items {
        items.push(QueryForId::new(original, world)?);
    }

    Ok(QueryParamBuilder::new_box(move |builder| {
        for item in items {
            match item {
                QueryForId::Ref(component_id) => {
                    builder.ref_id(component_id);
                }
                QueryForId::Mut(component_id) => {
                    builder.mut_id(component_id);
                }
                QueryForId::With(component_id) => {
                    builder.with_id(component_id);
                }
                QueryForId::Without(component_id) => {
                    builder.without_id(component_id);
                }
            }
        }
    }))
}

enum QueryForId {
    Ref(ComponentId),
    Mut(ComponentId),
    With(ComponentId),
    Without(ComponentId),
}

impl QueryForId {
    fn new(original: &QueryFor, world: &mut World) -> Result<Self> {
        Ok(match original {
            QueryFor::Ref(type_path) => Self::Ref(get_component_id(type_path, world)?),
            QueryFor::Mut(type_path) => Self::Mut(get_component_id(type_path, world)?),
            QueryFor::With(type_path) => Self::With(get_component_id(type_path, world)?),
            QueryFor::Without(type_path) => Self::Without(get_component_id(type_path, world)?),
        })
    }
}
