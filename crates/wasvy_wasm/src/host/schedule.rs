use wasvy_runtime::schedule::ModScheduleName;

use crate::bindings::wasvy::ecs::app::Schedule;

impl From<Schedule> for ModScheduleName {
    fn from(value: Schedule) -> Self {
        match value {
            Schedule::ModStartup => Self::ModStartup,
            Schedule::PreUpdate => Self::PreUpdate,
            Schedule::Update => Self::Update,
            Schedule::PostUpdate => Self::PostUpdate,
            Schedule::FixedPreUpdate => Self::FixedPreUpdate,
            Schedule::FixedUpdate => Self::FixedUpdate,
            Schedule::FixedPostUpdate => Self::FixedPostUpdate,
            Schedule::Custom(name) => Self::Custom(name),
        }
    }
}
