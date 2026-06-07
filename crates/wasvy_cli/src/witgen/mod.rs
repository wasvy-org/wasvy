//! Cli implementation for generating wit from mods.
//!
//! These types back the `wasvy:ecs` resources exposed to guest mods.

use std::{collections::HashSet, path::Path};

mod app;
mod host;
mod system;

use anyhow::*;
use askama::Template;
use error_collection::Errors;
use semver::Version;
pub(super) use wasmtime::component::Resource;
use wasmtime::component::Val;

pub mod bindings;

pub use app::*;
pub use host::*;
pub use system::*;

use crate::{fs::WriteTo, named::Named, runtime::Runtime, source::Source};

pub fn write_guest_wit(source: &Source) -> Result<()> {
    let wit = Wit::new(source)?;
    wit.write(source.path())
}

/// Used to generate wit bindings for a [Source]
#[derive(askama::Template)]
#[template(path = "./wit/guest.wit")]
pub struct Wit {
    pub namespace: String,
    pub name: String,
    pub wasvy_wit_version: Version,
    pub params: Vec<SystemParam>,
    pub systems: Vec<WasmSystem>,
}

impl Wit {
    pub fn new(config: impl TryInto<WitConfig, Error = impl Into<Error>>) -> Result<Self> {
        let WitConfig {
            name,
            namespace,
            systems,
            wasvy_wit_version,
        } = config.try_into().map_err(Into::into)?;

        let mut errors = Errors::new();
        if !is_valid_wit_ident(&name) {
            errors.push(anyhow!("invalid wit identifier name: {name}"));
        }

        if !is_valid_wit_ident(&namespace) {
            errors.push(anyhow!("invalid wit identifier namespace: {namespace}"));
        }

        let mut params: Vec<_> = systems
            .iter()
            .flat_map(|sys| sys.args.iter().map(|arg| arg.param.clone()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        // Predictable order is important so we avoid overwriting files that havn't changed
        params.sort();

        errors.as_result().map(|_| Self {
            name,
            namespace,
            params,
            systems,
            wasvy_wit_version,
        })
    }
}

impl TryInto<String> for Wit {
    type Error = Error;

    fn try_into(self) -> Result<String> {
        let mut buffer = Vec::new();
        self.write_into(&mut buffer)?;
        Ok(String::from_utf8(buffer)?
            // First line is metadata
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

pub struct WitConfig {
    pub namespace: String,
    pub name: String,
    pub wasvy_wit_version: Version,
    pub systems: Vec<WasmSystem>,
}

impl Default for WitConfig {
    fn default() -> Self {
        Self {
            name: Default::default(),
            namespace: Default::default(),
            wasvy_wit_version: Version::parse("0.0.7").unwrap(),
            systems: Default::default(),
        }
    }
}

impl From<&Runtime> for WitConfig {
    fn from(runtime: &Runtime) -> Self {
        let mut config = Self {
            namespace: runtime.namespace().to_string(),
            ..Default::default()
        };
        if let Some(dependency) = runtime.find_dependency("wasvy", "ecs") {
            config.wasvy_wit_version = dependency.version.clone();
        }
        config
    }
}

impl TryFrom<&Source> for WitConfig {
    type Error = Error;

    fn try_from(source: &Source) -> Result<Self> {
        let mut config: Self = source.runtime().into();
        config.name = source.name().to_string();

        let engine = Default::default();
        let mut linker = linker(&engine);
        config.systems =
            get_systems(source.path(), &mut linker, Default::default()).context("get_systems")?;

        if let Some(dependency) = source.runtime().find_dependency("wasvy", "ecs") {
            config.wasvy_wit_version = dependency.version.clone();
        }

        Ok(config)
    }
}

/// The default wit template for scaffolding new projects
pub struct ScaffoldWit(WitConfig);

impl ScaffoldWit {
    pub fn new(name: impl AsRef<str>, runtime: &Runtime) -> Self {
        let mut config: WitConfig = runtime.into();
        config.name = name.as_ref().to_string();
        config.systems = vec![
            WasmSystem {
                args: vec![Arg {
                    name: "commands".into(),
                    param: SystemParam::Commands,
                }],
                desc: "An example system that runs on ModSetup".into(),
                name: "start".into(),
            },
            WasmSystem {
                args: vec![Arg {
                    name: "query".into(),
                    param: SystemParam::Query,
                }],
                desc: "Another example system that runs every Update".into(),
                name: "update".into(),
            },
        ];
        Self(config)
    }
}

impl From<ScaffoldWit> for WitConfig {
    fn from(value: ScaffoldWit) -> Self {
        value.0
    }
}

pub fn linker(engine: &wasmtime::Engine) -> wasmtime::component::Linker<host::Host> {
    let mut linker = wasmtime::component::Linker::new(engine);

    type Data = wasmtime::component::HasSelf<Host>;
    bindings::add_to_linker::<_, Data>(&mut linker, |state| state)
        .expect("implement wasvy wit interface");

    linker
}

pub fn get_systems(
    path: impl AsRef<Path>,
    linker: &mut wasmtime::component::Linker<Host>,
    host: Host,
) -> Result<Vec<WasmSystem>> {
    let mut store = wasmtime::Store::new(linker.engine(), host);

    let component = wasmtime::component::Component::from_file(linker.engine(), &path)?;
    let instance = linker
        .instantiate(&mut store, &component)
        .context("Failed to instantiate component")?;

    let app = store
        .data_mut()
        .table
        .push(WasmApp)
        .expect("table has space left")
        .try_into_resource_any(&mut store)
        .expect("table has space left");

    let func = instance
        .get_func(&mut store, "setup")
        .context("missing setup function")?;

    func.call(&mut store, &[Val::Resource(app)], &mut [])
        .context("failed to run the \"setup\" wasm function")?;

    Ok(store.into_data().systems)
}

fn is_valid_wit_ident(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && value
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
        && value
            .chars()
            .last()
            .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use crate::runtime::Config;

    use super::*;

    #[test]
    fn scaffold() {
        let mut config = Config::default();
        config.namespace = "test".into();
        let runtime = Runtime::new(config).unwrap();
        let wit = Wit::new(ScaffoldWit::new("game", &runtime)).unwrap();

        let output: String = wit.try_into().unwrap();

        let first_line = output.lines().next().unwrap();
        assert_eq!(first_line, "package test:game;");
        assert!(output.contains(".{ commands, query, };"));
        assert!(output.contains("export start: func(commands: commands);"));
        assert!(output.contains("export update: func(query: query);"));
    }
}
