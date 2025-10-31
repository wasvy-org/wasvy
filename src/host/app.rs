use anyhow::{Result, bail};
use bevy::ecs::schedule::Schedules as BevySchedules;
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostApp, Schedule},
    host::{System, WasmHost},
    runner::State,
    schedule::Schedules,
};

pub struct App;

impl HostApp for WasmHost {
    fn new(&mut self) -> Result<Resource<App>> {
        let State::Setup {
            table, app_init, ..
        } = self.access()
        else {
            bail!("App can only be instantiated in a setup function")
        };

        if *app_init {
            bail!("App can only be instantiated once")
        }

        let app_res = table.push(App)?;
        *app_init = true;

        Ok(app_res)
    }

    fn add_systems(
        &mut self,
        _self: Resource<App>,
        schedule: Schedule,
        systems: Vec<Resource<System>>,
    ) -> Result<()> {
        let State::Setup {
            table,
            world,
            mod_name,
            asset_id,
            asset_version,
            ..
        } = self.access()
        else {
            unreachable!()
        };

        // Validate that the schedule requested by the mod is enabled
        let schedules = world.get_resource_or_init::<Schedules>();
        let Some(schedule) = schedules.evaluate(schedule) else {
            // Don't do anything if the schedule is disabled
            return Ok(());
        };

        for system in systems.iter() {
            let system = table.get_mut(system)?;
            let schedule_config = system.schedule(world, mod_name, asset_id, asset_version)?;

            let schedule = schedule.schedule_label();

            let mut schedules = world
                .get_resource_mut::<BevySchedules>()
                .expect("running in an App");
            schedules.add_systems(schedule, schedule_config);
        }

        Ok(())
    }

    fn drop(&mut self, app: Resource<App>) -> Result<()> {
        let _ = self.table().delete(app)?;

        Ok(())
    }
}
