use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use game_api::{Actor, SharedTimeline, SimulationGate};
use serde::{Deserialize, Serialize};
use wasvy::module_guest::Update;

wasvy::module! {
    name: "ai"
}

#[derive(Resource, Reflect, Debug, Default, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct AiState {
    pub initialized: bool,
    pub decisions: u32,
}

#[wasvy::on_first_load]
fn init(mut commands: Commands) {
    commands.insert_resource(AiState {
        initialized: true,
        decisions: 0,
    });
}

#[wasvy::system(Update)]
fn think(
    gate: Res<SimulationGate>,
    mut timeline: ResMut<SharedTimeline>,
    mut ai: ResMut<AiState>,
    mut actors: Query<&mut Actor>,
) {
    println!("think");
    if !gate.running {
        return;
    }

    ai.decisions += 100;
    timeline.ai_ticks += 1;

    for mut actor in &mut actors {
        actor.intent_score = timeline.combat_ticks as i32 - actor.health;
    }
}
