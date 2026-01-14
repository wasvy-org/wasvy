use bevy_app::prelude::*;
use bevy_ecs::{intern::Interned, prelude::*, schedule::ScheduleLabel};

use crate::bindings::wasvy::ecs::app::Schedule as WitSchedule;

/// This is an enum representing schedules in Bevy where mods can also be run.
///
/// See the docs for [bevy schedules](bevy_app::Main).
///
/// Call [ModloaderPlugin::enable_schedule](crate::plugin::ModloaderPlugin::enable_schedule)
/// to enable new or custom schedules for mods.
///
/// None of the startup schedules (like [PreStartup](bevy_app::PreStartup),
/// [Startup](bevy_app::Startup), etc) are included since mods can't usually run
/// within them, since mods take time to load and begin loading these schedules
/// have finished running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModSchedule {
    /// A custom schedule that runs the first time a mod is loaded.
    ///
    /// It is a custom schedule that runs during the setup schedule
    /// (which defaults to [First](bevy_app::First)), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule)).
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
    /// Note: Trying to add mod systems to the setup schedule (which defaults to [First](bevy_app::First), see
    /// [ModloaderPlugin::set_setup_schedule](crate::plugin::ModloaderPlugin::set_setup_schedule))
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
            Self::Custom { schedule, .. } => schedule.clone(),
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
            .expect("ModStartup schedule be added the App by ModloaderPlugin");

        // Run the schedule and drop it
        schedule.run(world);
    }
}

/// A collection of the [Schedules] where Wasvy will run mod systems.
///
/// Adjust this via [ModloaderPlugin::new](crate::plugin::ModloaderPlugin::new). This will only affect
/// mods with access to the world.
///
/// Or more simply, call [ModloaderPlugin::enable_schedule](crate::plugin::ModloaderPlugin::enable_schedule) with
/// [ModloaderPlugin::default](crate::plugin::ModloaderPlugin::default).
///
/// When using a [Sandbox](crate::sandbox::Sandbox), this is provided as an argument to adjust schedules for
/// mod systems that run in that sandbox.
#[derive(Resource, Debug, Clone)]
pub struct ModSchedules(pub Vec<ModSchedule>);

impl Default for ModSchedules {
    fn default() -> Self {
        Self(vec![
            ModSchedule::ModStartup,
            ModSchedule::PreUpdate,
            ModSchedule::Update,
            ModSchedule::PostUpdate,
            ModSchedule::FixedPreUpdate,
            ModSchedule::FixedUpdate,
            ModSchedule::FixedPostUpdate,
        ])
    }
}

impl ModSchedules {
    /// Returns an empty Schedules.
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, schedule: ModSchedule) {
        assert!(
            !self.0.contains(&schedule),
            "Duplicate schedule {:?} added to ModloaderPlugin",
            &schedule
        );

        self.0.push(schedule);
    }

    /// If this schedule was enabled during plugin instantiation, this returns the correct schedule
    ///
    /// Returns None if the schedule was never added.
    pub(crate) fn evaluate(&self, schedule: &WitSchedule) -> Option<ModSchedule> {
        let schedule_or_custom_name = match schedule {
            WitSchedule::ModStartup => Either::Left(ModSchedule::ModStartup),
            WitSchedule::PreUpdate => Either::Left(ModSchedule::PreUpdate),
            WitSchedule::Update => Either::Left(ModSchedule::Update),
            WitSchedule::PostUpdate => Either::Left(ModSchedule::PostUpdate),
            WitSchedule::FixedPreUpdate => Either::Left(ModSchedule::FixedPreUpdate),
            WitSchedule::FixedUpdate => Either::Left(ModSchedule::FixedUpdate),
            WitSchedule::FixedPostUpdate => Either::Left(ModSchedule::FixedPostUpdate),
            WitSchedule::Custom(custom_name) => Either::Right(custom_name),
        };

        match schedule_or_custom_name {
            Either::Left(schedule) => {
                if self.0.contains(&schedule) {
                    Some(schedule)
                } else {
                    None
                }
            }
            Either::Right(custom_name) => self
                .0
                .iter()
                .find(|schedule| match schedule {
                    ModSchedule::Custom { name, .. } => name == custom_name,
                    _ => false,
                })
                .map(Clone::clone),
        }
    }
}

enum Either<L, R> {
    Left(L),
    Right(R),
}
