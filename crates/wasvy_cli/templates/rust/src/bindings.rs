// filename: src/bindings.rs
wit_bindgen::generate!({
    path: ["./wit"],
    world: "{{ world_name }}",
    with: {
        "wasvy:ecs/app@{{ wasvy_wit_version }}": generate,
    }
});

pub use wasvy::ecs::app::*;
