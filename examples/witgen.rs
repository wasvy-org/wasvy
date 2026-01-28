use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_ecs::prelude::ReflectComponent;
use bevy_reflect::Reflect;

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

fn main() {
    let mut app = App::new();
    app.add_plugins(WasvyComponentPlugin::<Health>::default());
    app.add_plugins(WasvyMethodsPlugin::<Health>::default());
    app.add_plugins(WitGeneratorPlugin::default());

    app.update();

    println!("Wrote WIT to target/wasvy/components.wit");
}
