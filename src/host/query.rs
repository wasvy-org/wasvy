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
}

impl Query {
    pub(crate) fn new(index: usize) -> Self {
        Self { index, position: 0 }
    }
}

impl HostQuery for WasmHost {
    fn iter(&mut self, query: Resource<Query>) -> Result<Option<Vec<Resource<Component>>>> {
        let State::RunSystem { table, queries, .. } = self.access() else {
            bail!("Systems can only be instantiated in a setup function")
        };

        let query = table.get_mut(&query)?;

        let next = queries
            .get_mut(query.index)
            .iter()
            .nth(query.position)
            .map(|_item| Vec::new());

        query.position += 1;

        Ok(next)
    }

    fn drop(&mut self, _rep: Resource<Query>) -> Result<()> {
        Ok(())
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
