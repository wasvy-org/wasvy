use bevy_derive::Deref;
use bevy_ecs::resource::Resource;

use crate::host::WasmHost;

/// Cross engine instatiation of WASM components is not supported.
/// This resources is the global [`Engine`](wasmtime::Engine) that is used for instatiation.
///
/// Check the wasmtime [`Engine`](wasmtime::Engine) docs for more information.
#[derive(Resource, Clone, Deref)]
pub(crate) struct Engine(wasmtime::Engine);

impl Engine {
    pub(crate) fn new() -> Self {
        let engine = wasmtime::Engine::default();
        Self(engine)
    }

    pub(crate) fn inner(&self) -> &wasmtime::Engine {
        &self.0
    }
}

pub type Linker = wasmtime::component::Linker<WasmHost>;

pub(crate) fn create_linker(engine: &Engine) -> Linker {
    let engine = engine.inner();

    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).expect("implement common wit interface");

    type Data = wasmtime::component::HasSelf<WasmHost>;
    crate::bindings::wasvy::ecs::app::add_to_linker::<_, Data>(&mut linker, |state| state)
        .expect("implement wasvy wit interface");

    linker
}
