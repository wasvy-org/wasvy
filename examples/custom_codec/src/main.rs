use anyhow::{Result, bail};
use bevy::{
    dev_tools::fps_overlay::FpsOverlayPlugin,
    prelude::*,
    reflect::{
        TypeRegistration, TypeRegistry,
        serde::{TypedReflectDeserializer, TypedReflectSerializer},
    },
};
use bevy_egui::EguiPlugin;
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use serde::de::DeserializeSeed;
use wasvy::{plugin::ModloaderPlugin, serialize::WasvyCodec};

#[derive(Default)]
pub struct CustomCodec;

impl WasvyCodec for CustomCodec {
    fn encode_reflect(
        &self,
        reflect: &dyn PartialReflect,
        registry: &TypeRegistry,
    ) -> Result<Vec<u8>> {
        let serializer = TypedReflectSerializer::new(reflect, registry);
        Ok(rmp_serde::to_vec_named(&serializer)?)
    }

    fn decode_reflect(
        &self,
        bytes: &[u8],
        registration: &TypeRegistration,
        registry: &TypeRegistry,
    ) -> Result<Box<dyn PartialReflect>> {
        let mut de = rmp_serde::Deserializer::new(bytes);
        let reflect_deserializer = TypedReflectDeserializer::new(registration, registry);
        let boxed_dyn_reflect = reflect_deserializer.deserialize(&mut de)?;
        Ok(boxed_dyn_reflect)
    }

    fn get_type(&self) -> String {
        "msgpack".to_string()
    }

    fn decode_reflect_args(
        &self,
        params: &[u8],
        type_path: &[&str],
        registry: &TypeRegistry,
    ) -> Result<Vec<Option<Box<dyn PartialReflect>>>> {
        if params.is_empty() || params.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(vec![]);
        }

        let value = rmp_serde::from_slice(params)?;

        let args = match value {
            rmpv::Value::Nil => Vec::new(),
            rmpv::Value::Array(values) => values,
            _ => bail!("Expected JSON array for params, got {}", value),
        };

        let mut output = Vec::new();

        for (type_path, value) in type_path.iter().zip(args.iter()) {
            let registration = registry
                .get_with_type_path(type_path)
                .ok_or_else(|| anyhow::anyhow!("Type {type_path} is not registered"))?;

            let bytes = rmp_serde::to_vec(value)?;
            let mut de = rmp_serde::Deserializer::new(&*bytes);
            let reflect_de = TypedReflectDeserializer::new(registration, registry);
            output.push(Some(reflect_de.deserialize(&mut de)?));
        }

        Ok(output)
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            // Next, add the [`ModloaderPlugin`] ;)
            ModloaderPlugin::default().with_codec(CustomCodec),
            // Plus some helpers for the example
            FpsOverlayPlugin::default(),
            EguiPlugin::default(),
            WorldInspectorPlugin::new(),
        ))
        .run();
}
