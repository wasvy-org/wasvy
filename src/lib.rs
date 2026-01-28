#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://github.com/wasvy-org/wasvy/raw/main/assets/logo.png",
    html_favicon_url = "https://github.com/wasvy-org/wasvy/raw/main/assets/logo.png"
)]

pub mod access;
pub mod authoring;
pub mod asset;
pub(crate) mod cleanup;
pub mod component;
pub mod engine;
pub(crate) mod entity;
pub mod host;
pub mod methods;
pub mod mods;
pub mod plugin;
pub mod prelude;
pub(crate) mod query;
pub(crate) mod runner;
pub mod sandbox;
pub mod schedule;
pub mod send_sync_ptr;
pub(crate) mod setup;
pub(crate) mod system;
pub mod witgen;

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/ecs/ecs.wit",
        world: "host",
        // Interactions with `ResourceTable` can possibly trap so enable the ability
        // to return traps from generated functions.
        imports: { default: trappable },
        with: {
            "wasvy:ecs/app.app": crate::host::WasmApp,
            "wasvy:ecs/app.system": crate::host::WasmSystem,
            "wasvy:ecs/app.commands": crate::host::WasmCommands,
            "wasvy:ecs/app.entity": crate::host::WasmEntity,
            "wasvy:ecs/app.entity-commands": crate::host::WasmEntityCommands,
            "wasvy:ecs/app.query": crate::host::WasmQuery,
            "wasvy:ecs/app.query-result": crate::host::WasmQueryResult,
            "wasvy:ecs/app.component": crate::host::WasmComponent,
        },
    });
}

pub use wasvy_macros::{
    auto_host_components, component, guest_bindings, guest_type_paths, include_wasvy_components,
    method, methods,
};
