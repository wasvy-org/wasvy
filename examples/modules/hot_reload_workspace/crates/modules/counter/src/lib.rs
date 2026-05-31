use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use game_api::{Runner, StatusBoard};
use serde::{Deserialize, Serialize};
use wasvy::module_guest::Update;

wasvy::module! {
    name: "counter"
}

#[derive(Resource, Reflect, Debug, Default, Clone, Serialize, Deserialize)]
#[reflect(Resource)]
struct CounterState {
    ticks: u32,
    total_energy_added: i32,
}

// Change this while `wasvy dev` is running to watch the same world keep its state.
const STEP: i32 = 1;

#[wasvy::on_first_load]
fn init(mut commands: Commands) {
    commands.insert_resource(CounterState::default());
}

#[wasvy::system(Update)]
fn tick(
    mut board: ResMut<StatusBoard>,
    mut state: ResMut<CounterState>,
    mut runners: Query<&mut Runner>,
) {
    // println!("Tick");
    state.ticks += 1;
    state.total_energy_added += STEP;

    board.frames += 1;
    board.module_ticks = state.ticks;
    board.total_energy_added = state.total_energy_added;
    board.last_step = STEP;

    for mut runner in &mut runners {
        runner.energy += STEP;
    }
}
