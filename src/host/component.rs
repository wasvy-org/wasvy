use anyhow::{Result, bail};
use bevy::ecs::{entity::Entity, world::FilteredEntityRef};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostComponent, SerializedComponent},
    component::{ComponentRef, get_component, set_component},
    host::{QueryForComponent, WasmHost},
    runner::State,
};

pub struct Component {
    query_index: usize,
    entity: Entity,
    component_ref: ComponentRef,
    mutable: bool,
}

impl Component {
    pub(crate) fn new(
        query_index: usize,
        entity: &FilteredEntityRef,
        component: &QueryForComponent,
    ) -> Result<Self> {
        let (component_ref, mutable) = match component {
            QueryForComponent::Ref(component_ref) => (component_ref, false),
            QueryForComponent::Mut(component_ref) => (component_ref, true),
        };

        Ok(Self {
            query_index,
            entity: entity.id(),
            component_ref: component_ref.clone(),
            mutable,
        })
    }
}

impl HostComponent for WasmHost {
    fn get(&mut self, component: Resource<Component>) -> Result<SerializedComponent> {
        let State::RunSystem {
            table,
            queries,
            type_registry,
            ..
        } = self.access()
        else {
            bail!("Component can only be accessed in systems")
        };

        let Component {
            query_index,
            entity,
            component_ref,
            ..
        } = table.get(&component)?;

        let query = queries.get_mut(*query_index);
        let entity = query.get(*entity).expect("Component entity to be valid");

        let value = get_component(&entity, component_ref.clone(), type_registry)?;

        Ok(value)
    }

    fn set(&mut self, component: Resource<Component>, value: SerializedComponent) -> Result<()> {
        let State::RunSystem {
            table,
            queries,
            type_registry,
            ..
        } = self.access()
        else {
            bail!("Component can only be accessed in systems")
        };

        let Component {
            query_index,
            entity,
            component_ref,
            mutable,
        } = table.get(&component)?;
        if !mutable {
            bail!("Component is not mutable!")
        }

        let mut query = queries.get_mut(*query_index);
        let mut entity = query
            .get_mut(*entity)
            .expect("Component entity to be valid");

        set_component(&mut entity, component_ref, value, type_registry)?;

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, component: Resource<Component>) -> Result<()> {
        let _ = self.table().delete(component)?;

        Ok(())
    }
}
