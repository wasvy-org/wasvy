use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use serde_json::Value;

use crate::prelude::*;

/// Despawns one or more instances of a mod
///
/// Provide either a single u64 mod entity id or a vector of entity ids
pub fn despawn(mut params: In<Option<Value>>, mut mods: Mods) -> Result<Value> {
    let values: Vec<Value> = match params.take().unwrap_or(Value::Null) {
        Value::Null => bail!("expected at least one param"),
        Value::Array(values) => values,
        value => vec![value],
    };

    for entity in values
        .iter()
        .filter_map(Value::as_u64)
        .map(Entity::from_bits)
    {
        mods.despawn(entity);
    }

    Ok(Value::Null)
}

/// Despawns all mod instances
pub fn despawn_all(_: In<Option<Value>>, mut mods: Mods) -> Result<Value> {
    mods.despawn_all();

    Ok(Value::Null)
}
