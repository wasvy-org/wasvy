use anyhow::{Result, anyhow, bail};
use bevy_ecs::{
    component::ComponentId, prelude::*, query::FilteredAccess, system::QueryParamBuilder,
    world::FilteredEntityMut,
};

use crate::{
    bindings::wasvy::ecs::app::{ComponentIndex, QueryFor},
    component::{ComponentRef, get_component, set_component},
    system::Param,
};

pub(crate) type Queries<'w, 's> =
    ParamSet<'w, 's, Vec<Query<'static, 'static, FilteredEntityMut<'static, 'static>>>>;

/// A helper struct that stores a static lookup for queries.
///
/// - The first dimension is the ParamSet index (QueryId).
/// - The second is the component index
pub(crate) struct QueryResolver(Vec<Vec<QueryForComponent>>);

impl QueryResolver {
    pub(crate) fn new(params: &[Param], world: &mut World) -> Result<Self> {
        let mut result = Vec::new();
        for component in params.iter().filter_map(|param| Param::filter_query(param)) {
            let mut components = Vec::new();
            for original in component {
                if let Some(component) = QueryForComponent::new(original, world)? {
                    components.push(component);
                }
            }
            result.push(components);
        }

        Ok(Self(result))
    }

    pub(crate) fn get(
        &self,
        id: QueryId,
        entity: Entity,
        index: ComponentIndex,
        queries: &mut Queries<'_, '_>,
        type_registry: &AppTypeRegistry,
    ) -> Result<String> {
        let query_for = self.query_for(id, index)?;

        let query = queries.get_mut(id.0);
        let entity = query.get(entity)?;

        get_component(&entity, &query_for.component, type_registry)
    }

    pub(crate) fn set(
        &self,
        id: QueryId,
        entity: Entity,
        index: ComponentIndex,
        serialized_value: String,
        queries: &mut Queries<'_, '_>,
        type_registry: &AppTypeRegistry,
    ) -> Result<()> {
        let query_for = self.query_for(id, index)?;
        if !query_for.mutable {
            bail!("Component is not mutable!")
        }

        let mut query = queries.get_mut(id.0);
        let mut entity = query.get_mut(entity)?;

        set_component(
            &mut entity,
            &query_for.component,
            serialized_value,
            type_registry,
        )
    }

    fn query_for(&self, id: QueryId, index: ComponentIndex) -> Result<&QueryForComponent> {
        let id = id.0;
        self.0
            .get(id)
            .expect("Valid query index")
            .get(index as usize)
            .ok_or_else(|| anyhow!("Query index {id} does not have component index {index}"))
    }
}

#[derive(Default)]
pub(crate) struct QueryIdGenerator(usize);

impl QueryIdGenerator {
    pub(crate) fn generate(&mut self) -> QueryId {
        let index = self.0;
        self.0 += 1;
        QueryId(index)
    }
}

/// An index in the ParamSet of all queries for the system
#[derive(Clone, Copy)]
pub(crate) struct QueryId(usize);

/// A cursor so we can resume iterating the query from the last position.
#[derive(Default, Clone, Copy)]
pub(crate) struct QueryCursor(usize);

impl QueryCursor {
    /// Increments the cursor, returning the old one
    pub(crate) fn increment(&mut self) -> Self {
        let original = *self;
        self.0 += 1;
        original
    }

    /// Retrieves the entity at the cursor
    pub(crate) fn entity(&self, queries: &mut Queries<'_, '_>, id: QueryId) -> Option<Entity> {
        let query = queries.get_mut(id.0);

        // This is not the most efficient. Ideally we wouldn't need to walk
        // to the nth iter each time, but this allows to avoid unsafe.
        // TODO: Store an actual proper cursor.
        query.iter().nth(self.0).map(|a| a.id())
    }
}

/// Needed at runtime to construct the components wit resources returned from iter() on a query resource
///
/// Note: Ignores query filters (with and without) since these are not relevant
#[derive(Clone)]
struct QueryForComponent {
    component: ComponentRef,
    mutable: bool,
}

impl QueryForComponent {
    fn new(original: &QueryFor, world: &mut World) -> Result<Option<Self>> {
        Ok(match original {
            QueryFor::Ref(type_path) => Some(Self {
                component: ComponentRef::new(type_path, world)?,
                mutable: false,
            }),
            QueryFor::Mut(type_path) => Some(Self {
                component: ComponentRef::new(type_path, world)?,
                mutable: true,
            }),
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
