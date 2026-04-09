use std::collections::HashMap;

use anyhow::{Result, anyhow};
use bevy_asset::Assets;
use bevy_ecs::prelude::*;
use serde_json::Value;

use crate::prelude::*;

/// List spawned mods, returning a Map of
pub fn list(
    _: In<Option<Value>>,
    query: Query<(Entity, &Mod)>,
    assets: Res<Assets<ModAsset>>,
) -> Result<Value> {
    let mut mods: HashMap<String, Vec<u64>> = HashMap::new();
    for (entity, a_mod) in query.iter() {
        let Some(path) = assets
            .get(a_mod.asset().id())
            .and_then(|asset| asset.path().to_str())
        else {
            continue;
        };

        let id = entity.to_bits();
        mods.entry(path.to_string())
            .and_modify(|vec| {
                vec.push(id);
            })
            .or_insert(vec![id]);
    }
    Ok(serde_json::to_value(mods)?)
}

/// Spawn an instance of a mod, returning its mod id (u64).
///
/// Required params:
/// - **path** - Relative path to a pre-compiled wasm file located in the app's resources folder
///
/// Optional params:
/// - **name** - A name for the mod
pub fn spawn(params: In<Option<Value>>, mut mods: Mods, mut commands: Commands) -> Result<Value> {
    let path = params
        .as_ref()
        .and_then(|value| value["path"].as_str())
        .map(|s| s.to_string())
        .ok_or(anyhow!("Missing \"path\" param"))?;

    let mod_id = mods.spawn(&path);
    mods.enable_access(mod_id, ModAccess::World);

    if let Some(name) = params
        .as_ref()
        .and_then(|value| value["name"].as_str())
        .map(|s| s.to_string())
    {
        commands.entity(mod_id).insert(Name::new(name));
    }

    Ok(mod_id.to_bits().into())
}

/// Despawns an instance of a mod
///
/// Required params:
/// - **id** - A mod id (u64)
pub fn despawn(params: In<Option<Value>>, mut mods: Mods) -> Result<Value> {
    let id = params
        .as_ref()
        .and_then(|value| value["id"].as_u64())
        .map(Entity::from_bits)
        .ok_or(anyhow!("Missing \"id\" param"))?;

    mods.despawn(id);

    Ok(Value::Null)
}
