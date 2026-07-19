use wasvy_runtime::prelude::ModSchedule;

use crate::bindings::wasvy::ecs::app::Schedule;

impl PartialEq<ModSchedule> for Schedule {
    fn eq(&self, other: &ModSchedule) -> bool {
        match self {
            Schedule::ModStartup => &ModSchedule::ModStartup == other,
            Schedule::PreUpdate => &ModSchedule::PreUpdate == other,
            Schedule::Update => &ModSchedule::Update == other,
            Schedule::PostUpdate => &ModSchedule::PostUpdate == other,
            Schedule::FixedPreUpdate => &ModSchedule::FixedPreUpdate == other,
            Schedule::FixedUpdate => &ModSchedule::FixedUpdate == other,
            Schedule::FixedPostUpdate => &ModSchedule::FixedPostUpdate == other,
            Schedule::Custom(custom_name) => {
                if let ModSchedule::Custom { name, .. } = other {
                    custom_name == name
                } else {
                    false
                }
            }
        }
    }
}
