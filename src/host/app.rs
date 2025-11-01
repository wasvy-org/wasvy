use anyhow::{Result, bail};
use bevy::{
    ecs::schedule::{IntoScheduleConfigs, Schedules as BevySchedules},
    log::warn,
};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostApp, Schedule},
    host::{System, WasmHost},
    runner::State,
    sandbox::Sandbox,
    schedule::ModSchedules,
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
        if systems.is_empty() {
            return Ok(());
        }

        let State::Setup {
            table,
            world,
            mod_name,
            asset_id,
            asset_version,
            sandbox_entities,
            ..
        } = self.access()
        else {
            bail!("App can only be modified in a setup function")
        };

        // Validate that the schedule requested by the mod is enabled
        let schedules = world.get_resource_or_init::<ModSchedules>();
        let Some(schedule) = schedules.evaluate(&schedule) else {
            warn!(
                "Mod tried adding systems to schedule {:?}, but that system is not enabled",
                schedule
            );
            return Ok(());
        };

        // Each sandbox needs to have dedicated systems that run inside it
        for sandbox_entity in sandbox_entities {
            let sandbox = world
                .get::<Sandbox>(*sandbox_entity)
                .expect("has a sandbox");

            // Only add systems for the schedules that are enabled for this sandbox
            if sandbox.schedules().0.contains(&schedule) {
                continue;
            }

            let schedule = schedule.schedule_label();
            let access = sandbox.access();
            let system_set = sandbox.system_set();

            for system in systems.iter() {
                let schedule_config = table
                    .get_mut(system)?
                    .schedule(world, mod_name, asset_id, asset_version, &access)?
                    .in_set(system_set.clone());

                world
                    .get_resource_mut::<BevySchedules>()
                    .expect("running in an App")
                    .add_systems(schedule.clone(), schedule_config);
            }
        }

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, app: Resource<App>) -> Result<()> {
        let _ = self.table().delete(app)?;

        Ok(())
    }
}
