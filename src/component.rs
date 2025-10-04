use std::{alloc::Layout, any::TypeId};

use anyhow::{Result, anyhow};
use bevy::{
    ecs::{
        component::{ComponentDescriptor, ComponentId},
        reflect::ReflectCommandExt,
        world::{FilteredEntityMut, FilteredEntityRef},
    },
    platform::collections::HashMap,
    prelude::*,
    reflect::{
        ReflectFromPtr,
        serde::{TypedReflectDeserializer, TypedReflectSerializer},
    },
};
use serde::de::DeserializeSeed;

pub type TypePath = String;

/// Registry for storing the components that are registered from WASM assets.
///
/// Note that this is unique per world, not per app like the [AppTypeRegistry](bevy::ecs::reflect::AppTypeRegistry)
#[derive(Default, Clone, Debug, Resource, Deref, DerefMut)]
pub struct WasmComponentRegistry(HashMap<TypePath, ComponentId>);

/// This component is the wrapper component for all the Bevy components that are registered in a
/// WASM.
///
/// # Description
///
/// When you call the spawn method in WASM you need to provide a component id, that id is used to
/// add a new [`WasmComponent`] under that id with the `serialized_value` that is given.
///
/// This approach makes it possible to register components that don't exist in Rust.
#[derive(Component, Reflect)]
pub struct WasmComponent {
    pub serialized_value: String,
}

/// A command that inserts a guest defined component into an entity
///
/// It also registers the component if it hasn't been yet
struct InsertWasmComponent {
    component: WasmComponent,
    entity: Entity,
    type_path: String,
}

impl Command for InsertWasmComponent {
    fn apply(self, world: &mut World) {
        let component_id = get_wasm_component_id(&self.type_path, world);

        let mut commands = world.commands();
        let mut entity_commands = commands.entity(self.entity);

        // Safety:
        // - ComponentId is from the same world as self.
        // - T has the same layout as the one passed during component_id creation.
        unsafe { entity_commands.insert_by_id(component_id, self.component) };
    }
}

pub(crate) fn insert_component(
    commands: &mut Commands,
    type_registry: &AppTypeRegistry,
    entity: Entity,
    type_path: String,
    serialized_value: String,
) -> Result<()> {
    let type_registry = type_registry.read();

    // Insert types that are known by bevy (inserted as concrete types)
    if let Some(type_registration) = type_registry.get_with_type_path(&type_path) {
        let mut de = serde_json::Deserializer::from_str(&serialized_value);
        let reflect_deserializer = TypedReflectDeserializer::new(type_registration, &type_registry);
        let output: Box<dyn PartialReflect> = reflect_deserializer.deserialize(&mut de)?;

        commands.entity(entity).insert_reflect(output);
    }
    // Handle guest types (inserted as json strings)
    else {
        commands.queue(InsertWasmComponent {
            component: WasmComponent { serialized_value },
            entity,
            type_path,
        });
    }

    Ok(())
}

/// A collection containing a [ComponentId], and a [TypeId]
///
/// The type id is [None] for guest components, and [Some] for concrete host types
///
/// Guarantees:
/// - If type id is [None], then component_id must be of type [WasmComponent]
/// - If type id is [Some], then it is registered in the [AppTypeRegistry]
/// - If type id is [Some], then component_id must be of the same type
#[derive(Clone)]
pub(crate) struct ComponentRef {
    component_id: ComponentId,
    type_id: Option<TypeId>,
}

impl ComponentRef {
    /// See [ComponentRef]
    pub(crate) fn new(type_path: &str, mut world: &mut World) -> Result<Self> {
        let type_registry = world
            .get_resource::<AppTypeRegistry>()
            .expect("there to be an AppTypeRegistry")
            .read();

        // First try finding types known by bevy (inserted as concrete types)
        if let Some(type_registration) = type_registry.get_with_type_path(&type_path) {
            let type_id = type_registration.type_id();
            let component_id = world
                .components()
                .get_id(type_id)
                .ok_or(anyhow!("{type_path} is not a component"))?;

            Ok(Self {
                component_id,
                type_id: Some(type_id),
            })
        }
        // Otherwise handle guest types (inserted as json strings)
        else {
            drop(type_registry);

            let component_id = get_wasm_component_id(type_path, &mut world);

            Ok(Self {
                component_id,
                type_id: None,
            })
        }
    }

