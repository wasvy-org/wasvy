mod bindings {
    wasvy::guest_bindings!({
        path: ["tests/fixtures/guest_bindings"],
        world: "test:guest/example",
        with: {
            "game:components/components": generate,
        }
    });
}

use bindings::game::components::components::Health;

#[test]
fn guest_bindings_includes_type_path_helpers() {
    assert_eq!(
        Health::type_path_str(),
        "tests::fixtures::components::Health"
    );
    assert_eq!(Health::type_path(), Health::type_path_str().to_string());
}
