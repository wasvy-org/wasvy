use std::alloc::Layout;

use anyhow::Result;
use bevy::{
    ecs::{
        component::{ComponentDescriptor, ComponentId},
        reflect::ReflectCommandExt,
    },
    platform::collections::HashMap,
    prelude::*,
    reflect::serde::TypedReflectDeserializer,
};
use serde::de::DeserializeSeed;

pub type TypePath = String;

/// Registry for storing the components that are registered from WASM assets.
///
/// Note that this is unique per world, not per app like the [AppTypeRegistry](bevy::ecs::reflect::AppTypeRegistry)
#[derive(Default, Clone, Debug, Resource, Deref, DerefMut)]
pub struct WasmComponentRegistry(pub HashMap<TypePath, ComponentId>);

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
        // Get an existing id if it exists
        let component_registry = world.get_resource_or_init::<WasmComponentRegistry>();
        let component_id = if let Some(id) = component_registry.get(&self.type_path) {
            id.clone()
        }
        // Register it if necessary
        else {
            // Safety:
            // - the drop fn is usable on this component type
            // - the component is safe to access from any thread
            let descriptor = unsafe {
                ComponentDescriptor::new_with_layout(
                    self.type_path.clone(),
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
                .expect("this command initializes it");
            component_registry.insert(self.type_path, id);

            id
        };

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
