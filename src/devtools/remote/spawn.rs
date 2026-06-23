use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use error_collection::Errors;
use serde::Deserialize;
use serde_json::Value;

use crate::{access::ModAccess, mods::Mods};

/// Spawn one or more instances of a mod, returning spawned mod ids (an array of u64).
///
/// Either pass 1 object with the params below or an array of such objects.
///
/// Required params:
/// - **path** - Relative path to a pre-compiled wasm file located in the app's resources folder
///
/// Optional params:
/// - **name** - A name for the mod
/// - **access** - An array of ModAccess
pub fn spawn(mut params: In<Option<Value>>, mut mods: Mods) -> Result<Value> {
    let values: Vec<Value> = match params.take().unwrap_or(Value::Null) {
        Value::Null => bail!("expected at least one param"),
        Value::Array(values) => values,
        value => vec![value],
    };

    let mut errors = Errors::new();
    let mod_ids: Vec<Value> = values
        .into_iter()
        .filter_map(|value| errors.collect(serde_json::from_value(value)))
        .map(|Instance { path, name, access }| {
            let mod_id = mods.spawn(path, name);
            for access in access {
                mods.enable_access(mod_id, access);
            }
            mod_id
        })
        .map(|entity| entity.to_bits().into())
        .collect();

    errors.as_result().map(|_| Value::Array(mod_ids))
}

#[derive(Deserialize)]
struct Instance {
    path: String,
    name: Option<String>,
    #[serde(default = "default_mod_access")]
    access: Vec<ModAccess>,
}

fn default_mod_access() -> Vec<ModAccess> {
    vec![ModAccess::World]
}
