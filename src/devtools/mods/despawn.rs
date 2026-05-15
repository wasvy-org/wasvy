use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use serde_json::Value;

use crate::prelude::*;

/// Despawns one or more instances of a mod
///
/// Provide either a single u64 mod entity id or a vector of entity ids
pub fn despawn(params: In<Option<Value>>, mut mods: Mods) -> Result<Value> {
    let values = match &*params {
        Some(Value::Null) | None => bail!("expected at least one param"),
        Some(Value::Array(values)) => values.iter().collect(),
        Some(value) => vec![value],
    };

    for entity in values
        .into_iter()
        .filter_map(Value::as_u64)
        .map(Entity::from_bits)
    {
        mods.despawn(entity);
    }

    Ok(Value::Null)
}
