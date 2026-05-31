use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};

#[derive(Component, Reflect, Debug, Clone, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Actor {
    pub health: i32,
    pub intent_score: i32,
}

#[derive(Resource, Reflect, Debug, Default, Clone, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct SharedTimeline {
    pub frame: u32,
    pub combat_ticks: u32,
    pub ai_ticks: u32,
}

#[derive(Resource, Reflect, Debug, Default, Clone, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct SimulationGate {
    pub running: bool,
}
