use bevy_app::Update;
use bevy_ecs::prelude::*;

wasvy::module! {
    name: "bad-local"
}

#[wasvy::system(Update)]
fn bad_system(_local: Local<u32>) {}

fn main() {}
