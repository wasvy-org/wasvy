use anyhow::{Result, bail};
use bevy::ecs::{
    component::ComponentId,
    query::{FilteredAccess, QueryBuilder},
    system::QueryParamBuilder,
    world::{FilteredEntityMut, World},
};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostQuery, QueryFor},
    component::ComponentRef,
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
    fn iter(&mut self, query: Resource<Query>) -> Result<Option<Vec<Resource<Component>>>> {
        let State::RunSystem { table, queries, .. } = self.access() else {
            bail!("Query can only be accessed in systems")
        };

        let query = table.get_mut(&query)?;

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
            let resource = Component::new(query_index, &entity, component)?;
            let resource = table.push(resource)?;
            resources.push(resource);
        }

        Ok(Some(resources))
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, query: Resource<Query>) -> Result<()> {
        let _ = self.table().delete(query)?;

        Ok(())
    }
}

/// Needed to at runtime to construct the components wit resources returned from iter() on a query resource
///
/// Note: Ignores query filters (with and without) since these are not relevant
#[derive(Clone)]
pub(crate) enum QueryForComponent {
    Ref(ComponentRef),
    Mut(ComponentRef),
}

impl QueryForComponent {
    pub(crate) fn new(original: &QueryFor, world: &mut World) -> Result<Option<Self>> {
        Ok(match original {
            QueryFor::Ref(type_path) => Some(Self::Ref(ComponentRef::new(type_path, world)?)),
            QueryFor::Mut(type_path) => Some(Self::Mut(ComponentRef::new(type_path, world)?)),
            QueryFor::With(_) => None,
            QueryFor::Without(_) => None,
        })
    }
}

pub(crate) fn create_query_builder(
    original_items: &[QueryFor],
    world: &mut World,
    access: FilteredAccess,
) -> Result<
    QueryParamBuilder<Box<dyn FnOnce(&mut QueryBuilder<FilteredEntityMut<'static, 'static>>)>>,
> {
    let mut items = Vec::with_capacity(original_items.len());
    for original in original_items {
        items.push(QueryForId::new(original, world)?);
    }

    Ok(QueryParamBuilder::new_box(move |builder| {
        builder.extend_access(access);
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
            QueryFor::Ref(type_path) => {
                Self::Ref(ComponentRef::new(type_path, world)?.component_id())
            }
            QueryFor::Mut(type_path) => {
                Self::Mut(ComponentRef::new(type_path, world)?.component_id())
            }
            QueryFor::With(type_path) => {
                Self::With(ComponentRef::new(type_path, world)?.component_id())
            }
            QueryFor::Without(type_path) => {
                Self::Without(ComponentRef::new(type_path, world)?.component_id())
            }
        })
    }
}
