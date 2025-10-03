use anyhow::{Result, bail};
use bevy::ecs::{
    component::ComponentId, entity::Entity, reflect::AppTypeRegistry, world::FilteredEntityRef,
};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostComponent, SerializedComponent},
    component::{WasmComponentRegistry, get_component, set_component},
    host::{QueryForComponent, WasmHost},
    runner::State,
};

pub struct Component {
    query_index: usize,
    entity: Entity,
    value: String,
    id: ComponentId,
    type_path: String,
    mutable: bool,
    changed: bool,
}

impl Component {
    pub(crate) fn new(
        query_index: usize,
        entity: &FilteredEntityRef,
        component: &QueryForComponent,
        type_registry: &AppTypeRegistry,
        component_registry: &WasmComponentRegistry,
    ) -> Result<Self> {
        let (id, type_path, mutable) = match component {
            QueryForComponent::Ref { id, type_path } => (*id, type_path, false),
            QueryForComponent::Mut { id, type_path } => (*id, type_path, true),
        };

        let value =
            // SAFETY: the component id is registered with type_path
            unsafe { get_component(entity, id, type_path, type_registry, component_registry)? };

        Ok(Self {
            query_index,
            entity: entity.id(),
            value,
            id,
            type_path: type_path.clone(),
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
            component_registry,
            ..
        } = self.access()
        else {
            bail!("Component can only be accessed in systems")
        };

        let Component {
            query_index,
            entity,
            value,
            id,
            type_path,
            changed,
            ..
        } = table.delete(component)?;

        if !changed {
            return Ok(());
        }

        let mut query = queries.get_mut(query_index);
        let mut entity = query.get_mut(entity).expect("Component entity to be valid");

        // SAFETY: the component id is registered with type_path
        unsafe {
            set_component(
                &mut entity,
                id,
                &type_path,
                value,
                type_registry,
                component_registry,
            )?;
        }

        Ok(())
    }
}
