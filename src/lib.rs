#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://github.com/wasvy-org/wasvy/raw/main/assets/logo.png",
    html_favicon_url = "https://github.com/wasvy-org/wasvy/raw/main/assets/logo.png"
)]

extern crate self as wasvy;

#[cfg(not(target_arch = "wasm32"))]
pub mod access;
#[cfg(not(target_arch = "wasm32"))]
pub mod asset;
pub mod authoring;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod cleanup;
#[cfg(not(target_arch = "wasm32"))]
pub mod component;
#[cfg(not(target_arch = "wasm32"))]
pub mod devtools;
#[cfg(not(target_arch = "wasm32"))]
pub mod engine;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod entity;
#[cfg(not(target_arch = "wasm32"))]
pub mod host;
#[cfg(not(target_arch = "wasm32"))]
pub mod methods;
#[cfg(not(target_arch = "wasm32"))]
pub mod mods;
pub mod module_guest;
pub mod module_plugin;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod module_reload;
#[cfg(not(target_arch = "wasm32"))]
pub mod modules;
#[cfg(not(target_arch = "wasm32"))]
pub mod plugin;
#[cfg(not(target_arch = "wasm32"))]
pub mod prelude;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod query;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod resource;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod runner;
#[cfg(not(target_arch = "wasm32"))]
pub mod sandbox;
#[cfg(not(target_arch = "wasm32"))]
pub mod schedule;
#[cfg(not(target_arch = "wasm32"))]
pub mod send_sync_ptr;
#[cfg(not(target_arch = "wasm32"))]
pub mod serialize;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod setup;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod system;
#[cfg(not(target_arch = "wasm32"))]
pub mod witgen;
#[cfg(not(target_arch = "wasm32"))]
pub mod workspace;

#[cfg(not(target_arch = "wasm32"))]
mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/wasvy-ecs.wit",
        world: "host",
        // Interactions with `ResourceTable` can possibly trap so enable the ability
        // to return traps from generated functions.
        imports: { default: trappable },
        with: {
            "wasvy:ecs/app.serialize": crate::host::WasmSerialize,
            "wasvy:ecs/app.app": crate::host::WasmApp,
            "wasvy:ecs/app.system": crate::host::WasmSystem,
            "wasvy:ecs/app.commands": crate::host::WasmCommands,
            "wasvy:ecs/app.entity": crate::host::WasmEntity,
            "wasvy:ecs/app.entity-commands": crate::host::WasmEntityCommands,
            "wasvy:ecs/app.query": crate::host::WasmQuery,
            "wasvy:ecs/app.query-result": crate::host::WasmQueryResult,
            "wasvy:ecs/app.component": crate::host::WasmComponent,
            "wasvy:ecs/app.world-resource": crate::host::WasmResource,
        },
    });
}

pub use wasvy_macros::{
    WasvyComponent, auto_host_components, component, guest_bindings, guest_type_paths,
    include_wasvy_components, methods, module, on_first_load, skip, system,
};
