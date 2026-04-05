use anyhow::{Result, bail};
use bevy_reflect::{
    PartialReflect, TypeRegistration, TypeRegistry,
    serde::{TypedReflectDeserializer, TypedReflectSerializer},
};
use serde::{
    Serialize,
    de::{self, DeserializeSeed},
};

pub trait WasvyCodecImpl {
    type WasvySerializeValue;
    fn encode<T>(value: &T) -> Result<Vec<u8>>
    where
        T: ?Sized + Serialize;
    fn decode<'a, T>(v: &'a [u8]) -> Result<T>
    where
        T: de::Deserialize<'a>;

    fn encode_reflect(reflect: &dyn PartialReflect, registry: &TypeRegistry) -> Result<Vec<u8>>;
    fn decode_reflect(
        bytes: &[u8],
        registration: &TypeRegistration,
        registry: &TypeRegistry,
    ) -> Result<Box<dyn PartialReflect>>;
    fn parse_params(params: &[u8]) -> Result<Vec<Self::WasvySerializeValue>>;
    fn deserialize_arg(
        registry: &bevy_reflect::TypeRegistry,
        type_path: &str,
        value: &Self::WasvySerializeValue,
    ) -> Result<Box<dyn PartialReflect>>;
    fn get_type() -> String;
}

pub struct WasvyCodec;

#[cfg(feature = "serde_json")]
impl WasvyCodecImpl for WasvyCodec {
    type WasvySerializeValue = serde_json::Value;
    fn encode_reflect(reflect: &dyn PartialReflect, registry: &TypeRegistry) -> Result<Vec<u8>> {
        let serializer = TypedReflectSerializer::new(reflect, registry);
        Ok(serde_json::to_vec(&serializer)?)
    }

    fn decode_reflect(
        bytes: &[u8],
        registration: &TypeRegistration,
        registry: &TypeRegistry,
    ) -> Result<Box<dyn PartialReflect>> {
        let mut de = serde_json::Deserializer::from_slice(bytes);
        let reflect_deserializer = TypedReflectDeserializer::new(registration, registry);
        let boxed_dyn_reflect = reflect_deserializer.deserialize(&mut de)?;
        Ok(boxed_dyn_reflect)
    }

    fn encode<T>(value: &T) -> Result<Vec<u8>>
    where
        T: ?Sized + Serialize,
    {
        Ok(serde_json::to_vec(&value)?)
    }

    fn decode<'a, T>(v: &'a [u8]) -> Result<T>
    where
        T: de::Deserialize<'a>,
    {
        serde_json::from_slice(v).map_err(anyhow::Error::from)
    }

    fn parse_params(params: &[u8]) -> Result<Vec<Self::WasvySerializeValue>> {
        if params.is_empty() || params.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(Vec::new());
        }

        let value: Self::WasvySerializeValue = serde_json::from_slice(params)?;
        match value {
            Self::WasvySerializeValue::Null => Ok(Vec::new()),
            Self::WasvySerializeValue::Array(values) => Ok(values),
            other => bail!("Expected JSON array for params, got {other}"),
        }
    }

    fn deserialize_arg(
        registry: &bevy_reflect::TypeRegistry,
        type_path: &str,
        value: &Self::WasvySerializeValue,
    ) -> Result<Box<dyn PartialReflect>> {
        let registration = registry
            .get_with_type_path(type_path)
            .ok_or_else(|| anyhow::anyhow!("Type {type_path} is not registered"))?;

        let bytes = serde_json::to_vec(value)?;
        let mut de = serde_json::Deserializer::from_slice(&bytes);
        let reflect_de = TypedReflectDeserializer::new(registration, registry);
        let output: Box<dyn PartialReflect> = reflect_de.deserialize(&mut de)?;
        Ok(output)
    }
    fn get_type() -> String {
        "json".to_string()
    }
}
