use bevy_ecs::prelude::Component;
use bevy_reflect::Reflect;
use wasvy::WasvyComponent;

#[derive(Component, Reflect, Default, WasvyComponent)]
struct Health;

#[wasvy::methods]
impl Health {
    fn generic<T>(&self, _value: T) {}
}

fn main() {}
