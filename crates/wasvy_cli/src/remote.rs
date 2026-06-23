use core::time::Duration;
use std::{borrow::Borrow, collections::HashMap, fs, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, anyhow, bail};
use bevy_remote::{
    BrpPayload, BrpRequest, BrpResponse,
    http::{DEFAULT_ADDR, DEFAULT_PORT},
};
use derive_more::{Deref, DerefMut};
use error_collection::Errors;
use http::{Uri, uri::InvalidUri};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use wit_parser::Resolve;

use crate::{command::Logging, dependency::Dependency, named::Named, source::Source, watch::watch};

#[derive(Debug)]
pub struct Remote {
    pub dependencies: Vec<Dependency>,
    pub endpoint: RemoteUri,
    pub asset_dir: PathBuf,
    pub current_exe: PathBuf,
    pub name: String,
}

impl Remote {
    pub fn connect(
        endpoint: impl TryInto<RemoteUri, Error = impl Into<anyhow::Error>>,
    ) -> Result<Remote> {
        let endpoint = endpoint.try_into().map_err(Into::into)?;

        let res = endpoint.send("wasvy.metadata", Value::Null)?;

        let mut errors = Errors::new();
        let mut resolve = Resolve::new();
        let dependencies = res
            .get("interfaces")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .filter_map(|v| errors.collect(Dependency::new_with_resolve(v, &mut resolve)))
                    .map(|(dep, _)| dep)
                    .collect()
            })
            .unwrap_or_default();

        if !errors.is_empty() {
            return Err(errors.into());
        }

        let asset_dir = res
            .get("asset_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .context("unknown asset_dir")?;

        let current_exe = res
            .get("current_exe")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .context("unknown current_exe")?;

        let name = res
            .get("program_name")
            .and_then(|v| v.as_str())
            .context("unknown program_name")?
            .to_string();

        Ok(Remote {
            asset_dir,
            current_exe,
            dependencies,
            endpoint,
            name,
        })
    }

    /// Pushes a built source to the remote, returning the asset path.
    pub fn push(&self, source: &Source) -> Result<String> {
        if !source.is_wasm() {
            bail!("Source {source:?} must be built before pushing to remote")
        }

        let mods_dir = self.asset_dir.join("mods");
        let _ = fs::create_dir_all(&mods_dir);

        let from = source.path();
        let path = from.file_name().expect("is a wasm file").to_string_lossy();
        let to = mods_dir.join(path.as_ref());

        // This is obviously quite naive, but for now assume the remote shares the same filesystem
        fs::copy(from, &to).with_context(|| anyhow!("Copying from {from:?} to {to:?}"))?;

        Ok(format!("mods/{path}"))
    }

    /// Pulls a built source to the remote, returning the asset path.
    pub fn pull(&self, source: &Source) -> Result<String> {
        if !source.is_wasm() {
            bail!("Source {source:?} must be built before pulling from remote")
        }

        let mods_dir = self.asset_dir.join("mods");
        let _ = fs::create_dir_all(&mods_dir);

        let from = source.path();
        let path = from.file_name().expect("is a wasm file").to_string_lossy();
        let to = mods_dir.join(path.as_ref());

        // This is obviously quite naive, but for now assume the remote shares the same filesystem
        if let Err(error) = fs::remove_file(to)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }

        Ok(format!("mods/{path}"))
    }

    /// Returns a list of spawned mod instances for each mod asset
    pub fn list(&self) -> Result<HashMap<String, Vec<u64>>> {
        let value = self.endpoint.send("wasvy.mods.list", Value::Null)?;
        let map =
            serde_json::from_value(value).context("unexpected response from wasvy.mods.list")?;
        Ok(map)
    }

    /// Builds and spawns mod sources in the remote app with the given mod accesses
    pub fn spawn(
        &self,
        sources: impl IntoIterator<Item = impl Borrow<Source>>,
        access: impl IntoIterator<Item = Access>,
        logging: Logging,
    ) -> Result<Vec<u64>> {
        let mut errors = Errors::new();

        let access: Vec<Value> = access
            .into_iter()
            .filter_map(|access| errors.collect(serde_json::to_value(access)))
            .collect();

        let mut mods = Vec::new();
        for source in sources {
            let source = source.borrow();

            // Build the source, ensuring it is wasm
            let result = source
                .build(logging.clone())
                .with_context(|| anyhow!("building {source:?}"));
            let Some(source) = errors.collect(result) else {
                continue;
            };

            // Push to the remote
            let result = self.push(source.as_ref());
            let Some(path) = errors.collect(result) else {
                continue;
            };

            let mut map = Map::new();
            map.insert("path".into(), Value::String(path));
            map.insert("name".into(), Value::String(source.name().to_string()));
            map.insert("access".into(), Value::Array(access.clone()));
            mods.push(Value::Object(map));
        }

        let mut ids = None;
        if !mods.is_empty() && !access.is_empty() {
            let result = self.endpoint.send("wasvy.mods.spawn", Value::Array(mods));
            if let Some(value) = errors.collect(result) {
                ids = errors.collect(serde_json::from_value(value));
            }
        }

        errors.as_result().map(|_| ids.unwrap_or_default())
    }

    /// Despawns mods by their id
    pub fn despawn(&self, mods: impl IntoIterator<Item = u64>) -> Result<()> {
        let mods: Vec<Value> = mods.into_iter().map(Value::from).collect();
        if !mods.is_empty() {
            self.endpoint
                .send("wasvy.mods.despawn", Value::Array(mods))?;
        }
        Ok(())
    }

    pub fn load(
        &self,
        sources: impl IntoIterator<Item = impl Borrow<Source>>,
        logging: Logging,
    ) -> Result<()> {
        let mut errors = Errors::new();

        // Build a list of mod sources
        let mut mods = Vec::new();
        for source in sources {
            let source = source.borrow();

            // Build the source, ensuring it is wasm
            let result = source
                .build(logging.clone())
                .with_context(|| anyhow!("building {source:?}"));
            let Some(source) = errors.collect(result) else {
                continue;
            };

            // Push to the remote
            let result = self.push(source.as_ref());
            let Some(path) = errors.collect(result) else {
                continue;
            };

            mods.push((source.into_owned(), path));
        }

        // Unload existing mods before loading new ones
        let despawn = self
            .list()?
            .into_iter()
            .filter(|(path, _)| mods.iter().any(|(_, p)| path == p))
            .flat_map(|(_, ids)| ids.into_iter());
        errors.collect(
            self.despawn(despawn)
                .context("Despawning existing mods before loading new ones"),
        );

        let sources = mods.iter().map(|(source, _)| source);
        errors.collect(
            self.spawn(sources, [Access::World], logging)
                .context("Spawning loaded mods"),
        );

        errors.as_result()
    }

    pub fn unload(
        &self,
        sources: impl IntoIterator<Item = impl Borrow<Source>>,
        logging: Logging,
    ) -> Result<()> {
        let mut errors = Errors::new();

        // Build a list of built mod paths
        let mut paths = Vec::new();
        for source in sources {
            let source = source.borrow();

            // Build the source, ensuring it is wasm
            let result = source
                .build(logging.clone())
                .with_context(|| anyhow!("building {source:?}"));
            let Some(source) = errors.collect(result) else {
                continue;
            };

            // Push to the remote
            let result = self.pull(source.as_ref());
            let Some(path) = errors.collect(result) else {
                continue;
            };

            paths.push(path);
        }

        // Unload existing mods before loading new ones
        let despawn = self
            .list()?
            .into_iter()
            .filter(|(path, _)| paths.iter().any(|p| path == p))
            .flat_map(|(_, ids)| ids.into_iter());
        self.despawn(despawn).context("Despawning mods")?;

        Ok(())
    }

    pub fn watch(
        &self,
        sources: impl IntoIterator<Item = impl Borrow<Source>>,
        timeout: Duration,
        count: Option<usize>,
        logging: Logging,
    ) -> Result<()> {
        watch(sources, self, timeout, count, logging)
    }
}

