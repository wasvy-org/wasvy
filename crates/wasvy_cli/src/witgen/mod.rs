//! Cli implementation for generating wit from mods.
//!
//! These types back the `wasvy:ecs` resources exposed to guest mods.

use std::{collections::HashSet, path::Path};

mod app;
mod host;
mod system;

use anyhow::Context;
use wasmtime::component::Val;
pub(super) use wasmtime::{Result, component::Resource};

pub mod bindings;

pub use app::*;
pub use host::*;
pub use system::*;

use crate::{fs::WriteTo, named::Named, source::Source};

pub fn write_guest_wit(source: &Source) -> Result<()> {
    let wit = GuestWit::new(source)?;
    wit.write(source.path())
}

#[derive(askama::Template)]
#[template(path = "./wit/guest.wit")]
pub struct GuestWit<'a> {
    pub namespace: &'a str,
    pub name: &'a str,
    pub wasvy_wit_version: String,
    pub params: HashSet<SystemParam>,
    pub systems: Vec<WasmSystem>,
}

impl<'a> GuestWit<'a> {
    pub fn new(source: &'a Source) -> anyhow::Result<Self> {
        let engine = Default::default();
        let mut linker = linker(&engine);
        let systems =
            get_systems(source.path(), &mut linker, Default::default()).context("get_systems")?;

        let params = systems
            .iter()
            .flat_map(|sys| sys.args.iter().map(|arg| arg.param.clone()))
            .collect();
        let name = source.name();
        let namespace = source.runtime().namespace();
        let wasvy_wit_version = source
            .runtime()
            .find_dependency("wasvy", "ecs")
            .expect("wasvy:ecs is a dependecy of the runtime")
            .version
            .to_string();

        Ok(Self {
            name,
            namespace,
            params,
            systems,
            wasvy_wit_version,
        })
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
) -> anyhow::Result<Vec<WasmSystem>> {
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
