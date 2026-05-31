//! Dynamic resource serialization and storage for Wasvy guest systems.

use std::{alloc::Layout, any::TypeId};

use anyhow::{Result, anyhow};
use bevy_ecs::{
    change_detection::MaybeLocation,
    component::{ComponentCloneBehavior, ComponentDescriptor, ComponentId, StorageType},
    prelude::*,
    ptr::OwningPtr,
    reflect::ReflectResource,
    system::FilteredResourcesMutParamBuilder,
    world::{FilteredResourcesMut, FilteredResourcesMutBuilder},
};
use bevy_platform::collections::HashMap;
use bevy_reflect::{Reflect, ReflectFromPtr};

use crate::serialize::CodecResource;

pub type TypePath = String;

#[derive(Default)]
pub(crate) struct ResourceIdGenerator(usize);

impl ResourceIdGenerator {
    pub(crate) fn generate(&mut self) -> ResourceId {
        let index = self.0;
        self.0 += 1;
        ResourceId(index)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResourceId(usize);

#[derive(Clone)]
pub(crate) struct ResourceDescriptor {
    resource: ResourceRef,
    mutable: bool,
}

impl ResourceDescriptor {
    pub(crate) fn resource(&self) -> &ResourceRef {
        &self.resource
    }

    pub(crate) fn mutable(&self) -> bool {
        self.mutable
    }
}

pub(crate) struct ResourceResolver(Vec<ResourceDescriptor>);

impl ResourceResolver {
    pub(crate) fn new(params: &[crate::system::Param], world: &mut World) -> Result<Self> {
        let mut result = Vec::new();
        for param in params
            .iter()
            .filter_map(crate::system::Param::filter_resource)
        {
            result.push(ResourceDescriptor {
                resource: ResourceRef::new(param.type_path(), world)?,
                mutable: param.mutable(),
            });
        }
        Ok(Self(result))
    }

    pub(crate) fn get(&self, id: ResourceId) -> Result<&ResourceDescriptor> {
        self.0
            .get(id.0)
            .ok_or_else(|| anyhow!("Unknown resource id {}", id.0))
    }
}

pub(crate) fn create_resource_builder<'a>(
    params: &[crate::system::Param],
    world: &mut World,
) -> Result<FilteredResourcesMutParamBuilder<Box<dyn FnOnce(&mut FilteredResourcesMutBuilder) + 'a>>>
{
    let mut resources = Vec::new();
    for param in params
        .iter()
        .filter_map(crate::system::Param::filter_resource)
    {
        resources.push((
            ResourceRef::new(param.type_path(), world)?.component_id(),
            param.mutable(),
        ));
    }

    Ok(FilteredResourcesMutParamBuilder::new_box(move |builder| {
        for (component_id, mutable) in resources {
            if mutable {
                builder.add_write_by_id(component_id);
            } else {
                builder.add_read_by_id(component_id);
            }
        }
    }))
}

#[derive(Default, Clone, Debug, Resource)]
pub struct WasmResourceRegistry(HashMap<TypePath, ComponentId>);

#[derive(Resource, Reflect)]
pub struct WasmResourceValue {
    pub serialized_value: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct ResourceRef {
    component_id: ComponentId,
    type_id: Option<TypeId>,
}

impl ResourceRef {
    pub(crate) fn new(type_path: &str, world: &mut World) -> Result<Self> {
        let type_registry = world
            .get_resource::<AppTypeRegistry>()
            .expect("there to be an AppTypeRegistry")
            .read();

        if let Some(type_registration) = type_registry.get_with_type_path(type_path) {
            let type_id = type_registration.type_id();
            let component_id = world
                .components()
                .get_resource_id(type_id)
                .ok_or(anyhow!("{type_path} is not a resource"))?;

            Ok(Self {
                component_id,
                type_id: Some(type_id),
            })
        } else {
            drop(type_registry);

            let component_id = get_wasm_resource_id(type_path, world);
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

fn get_wasm_resource_id(type_path: &str, world: &mut World) -> ComponentId {
    let registry = world.get_resource_or_init::<WasmResourceRegistry>();
    if let Some(id) = registry.0.get(type_path) {
        *id
    } else {
        let type_path = type_path.to_string();
        let descriptor = unsafe {
            ComponentDescriptor::new_with_layout(
                type_path.clone(),
                StorageType::Table,
                Layout::new::<WasmResourceValue>(),
                Some(|ptr| {
                    ptr.drop_as::<WasmResourceValue>();
                }),
                true,
                ComponentCloneBehavior::Default,
                None,
            )
        };

        let id = world.register_resource_with_descriptor(descriptor);
        let mut registry = world
            .get_resource_mut::<WasmResourceRegistry>()
            .expect("registry initialized above");
        registry.0.insert(type_path, id);
        id
    }
}

pub(crate) fn queue_insert_resource(
    commands: &mut Commands,
    type_path: String,
    serialized_value: Vec<u8>,
) {
    commands.queue(InsertResourceCommand {
        type_path,
        serialized_value,
    });
}

pub(crate) fn queue_remove_resource(commands: &mut Commands, type_path: String) {
    commands.queue(RemoveResourceCommand { type_path });
}

struct InsertResourceCommand {
    type_path: String,
    serialized_value: Vec<u8>,
}

impl Command for InsertResourceCommand {
    fn apply(self, world: &mut World) {
        if let Err(err) = insert_resource(world, self.type_path, self.serialized_value) {
            panic!("failed to insert resource from command: {err}");
        }
    }
}

struct RemoveResourceCommand {
    type_path: String,
}

impl Command for RemoveResourceCommand {
    fn apply(self, world: &mut World) {
        let registry = world.get_resource::<WasmResourceRegistry>().cloned();
        let component_id = if let Some(type_id) = world
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry registered")
            .read()
            .get_with_type_path(&self.type_path)
            .map(|registration| registration.type_id())
        {
            world.components().get_resource_id(type_id)
        } else {
            registry.and_then(|registry| registry.0.get(&self.type_path).copied())
        };

        if let Some(component_id) = component_id {
            let _ = world.remove_resource_by_id(component_id);
        }
    }
}

pub(crate) fn insert_resource(
    world: &mut World,
    type_path: String,
    serialized_value: Vec<u8>,
) -> Result<()> {
    let type_registry = world
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry registered")
        .clone();

    let known = {
        let registry = type_registry.read();
        if let Some(type_registration) = registry.get_with_type_path(&type_path) {
            let reflect_resource = type_registration
                .data::<ReflectResource>()
                .cloned()
                .ok_or_else(|| {
                    anyhow!("{type_path} is registered but missing ReflectResource data")
                })?;
            let codec = world
                .get_resource::<CodecResource>()
                .expect("CodecResource registered");
            let output = codec.decode_reflect(&serialized_value, type_registration, &registry)?;
            Some((reflect_resource, output))
        } else {
            None
        }
    };

    if let Some((reflect_resource, output)) = known {
        let registry = type_registry.read();
        reflect_resource.apply_or_insert(world, output.as_partial_reflect(), &registry);
    } else {
        let component_id = get_wasm_resource_id(&type_path, world);
        let value = WasmResourceValue { serialized_value };
        OwningPtr::make(value, |ptr| unsafe {
            world.insert_resource_by_id(component_id, ptr, MaybeLocation::caller());
        });
    }

    Ok(())
}

pub(crate) fn get_resource(
    resources: &FilteredResourcesMut<'_, '_>,
    resource: &ResourceRef,
    type_registry: &AppTypeRegistry,
    codec: &CodecResource,
) -> Result<Vec<u8>> {
    let val = resources.as_readonly().get_by_id(resource.component_id())?;

    if let Some(type_id) = resource.type_id {
        let type_registry = type_registry.read();
        let type_registration = type_registry
            .get(type_id)
            .expect("ResourceRef type_id be registered");
        let reflect_from_ptr = type_registration
            .data::<ReflectFromPtr>()
            .expect("ReflectFromPtr to be registered");
        let reflect = unsafe { reflect_from_ptr.as_reflect(val) };
        Ok(codec.encode_reflect(reflect, &type_registry)?)
    } else {
        let value = unsafe { val.deref::<WasmResourceValue>() };
        Ok(value.serialized_value.clone())
    }
}

pub(crate) fn set_resource(
    resources: &mut FilteredResourcesMut<'_, '_>,
    resource: &ResourceRef,
    serialized_value: Vec<u8>,
    type_registry: &AppTypeRegistry,
    codec: &CodecResource,
) -> Result<()> {
    let mut val = resources.get_mut_by_id(resource.component_id())?;

    if let Some(type_id) = resource.type_id {
        let type_registry = type_registry.read();
        let type_registration = type_registry
            .get(type_id)
            .expect("ResourceRef type_id be registered");
        let reflect_from_ptr = type_registration
            .data::<ReflectFromPtr>()
            .expect("ReflectFromPtr to be registered");
        let boxed_dyn_reflect =
            codec.decode_reflect(&serialized_value, type_registration, &type_registry)?;
        let reflect = unsafe { reflect_from_ptr.as_reflect_mut(val.as_mut()) };
        reflect.apply(boxed_dyn_reflect.as_partial_reflect());
        Ok(())
    } else {
        let value = unsafe { val.as_mut().deref_mut::<WasmResourceValue>() };
        value.serialized_value = serialized_value;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_reflect::Reflect;
    use serde::{Deserialize, Serialize};

    #[derive(Resource, Reflect, Default, PartialEq, Debug, Serialize, Deserialize)]
    #[reflect(Resource)]
    struct KnownCounter(u32);

    #[test]
    fn known_resources_round_trip_via_reflection() {
        let mut world = World::new();
        world.init_resource::<AppTypeRegistry>();
        world.insert_resource(CodecResource::default());
        {
            let registry = world.resource_mut::<AppTypeRegistry>();
            let mut registry = registry.write();
            registry.register::<KnownCounter>();
            registry.register_type_data::<KnownCounter, ReflectResource>();
        }
        world.insert_resource(KnownCounter(3));

        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let codec = CodecResource::default();
        let resource = ResourceRef::new(std::any::type_name::<KnownCounter>(), &mut world).unwrap();
        let mut filtered = FilteredResourcesMut::from(&mut world);

        let bytes = get_resource(&filtered, &resource, &type_registry, &codec).unwrap();
        let decoded: KnownCounter = crate::serialize::wasvy_decode(&bytes).unwrap();
        assert_eq!(decoded, KnownCounter(3));

        set_resource(
            &mut filtered,
            &resource,
            crate::serialize::wasvy_encode(&KnownCounter(9)).unwrap(),
            &type_registry,
            &codec,
        )
        .unwrap();

        assert_eq!(world.resource::<KnownCounter>().0, 9);
    }

    #[test]
    fn guest_resources_round_trip_as_serialized_blobs() {
        let mut world = World::new();
        world.init_resource::<AppTypeRegistry>();
        world.insert_resource(CodecResource::default());

        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let codec = CodecResource::default();
        insert_resource(
            &mut world,
            "combat::PrivateState".to_string(),
            b"{\"value\":1}".to_vec(),
        )
        .unwrap();

        let resource = ResourceRef::new("combat::PrivateState", &mut world).unwrap();
        let mut filtered = FilteredResourcesMut::from(&mut world);
        let initial = get_resource(&filtered, &resource, &type_registry, &codec).unwrap();
        assert_eq!(initial, b"{\"value\":1}".to_vec());

        set_resource(
            &mut filtered,
            &resource,
            b"{\"value\":2}".to_vec(),
            &type_registry,
            &codec,
        )
        .unwrap();

        let updated = get_resource(&filtered, &resource, &type_registry, &codec).unwrap();
        assert_eq!(updated, b"{\"value\":2}".to_vec());
    }
}
