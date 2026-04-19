mod inner {
    wasmtime::component::bindgen!({
        path: "../../wit/wasvy-ecs.wit",
        world: "host",
        // Interactions with `ResourceTable` can possibly trap so enable the ability
        // to return traps from generated functions.
        imports: { default: trappable },
        with: {
            "wasvy:ecs/app.app": crate::witgen::WasmApp,
            "wasvy:ecs/app.system": crate::witgen::WasmSystem,
        },
    });
}

pub use inner::wasvy::ecs::app::*;
