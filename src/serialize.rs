use anyhow::{Result, bail};
use bevy_reflect::{
    PartialReflect, TypeRegistration, TypeRegistry,
    serde::{TypedReflectDeserializer, TypedReflectSerializer},
};
use serde::{
    Serialize,
    de::{self, DeserializeSeed},
};

#[cfg(feature = "serde_json")]
type WasvyValue = serde_json::Value;

#[cfg(feature = "rmp")]
type WasvyValue = rmpv::Value;

pub trait WasvyCodecImpl {
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
    fn parse_params(params: &[u8]) -> Result<Vec<WasvyValue>>;
    fn deserialize_arg(
        registry: &bevy_reflect::TypeRegistry,
        type_path: &str,
        value: &WasvyValue,
    ) -> Result<Box<dyn PartialReflect>>;
    fn get_type() -> String;
}

pub struct WasvyCodec;

#[cfg(feature = "serde_json")]
impl WasvyCodecImpl for WasvyCodec {
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

    fn parse_params(params: &[u8]) -> Result<Vec<WasvyValue>> {
        if params.is_empty() || params.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(Vec::new());
        }

        let value: WasvyValue = serde_json::from_slice(params)?;
        match value {
            WasvyValue::Null => Ok(Vec::new()),
            WasvyValue::Array(values) => Ok(values),
            other => bail!("Expected JSON array for params, got {other}"),
        }
    }

    fn deserialize_arg(
        registry: &bevy_reflect::TypeRegistry,
        type_path: &str,
        value: &WasvyValue,
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

#[cfg(feature = "rmp")]
impl WasvyCodecImpl for WasvyCodec {
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

    fn parse_params(params: &[u8]) -> Result<Vec<WasvyValue>> {
        if params.is_empty() || params.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(Vec::new());
        }

        let value: WasvyValue = rmp_serde::from_slice(params)?;
        match value {
            WasvyValue::Nil => Ok(Vec::new()),
            WasvyValue::Array(values) => Ok(values),
            other => bail!("Expected MessagePack array for params, got {other}"),
        }
    }

    fn deserialize_arg(
        registry: &bevy_reflect::TypeRegistry,
        type_path: &str,
        value: &WasvyValue,
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
