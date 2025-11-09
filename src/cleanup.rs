use bevy::ecs::prelude::*;

use crate::{mods::ModSystemSet, prelude::ModSchedules};

/// A [Message] that triggers disabling of scheduled [ModSystemSets](ModSystemSet).
///
/// For ease of use within component hooks, this is also a command that can be enqueued like any other.
///
/// This is a message because it must run during the setup_schedule. This way
/// it can cleanup all schedules, including the one from which the message was written.
#[derive(Message)]
pub(crate) struct DisableSystemSet {
    pub(crate) set: ModSystemSet,
    pub(crate) schedules: ModSchedules,
}

impl Command<()> for DisableSystemSet {
    fn apply(self, world: &mut World) {
        if !self.schedules.0.is_empty() {
            world.write_message(self);
        }
    }
}

pub(crate) fn disable_system_sets(
    mut messages: MessageReader<DisableSystemSet>,
    mut schedules: ResMut<Schedules>,
) {
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
