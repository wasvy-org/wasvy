use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{intern::Interned, prelude::*, schedule::ScheduleLabel};
use bevy_platform::collections::HashSet;

/// This is an enum representing schedules in Bevy where mods can also be run.
///
/// See the docs for [bevy schedules](bevy_app::Main).
///
/// Call [ModLoaderPlugin::enable_schedule](crate::plugin::ModLoaderPlugin::enable_schedule)
/// to enable new or custom schedules for mods.
///
/// None of the startup schedules (like [PreStartup],
/// [Startup], etc) are included since mods can't usually run
/// within them, since mods take time to load and begin loading these schedules
/// have finished running.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModSchedule {
    /// A custom schedule that runs the first time a mod is loaded.
    ///
    /// It is a custom schedule that runs during the setup schedule
    /// (which defaults to [First]), see
    /// [ModLoaderPlugin::set_setup_schedule](crate::plugin::ModLoaderPlugin::set_setup_schedule)).
    ///
    /// Upon being loaded, mods are guaranteed to only run this schedule once,
    /// even if other mods are loaded afterwards.
    ModStartup,

    /// See the Bevy schedule [PreUpdate]
    PreUpdate,

    /// See the Bevy schedule [Update]
    Update,

    /// See the Bevy schedule [PostUpdate]
    PostUpdate,

    /// See the Bevy schedule [FixedPreUpdate]
    FixedPreUpdate,

    /// See the Bevy schedule [FixedUpdate]
    FixedUpdate,

    /// See the Bevy schedule [FixedPostUpdate]
    FixedPostUpdate,

    /// A custom schedule. See [ModSchedule::new_custom] for more details.
    Custom {
        name: String,
        schedule: Interned<dyn ScheduleLabel>,
    },
}

impl ModSchedule {
    /// A custom schedule for the Modloader
    ///
    /// - `name` must match what the mod registers with via the wit api
    /// - `schedule` is the Bevy schedule this represents. This schedule must be added to the Bevy Schedules.
    ///
    /// Note: Trying to add mod systems to the setup schedule (which defaults to [First], see
    /// [ModLoaderPlugin::set_setup_schedule](crate::plugin::ModLoaderPlugin::set_setup_schedule))
    /// Bevy's First schedule will do nothing since this is the mod setup phase
    pub fn new_custom(name: impl ToString, schedule: impl ScheduleLabel) -> Self {
        let name = name.to_string();
        let schedule = schedule.intern();
        Self::Custom { name, schedule }
    }

    /// Returns a bevy [ScheduleLabel]. This can be passed into any methods that accept an `impl ScheduleLabel`.
    pub fn schedule_label(&self) -> Interned<dyn ScheduleLabel> {
        match self {
            Self::ModStartup => ModStartup.intern(),
            Self::PreUpdate => PreUpdate.intern(),
            Self::Update => Update.intern(),
            Self::PostUpdate => PostUpdate.intern(),
            Self::FixedPreUpdate => FixedPreUpdate.intern(),
            Self::FixedUpdate => FixedUpdate.intern(),
            Self::FixedPostUpdate => FixedPostUpdate.intern(),
            Self::Custom { schedule, .. } => *schedule,
        }
    }
}

/// The hidden custom schedule that runs when one or more new mods were loaded
///
/// This isn't added to the scheduler, instead it's run by the exclusive system ([run_setup](crate::setup::run_setup)) after one or more mods finish loading
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) struct ModStartup;

impl ModStartup {
    pub(crate) fn new_schedule() -> Schedule {
        Schedule::new(Self)
    }

    pub(crate) fn run(world: &mut World) {
        let mut schedules = world
            .get_resource_mut::<Schedules>()
            .expect("running in an App");

        // Swap the schedule with a new one
        // This ensures that next time a mod adds a system to this schedule and we run it we won't also re-run old systems
        let mut schedule = schedules
            .insert(Self::new_schedule())
            .expect("ModStartup schedule be added the App by ModLoaderPlugin");

        // Run the schedule and drop it
        schedule.run(world);
    }
}

/// A collection of the [ModSchedules] where Wasvy will run mod systems.
///
/// Adjust this via [ModLoaderPlugin::new](crate::plugin::ModLoaderPlugin::new). This will only affect
/// mods with access to the world.
///
/// Or more simply, call [ModLoaderPlugin::enable_schedule](crate::plugin::ModLoaderPlugin::enable_schedule) with
/// [ModLoaderPlugin::default](crate::plugin::ModLoaderPlugin::default).
///
/// When using a [Sandbox](crate::sandbox::Sandbox), this is provided as an argument to adjust schedules for
/// mod systems that run in that sandbox.
#[derive(Resource, Debug, Clone, Deref, DerefMut)]
pub struct ModSchedules(pub HashSet<ModSchedule>);

impl Default for ModSchedules {
    fn default() -> Self {
        let mut set = HashSet::with_capacity(8);
        set.insert(ModSchedule::ModStartup);
        set.insert(ModSchedule::ModStartup);
        set.insert(ModSchedule::PreUpdate);
        set.insert(ModSchedule::Update);
        set.insert(ModSchedule::PostUpdate);
        set.insert(ModSchedule::FixedPreUpdate);
        set.insert(ModSchedule::FixedUpdate);
        set.insert(ModSchedule::FixedPostUpdate);
        Self(set)
    }
}

impl ModSchedules {
    /// Returns an empty Schedules.
    pub fn empty() -> Self {
        Self(HashSet::new())
    }
}
