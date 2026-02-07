use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_ecs::prelude::ReflectComponent;
use bevy_reflect::Reflect;

use wasvy::WasvyComponent;
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
}

fn main() {
    let mut app = App::new();
    app.add_plugins(WasvyAutoRegistrationPlugin);
    app.add_plugins(WitGeneratorPlugin::default());

    app.update();

    println!("Wrote WIT to target/wasvy/components.wit");
}
