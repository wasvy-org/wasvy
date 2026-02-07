use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_ecs::prelude::{AppTypeRegistry, ReflectComponent};
use bevy_ecs::reflect::AppFunctionRegistry;
use bevy_reflect::{Reflect, TypePath};

use wasvy::WasvyComponent;
use wasvy::authoring::register_all;
use wasvy::methods::MethodTarget;
use wasvy::prelude::*;

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

    #[wasvy::skip]
    #[allow(dead_code)]
    fn internal_ratio(&self) -> f32 {
        self.current / self.max
    }
}

#[test]
fn methods_macro_registers() {
    let mut app = App::new();
    app.init_resource::<AppFunctionRegistry>();
    register_all(&mut app);

    let mut health = Health {
        current: 2.0,
        max: 10.0,
    };

    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry to exist");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry to exist");
    let index = FunctionIndex::build(type_registry, function_registry);
    let out = index
        .invoke(
            Health::type_path(),
            "heal",
            MethodTarget::Write(&mut health),
            "[5.0]",
            type_registry,
        )
        .unwrap();

    assert_eq!(out, "null");
    assert_eq!(health.current, 7.0);

    let pct = index
        .invoke(
            Health::type_path(),
            "pct",
            MethodTarget::Read(&health),
            "null",
            type_registry,
        )
        .unwrap();

    let pct_val: f32 = serde_json::from_str(&pct).unwrap();
    assert!((pct_val - 0.7).abs() < 1e-6);
}

#[test]
fn component_plugin_registers_type() {
    let mut app = App::new();
    app.add_plugins(WasvyComponentPlugin::<Health>::default());

    let registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry to exist");
    let registry = registry.read();

    assert!(registry.get_with_type_path(Health::type_path()).is_some());
    let registration = registry
        .get_with_type_path(Health::type_path())
        .expect("type registration");
    assert!(
        registration
            .data::<wasvy::authoring::WasvyExport>()
            .is_some()
    );
}

#[test]
fn auto_registration_plugin_registers_all() {
    let mut app = App::new();
    app.add_plugins(WasvyAutoRegistrationPlugin);

    {
        let registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry to exist");
        let registry = registry.read();
        assert!(registry.get_with_type_path(Health::type_path()).is_some());
    }

    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry to exist");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry to exist");
    let index = FunctionIndex::build(type_registry, function_registry);
    let mut health = Health {
        current: 2.0,
        max: 10.0,
    };
    let out = index
        .invoke(
            Health::type_path(),
            "heal",
            MethodTarget::Write(&mut health),
            "[1.0]",
            type_registry,
        )
        .unwrap();
    assert_eq!(out, "null");
    assert!((health.current - 3.0).abs() < f32::EPSILON);
}

#[test]
fn skip_attribute_excludes_method() {
    let mut app = App::new();
    register_all(&mut app);

    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry to exist");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry to exist");
    let index = FunctionIndex::build(type_registry, function_registry);

    let health = Health {
        current: 2.0,
        max: 10.0,
    };

    let err = index
        .invoke(
            Health::type_path(),
            "internal_ratio",
            MethodTarget::Read(&health),
            "null",
            type_registry,
        )
        .unwrap_err();

    assert!(
        err.to_string().contains("internal_ratio"),
        "unexpected error: {err}"
    );
}

#[test]
fn wit_uses_arg_names() {
    let mut app = App::new();
    register_all(&mut app);

    let type_registry = app
        .world()
        .get_resource::<AppTypeRegistry>()
        .expect("AppTypeRegistry to exist");
    let function_registry = app
        .world()
        .get_resource::<AppFunctionRegistry>()
        .expect("AppFunctionRegistry to exist");
    let settings = wasvy::witgen::WitGeneratorSettings::default();
    let output = wasvy::witgen::generate_wit(&settings, type_registry, function_registry);

    assert!(output.contains("heal: func(amount: f32)"), "{output}");
}
