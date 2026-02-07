use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_ecs::prelude::{AppFunctionRegistry, AppTypeRegistry, ReflectComponent};
use bevy_reflect::Reflect;

use wasvy::WasvyComponent;
use wasvy::authoring::register_all;
use wasvy::witgen::{self, WitGeneratorSettings};

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
struct Health {
    current: f32,
    max: f32,
}

#[wasvy::methods]
impl Health {
    fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    fn pct(&self) -> f32 {
        self.current / self.max
    }
}

mod alt {
    use super::*;

    #[derive(Component, Reflect, Default, WasvyComponent)]
    #[reflect(Component)]
    pub struct Health {
        pub current: f32,
        pub max: f32,
    }

    #[wasvy::methods]
    impl Health {
        fn heal(&mut self, amount: f32) {
            self.current = (self.current + amount).min(self.max);
        }
    }
}

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
struct Marker;

#[test]
fn generates_wit_resources() {
    let mut app = App::new();
    register_all(&mut app);

    let settings = WitGeneratorSettings::default();
    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry");
    let output = witgen::generate_wit(&settings, type_registry, function_registry);

    let wasvy_use = "use wasvy:ecs/app.{component}";

    assert!(output.contains("package game:components;"));
    assert!(output.contains("interface components"));
    assert!(output.contains(wasvy_use));
    assert!(output.contains("resource health"));
    assert!(output.contains("wasvy:type-path="));
    assert!(output.contains("constructor(component: component)"));
    assert!(output.contains("heal: func(amount: f32)"));
    assert!(output.contains("pct: func() -> f32"));
    assert!(output.contains("world host"));
}

#[test]
fn wit_handles_collisions_and_empty_methods() {
    let mut app = App::new();
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

    assert!(output.contains("resource health"));
    assert!(output.contains("resource health-1"));
    assert!(output.contains("resource marker"));
    assert!(output.contains("constructor(component: component)"));
}
