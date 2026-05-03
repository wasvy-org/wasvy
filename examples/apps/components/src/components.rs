use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use wasvy::WasvyComponent;

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[wasvy::methods]
impl Health {
    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }

    pub fn pct(&self) -> f32 {
        self.current / self.max
    }
}
