use std::{fs, path::PathBuf};

use anyhow::Result;
use bevy_asset::{AssetPlugin, io::file::FileAssetReader};
use bevy_ecs::prelude::*;
use serde::Serialize;
use serde_json::Value;

use crate::devtools::Devtools;

#[derive(Resource, Serialize)]
pub(super) struct Metadata {
    #[serde(flatten)]
    devtools: Devtools,
    asset_dir: PathBuf,
}

impl Metadata {
    pub(super) fn new(devtools: Devtools, asset_plugin: &AssetPlugin) -> Self {
        let reader = FileAssetReader::new(&asset_plugin.file_path);
        let asset_dir =
            fs::canonicalize(reader.root_path()).expect("able to canonicalize asset file path");
        Self {
            devtools,
            asset_dir,
        }
    }
}

pub fn metadata(_: In<Option<Value>>, metadata: Res<Metadata>) -> Result<Value> {
    Ok(serde_json::to_value(&*metadata)?)
}
