use std::collections::HashMap;

use anyhow::Result;
use bevy_asset::Assets;
use bevy_ecs::prelude::*;
use serde_json::Value;

use crate::prelude::*;

/// List spawned mods, returning a mapping of wasm mods to the mod instance entity ids
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
