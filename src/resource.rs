use bevy_ecs::{resource::Resource, world::World};
use bevy_reflect::Reflect;

/// This resource is the wrapper resource for all the Bevy resources that are registered in a
/// WASM.
///
/// # Description
///
/// When you call the spawn method in WASM you need to provide a component id, that id is used to
/// add a new [`WasmResource`] under that id with the `serialized_value` that is given.
///
/// This approach makes it possible to register resources that don't exist in Rust.
#[derive(Resource, Reflect)]
pub struct WasmResource {
    pub serialized_value: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct InsertResources(pub Vec<(String, WasmResource)>);

impl InsertResources {
    pub(crate) fn push(&mut self, resource: (String, WasmResource)) {
        self.0.push(resource);
    }

    pub(crate) fn add_resources(
        &self,
        world: &mut World,
        type_registry: &AppTypeRegistry,
        codec: &CodecResource,
        entity: Entity,
        type_path: String,
        serialized_value: Vec<u8>,
    ) {
        for (type_path, resource) in self.0.iter() {}
    }
}