    pub(crate) fn component_id(&self) -> ComponentId {
        self.component_id
    }
}

/// Gets the component id given a type path, or registers a new component id for a [WasmComponent]
fn get_wasm_component_id(type_path: &str, world: &mut World) -> ComponentId {
    let component_registry = world.get_resource_or_init::<WasmComponentRegistry>();

    // Get an existing id if it exists
    if let Some(id) = component_registry.get(type_path) {
        *id
    }
    // Register it if necessary
    else {
        let type_path = type_path.to_string();

        // Safety:
        // - the drop fn is usable on this component type
        // - the component is safe to access from any thread
        let descriptor = unsafe {
            ComponentDescriptor::new_with_layout(
                type_path.clone(),
                WasmComponent::STORAGE_TYPE,
                Layout::new::<WasmComponent>(),
                Some(|ptr| {
                    ptr.drop_as::<WasmComponent>();
                }),
                true,
                WasmComponent::clone_behavior(),
            )
        };

        let id = world.register_component_with_descriptor(descriptor);

        let mut component_registry = world
            .get_resource_mut::<WasmComponentRegistry>()
            .expect("this method initialized it");
        component_registry.insert(type_path, id);

        id
    }
}

/// Retrieves the value of a component on an entity given a json string
pub(crate) fn get_component(
    entity: &FilteredEntityRef,
    component_ref: ComponentRef,
    type_registry: &AppTypeRegistry,
) -> Result<String> {
    let val = entity
        .get_by_id(component_ref.component_id)
        .expect("to be able to find this component id on the entity");

    // Types that are known by bevy (inserted as concrete types)
    if let Some(type_id) = component_ref.type_id {
        let type_registry = type_registry.read();
        let type_registration = type_registry
            .get(type_id)
            .expect("ComponentRef type_id be registered");
        let reflect_from_ptr = type_registration
            .data::<ReflectFromPtr>()
            .expect("ReflectFromPtr to be registered");

        // SAFETY: val is of the same type that reflect_from_ptr was constructed for
        let reflect = unsafe { reflect_from_ptr.as_reflect(val) };
        let serializer = TypedReflectSerializer::new(reflect, &type_registry);
        let value = serde_json::to_string(&serializer)?;

        Ok(value)
    }
    // Handle guest types (inserted as json strings)
    else {
        // SAFETY: val must be a WasmComponent (see [ComponentRef])
        let value = unsafe { val.deref::<WasmComponent>() };
        Ok(value.serialized_value.clone())
    }
}

/// Sets the value of a component on an entity given a json string
pub(crate) fn set_component(
    entity: &mut FilteredEntityMut,
    component_ref: ComponentRef,
    serialized_value: String,
    type_registry: &AppTypeRegistry,
) -> Result<()> {
    let mut val = entity
        .get_mut_by_id(component_ref.component_id)
        .expect("to be able to find this component id on the entity");

    // Types that are known by bevy (inserted as concrete types)
    if let Some(type_id) = component_ref.type_id {
        let type_registry = type_registry.read();
        let type_registration = type_registry
            .get(type_id)
            .expect("ComponentRef type_id be registered");

        let reflect_from_ptr = type_registration
            .data::<ReflectFromPtr>()
            .expect("ReflectFromPtr to be registered");

        let mut de = serde_json::Deserializer::from_str(&serialized_value);
        let reflect_deserializer = TypedReflectDeserializer::new(type_registration, &type_registry);
        let boxed_dyn_reflect = reflect_deserializer.deserialize(&mut de)?;

        // SAFETY: val is of the same type that ReflectFromPtr was constructed for
        let reflect = unsafe { reflect_from_ptr.as_reflect_mut(val.as_mut()) };
        reflect.apply(boxed_dyn_reflect.as_partial_reflect());

        Ok(())
    }
    // Handle guest types (inserted as json strings)
    else {
        // SAFETY: val must be a WasmComponent (see [ComponentRef])
        let component = unsafe { val.as_mut().deref_mut::<WasmComponent>() };
        component.serialized_value = serialized_value;

        Ok(())
    }
}
