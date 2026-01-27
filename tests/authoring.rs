use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_ecs::prelude::{AppTypeRegistry, ReflectComponent};
use bevy_reflect::{Reflect, TypePath};

use wasvy::methods::MethodTarget;
use wasvy::prelude::*;

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
#[wasvy::component]
struct Health {
    current: f32,
    max: f32,
}

#[wasvy::methods]
impl Health {
    #[wasvy::method]
    fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    #[wasvy::method]
    fn pct(&self) -> f32 {
        self.current / self.max
    }
}

#[test]
fn methods_macro_registers() {
    let mut registry = MethodRegistry::default();
    Health::register_methods(&mut registry);

    let mut health = Health {
        current: 2.0,
        max: 10.0,
    };

    let out = registry
        .invoke(
            Health::type_path(),
            "heal",
            MethodTarget::Write(&mut health),
            "[5.0]",
        )
        .unwrap();

    assert_eq!(out, "null");
    assert_eq!(health.current, 7.0);

    let pct = registry
        .invoke(
            Health::type_path(),
            "pct",
            MethodTarget::Read(&health),
            "null",
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
}
