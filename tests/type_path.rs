mod fixtures {
    wasvy::include_wasvy_components!("tests/fixtures");
}

use bevy_app::App;
use bevy_ecs::prelude::AppTypeRegistry;
use bevy_ecs::reflect::AppFunctionRegistry;
use wasvy::authoring::register_all;
use wasvy::witgen::{self, WitGeneratorSettings};

#[test]
fn include_wasvy_components_preserves_type_path() {
    let mut app = App::new();
    app.init_resource::<AppFunctionRegistry>();
    register_all(&mut app);

    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry");
    let settings = WitGeneratorSettings::default();
    let output = witgen::generate_wit(&settings, type_registry, function_registry);
    let expected = format!(
        "wasvy:type-path={}::fixtures::components::Health",
        module_path!()
    );
    assert!(
        output.contains(&expected),
        "missing type path: {expected}\n{output}"
    );
}
