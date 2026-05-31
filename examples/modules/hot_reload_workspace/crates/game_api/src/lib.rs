use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};

#[derive(Component, Reflect, Debug, Clone, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Runner {
    pub energy: i32,
}

#[derive(Resource, Reflect, Debug, Default, Clone, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct StatusBoard {
    pub frames: u32,
    pub module_ticks: u32,
    pub total_energy_added: i32,
    pub last_step: i32,
}
