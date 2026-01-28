use wasvy::engine::Linker;

wasvy::auto_host_components! {
    path = "tests/fixtures/auto_host",
    world = "game:components/host",
    module = auto_components_bindings,
}

#[test]
fn auto_host_components_registers_linker() {
    let engine = wasmtime::Engine::default();
    let mut linker: Linker = Linker::new(&engine);
    add_components_to_linker(&mut linker);
}
