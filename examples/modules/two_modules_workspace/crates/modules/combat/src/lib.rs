use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use game_api::{Actor, SharedTimeline, SimulationGate};
use serde::{Deserialize, Serialize};
use wasvy::module_guest::Update;

wasvy::module! {
    name: "combat"
}

#[derive(Resource, Reflect, Debug, Default, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct CombatState {
    pub initialized: bool,
    pub swings: u32,
}

#[wasvy::on_first_load]
fn init(mut commands: Commands) {
    commands.insert_resource(CombatState {
        initialized: true,
        swings: 0,
    });
}

#[wasvy::system(Update)]
fn tick(
    gate: Res<SimulationGate>,
    mut timeline: ResMut<SharedTimeline>,
    mut combat: ResMut<CombatState>,
    mut actors: Query<&mut Actor>,
) {
    if !gate.running {
        return;
    }

    timeline.frame += 1;
    timeline.combat_ticks += 1;
    combat.swings += 1;

    for mut actor in &mut actors {
        actor.health -= 1;
    }
}