#[derive(Debug, Clone, Deref, DerefMut)]
pub struct RemoteUri(pub Uri);

impl RemoteUri {
    pub fn new(port: u16) -> Self {
        format!("http://{}:{}", DEFAULT_ADDR, port).parse().unwrap()
    }
}

impl Default for RemoteUri {
    fn default() -> Self {
        Self::new(DEFAULT_PORT)
    }
}

impl From<Uri> for RemoteUri {
    fn from(value: Uri) -> Self {
        RemoteUri(value)
    }
}

impl FromStr for RemoteUri {
    type Err = InvalidUri;

    #[inline]
    fn from_str(s: &str) -> core::result::Result<Self, InvalidUri> {
        let uri = Uri::from_str(s)?;
        Ok(Self(uri))
    }
}

impl RemoteUri {
    /// Send a BRP JSON-RPC 2.0 request and return the result
    pub fn send(&self, method: impl Into<String>, params: Value) -> Result<Value> {
        let body = BrpRequest {
            method: method.into(),
            id: None,
            params: match params {
                Value::Null => None,
                params => Some(params),
            },
        };

        let response: BrpResponse = ureq::post(&self.0)
            .send_json(&body)
            .context("making BrpRequest")?
            .body_mut()
            .read_json()
            .context("parsing BrpPayload")?;

        match response.payload {
            BrpPayload::Error(error) => Err(anyhow!("BRP error: {error:#?}")),
            BrpPayload::Result(value) => Ok(value),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub enum Access {
    #[default]
    World,
    Sandbox(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_remote_endpoint() {
        let _ = RemoteUri::default();
    }
}
