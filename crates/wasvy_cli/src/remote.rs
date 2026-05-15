use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use bevy_remote::{
    BrpPayload, BrpRequest,
    http::{DEFAULT_ADDR, DEFAULT_PORT},
};
use error_collection::Errors;
use http::{Uri, uri::InvalidUri};
use serde::{Deserialize, Serialize};

use crate::dependency::Dependency;

#[derive(Debug, Clone)]
pub struct Remote {
    pub dependencies: Vec<Dependency>,
    pub name: String,
}

pub struct RemoteEndpoint(pub Uri);

impl Default for RemoteEndpoint {
    fn default() -> Self {
        format!("http://{}:{}", DEFAULT_ADDR, DEFAULT_PORT)
            .parse()
            .unwrap()
    }
}

impl From<Uri> for RemoteEndpoint {
    fn from(value: Uri) -> Self {
        RemoteEndpoint(value)
    }
}

impl FromStr for RemoteEndpoint {
    type Err = InvalidUri;

    #[inline]
    fn from_str(s: &str) -> core::result::Result<Self, InvalidUri> {
        let uri = Uri::from_str(s)?;
        Ok(Self(uri))
    }
}

impl Remote {
    pub fn connect<T>(endpoint: T) -> Result<Remote>
    where
        T: TryInto<RemoteEndpoint>,
        T::Error: Into<anyhow::Error>,
    {
        let endpoint = endpoint.try_into().map_err(Into::into)?;

        let res = brp_request(endpoint, "wasvy/metadata", None)?;

        let mut errors = Errors::new();
        let dependencies = res
            .get("interfaces")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .filter_map(|v| errors.collect(Dependency::new(v)))
                    .collect()
            })
            .unwrap_or_default();

        if !errors.is_empty() {
            return Err(errors.into());
        }

        let name = res
            .get("program_name")
            .and_then(|v| v.as_str())
            .context("unknown program_name")?
            .to_string();

        Ok(Remote { dependencies, name })
    }
}

/// Send a BRP JSON-RPC 2.0 request and return the result
pub fn brp_request(
    endpoint: impl Into<RemoteEndpoint>,
    method: impl Into<String>,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let uri = endpoint.into().0;
    let body = BrpRequest {
        jsonrpc: "2.0".into(),
        method: method.into(),
        id: None,
        params,
    };

    let response: BrpResponse = ureq::post(uri)
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

/// A response according to BRP.
///
/// [bevy_remote::BrpResponse] is not deserializable. See https://github.com/bevyengine/bevy/pull/24305
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BrpResponse {
    #[serde(flatten)]
    pub payload: BrpPayload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_remote_endpoint() {
        let _ = RemoteEndpoint::default();
    }
}
