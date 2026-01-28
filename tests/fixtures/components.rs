use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
#[wasvy::component]
pub struct Health {
    current: f32,
    max: f32,
}

#[wasvy::methods]
impl Health {
    #[wasvy::method]
    fn pct(&self) -> f32 {
        self.current / self.max
    }
}
