#![doc = "WASM mod loader for wasvy_runtime, powered by Wasmtime and WASI."]

pub mod engine;
pub mod host;
pub mod plugin;
pub mod wasm_asset;

pub(crate) mod entity;
pub(crate) mod query;
pub(crate) mod runner;
pub(crate) mod system;

pub(crate) mod bindings {
    wasvy_macros::wasvy_bindgen! {
        world: "host",
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
        },
    }
}

pub use engine::Linker;
pub use plugin::WasmLoaderPlugin;
pub use wasm_asset::WasmAssetLoader;
