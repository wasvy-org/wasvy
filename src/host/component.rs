use anyhow::{Result, bail};
use bevy::ecs::{entity::Entity, reflect::AppTypeRegistry, world::FilteredEntityRef};
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
    value: String,
    component_ref: ComponentRef,
    mutable: bool,
    changed: bool,
}

impl Component {
    pub(crate) fn new(
        query_index: usize,
        entity: &FilteredEntityRef,
        component: &QueryForComponent,
        type_registry: &AppTypeRegistry,
    ) -> Result<Self> {
        let (component_ref, mutable) = match component {
            QueryForComponent::Ref(component_ref) => (component_ref, false),
            QueryForComponent::Mut(component_ref) => (component_ref, true),
        };

        let value = get_component(entity, component_ref.clone(), type_registry)?;

        Ok(Self {
            query_index,
            entity: entity.id(),
            value,
            component_ref: component_ref.clone(),
            mutable,
            changed: false,
        })
    }
}

impl HostComponent for WasmHost {
    fn get(&mut self, component: Resource<Component>) -> Result<SerializedComponent> {
        let component = self.table().get(&component)?;
        Ok(component.value.clone())
    }

    fn set(&mut self, component: Resource<Component>, value: SerializedComponent) -> Result<()> {
        let component = self.table().get_mut(&component)?;

        if !component.mutable {
            bail!("Component is not mutable!")
        }

        component.changed = true;
        component.value = value;

        Ok(())
    }

    fn drop(&mut self, component: Resource<Component>) -> Result<()> {
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
            value,
            component_ref,
            changed,
            ..
        } = table.delete(component)?;

        if !changed {
            return Ok(());
        }

        let mut query = queries.get_mut(query_index);
        let mut entity = query.get_mut(entity).expect("Component entity to be valid");

        set_component(&mut entity, component_ref, value, type_registry)?;

        Ok(())
    }
}
