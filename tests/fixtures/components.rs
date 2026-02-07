use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use wasvy::WasvyComponent;

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Health {
    current: f32,
    max: f32,
}

#[wasvy::methods]
impl Health {
    fn pct(&self) -> f32 {
        self.current / self.max
    }
}
