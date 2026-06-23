use anyhow::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::resource::Resource;
use bevy_reflect::{PartialReflect, TypeRegistration, TypeRegistry};

#[cfg(feature = "serde_json")]
use serde::de::DeserializeSeed;

#[derive(Resource, Deref, DerefMut)]
pub struct CodecResource(pub Box<dyn WasvyCodec>);

impl CodecResource {
    pub fn new(codec: impl WasvyCodec) -> Self {
        Self(Box::new(codec))
    }
}

pub trait WasvyCodec: Send + Sync + 'static {
    fn encode_reflect(
        &self,
        reflect: &dyn PartialReflect,
        registry: &TypeRegistry,
    ) -> Result<Vec<u8>>;
    fn decode_reflect(
        &self,
        bytes: &[u8],
        registration: &TypeRegistration,
        registry: &TypeRegistry,
    ) -> Result<Box<dyn PartialReflect>>;
    fn decode_reflect_args(
        &self,
        params: &[u8],
        type_path: &[&str],
        registry: &TypeRegistry,
    ) -> Result<Vec<Option<Box<dyn PartialReflect>>>>;
    fn get_type(&self) -> String;
}

#[derive(Default, Resource)]
pub struct JsonCodec;

#[cfg(feature = "serde_json")]
impl Default for CodecResource {
    fn default() -> Self {
        Self(Box::new(JsonCodec))
    }
}

#[cfg(feature = "serde_json")]
impl WasvyCodec for JsonCodec {
    fn encode_reflect(
        &self,
        reflect: &dyn PartialReflect,
        registry: &TypeRegistry,
    ) -> Result<Vec<u8>> {
        let serializer = bevy_reflect::serde::TypedReflectSerializer::new(reflect, registry);
        Ok(serde_json::to_vec(&serializer)?)
    }

    fn decode_reflect(
        &self,
        bytes: &[u8],
        registration: &TypeRegistration,
        registry: &TypeRegistry,
    ) -> Result<Box<dyn PartialReflect>> {
        let mut de = serde_json::Deserializer::from_slice(bytes);
        let reflect_deserializer =
            bevy_reflect::serde::TypedReflectDeserializer::new(registration, registry);
        let boxed_dyn_reflect = reflect_deserializer.deserialize(&mut de)?;
        Ok(boxed_dyn_reflect)
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

        let value = serde_json::from_slice(params)?;

        let args = match value {
            serde_json::Value::Null => Vec::new(),
            serde_json::Value::Array(values) => values,
            _ => bail!("Expected JSON array for params, got {}", value),
        };

        let mut output = Vec::new();

        for (type_path, value) in type_path.iter().zip(args.iter()) {
            let registration = registry
                .get_with_type_path(type_path)
                .ok_or_else(|| anyhow::anyhow!("Type {type_path} is not registered"))?;

            let bytes = serde_json::to_vec(value)?;
            let mut de = serde_json::Deserializer::from_slice(&bytes);
            let reflect_de =
                bevy_reflect::serde::TypedReflectDeserializer::new(registration, registry);
            output.push(Some(reflect_de.deserialize(&mut de)?));
        }

        Ok(output)
    }

    fn get_type(&self) -> String {
        "json".to_string()
    }
}

#[cfg(feature = "serde_json")]
pub fn wasvy_encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: ?Sized + serde::Serialize,
{
    Ok(serde_json::to_vec(&value)?)
}

#[cfg(feature = "serde_json")]
pub fn wasvy_decode<'a, T>(v: &'a [u8]) -> Result<T>
where
    T: serde::Deserialize<'a>,
{
    serde_json::from_slice(v).map_err(anyhow::Error::from)
}
