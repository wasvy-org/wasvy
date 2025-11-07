pub mod asset;
pub mod cleanup;
pub mod component;
pub mod engine;
pub mod host;
pub mod mods;
pub mod plugin;
pub mod prelude;
pub mod runner;
pub mod sandbox;
pub mod schedule;
pub mod send_sync_ptr;
pub mod systems;

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/ecs/ecs.wit",
        world: "host",
        // Interactions with `ResourceTable` can possibly trap so enable the ability
        // to return traps from generated functions.
        imports: { default: trappable },
        with: {
            "wasvy:ecs/app/app": crate::host::App,
            "wasvy:ecs/app/system": crate::host::System,
            "wasvy:ecs/app/commands": crate::host::Commands,
            "wasvy:ecs/app/query": crate::host::Query,
            "wasvy:ecs/app/component": crate::host::Component,
        },
    });
}
