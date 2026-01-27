use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
#[wasvy::component]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[wasvy::methods]
impl Health {
    #[wasvy::method]
    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    #[wasvy::method]
    pub fn pct(&self) -> f32 {
        self.current / self.max
    }
}
