use bevy::ecs::prelude::*;

use crate::prelude::{ModSchedules, Sandbox};

/// A [Message] that triggers removal of scheduled system sets.
///
/// For ease of use within component hooks, this is also a command that can be enqueued like any other.
///
/// This is a message because it must run during the setup_schedule. This way
/// it can cleanup all schedules, including the one from which the message was written.
#[derive(Message)]
pub(crate) struct RemoveSystemSet<T> {
    set: T,
    schedules: ModSchedules,
}

impl<T> RemoveSystemSet<T> {
    /// Triggers removal of scheduled system sets
    pub(crate) fn new(set: T, sandbox: &Sandbox) -> Self {
        Self {
            set,
            schedules: sandbox.schedules().clone(),
        }
    }
}

impl<T> Command<()> for RemoveSystemSet<T>
where
    T: Send + Sync + 'static,
{
    fn apply(self, world: &mut World) {
        world.write_message(self);
    }
}

pub(crate) fn remove_system_sets<T>(
    mut messages: MessageReader<RemoveSystemSet<T>>,
    mut schedules: ResMut<Schedules>,
) where
    T: SystemSet + Clone,
{
    for message in messages.read() {
        for schedule in message.schedules.0.iter() {
            // Quick and dirty way of ensuring systems sets no longer run
            // TODO: Next bevy release, remove systems from the schedule
            // See: https://github.com/bevyengine/bevy/pull/20298
            let sets = message.set.clone().run_if(|| false);
            schedules.configure_sets(schedule.schedule_label(), sets);
        }
    }
}
