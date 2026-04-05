use anyhow::{Result, bail};
use bevy::{dev_tools::fps_overlay::FpsOverlayPlugin, prelude::*, reflect::{TypeRegistration, TypeRegistry, serde::{TypedReflectDeserializer, TypedReflectSerializer}}};
use bevy_egui::EguiPlugin;
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use serde::{Serialize, de::{self, DeserializeSeed}};
use wasvy::{plugin::ModloaderPlugin, serialize::WasvyCodecImpl};

pub struct CustomCodec;

impl WasvyCodecImpl for CustomCodec {
    type WasvySerializeValue = rmpv::Value;

    fn encode<T>(value: &T) -> Result<Vec<u8>>
    where
        T: ?Sized + Serialize,
    {
        Ok(rmp_serde::to_vec_named(value)?)
    }

    fn decode<'a, T>(v: &'a [u8]) -> Result<T>
    where
        T: de::Deserialize<'a>,
    {
        rmp_serde::from_slice(v).map_err(anyhow::Error::from)
    }

    fn encode_reflect(reflect: &dyn PartialReflect, registry: &TypeRegistry) -> Result<Vec<u8>> {
        let serializer = TypedReflectSerializer::new(reflect, registry);
        Ok(rmp_serde::to_vec_named(&serializer)?)
    }

    fn decode_reflect(
        bytes: &[u8],
        registration: &TypeRegistration,
        registry: &TypeRegistry,
    ) -> Result<Box<dyn PartialReflect>> {
        let mut de = rmp_serde::Deserializer::new(bytes);
        let reflect_deserializer = TypedReflectDeserializer::new(registration, registry);
        let boxed_dyn_reflect = reflect_deserializer.deserialize(&mut de)?;
        Ok(boxed_dyn_reflect)
    }

    fn parse_params(params: &[u8]) -> Result<Vec<Self::WasvySerializeValue>> {
        if params.is_empty() || params.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(Vec::new());
        }

        let value: Self::WasvySerializeValue = rmp_serde::from_slice(params)?;
        match value {
            Self::WasvySerializeValue::Nil => Ok(Vec::new()),
            Self::WasvySerializeValue::Array(values) => Ok(values),
            other => bail!("Expected MessagePack array for params, got {other}"),
        }
    }

    fn deserialize_arg(
        registry: &TypeRegistry,
        type_path: &str,
        value: &Self::WasvySerializeValue,
    ) -> Result<Box<dyn PartialReflect>> {
        let registration = registry
            .get_with_type_path(type_path)
            .ok_or_else(|| anyhow::anyhow!("Type {type_path} is not registered"))?;

        let bytes = rmp_serde::to_vec(value)?;
        let mut de = rmp_serde::Deserializer::new(&*bytes);
        let reflect_de = TypedReflectDeserializer::new(registration, registry);
        let output: Box<dyn PartialReflect> = reflect_de.deserialize(&mut de)?;
        Ok(output)
    }

    fn get_type() -> String {
        "msgpack".to_string()
    }
}

fn main() {
        App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins((
            // Next, add the [`ModloaderPlugin`] ;)
            ModloaderPlugin::default(),
            // Plus some helpers for the example
            FpsOverlayPlugin::default(),
            EguiPlugin::default(),
            WorldInspectorPlugin::new(),
        ))
        .run();
}
