use bevy_app::Update;
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Marker;

wasvy::module! {
    name: "bad-init"
}

#[wasvy::on_first_load]
fn init(_query: Query<&Marker>) {}

#[wasvy::system(Update)]
fn noop() {}

fn main() {}
