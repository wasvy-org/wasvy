use anyhow::{Result, bail};
use bevy::{
    ecs::schedule::{IntoScheduleConfigs, Schedules},
    log::warn,
};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::{HostApp, Schedule},
    host::{System, WasmHost},
    mods::ModSystemSet,
    runner::State,
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
            mod_id,
            mod_name,
            asset_id,
            asset_version,
            accesses,
            ..
        } = self.access()
        else {
            bail!("App can only be modified in a setup function")
        };

        // Each access needs to have dedicated systems that run inside it
        for access in accesses {
            // Validate that the schedule requested by the mod is enabled
            let Some(schedule) = access
                .schedules(world)
                .evaluate(&schedule)
                .map(|schedule| schedule.schedule_label())
            else {
                warn!(
                    "Mod tried adding systems to schedule {:?}, but that system is not enabled",
                    schedule
                );
                continue;
            };

            for system in systems.iter() {
                let schedule_config = table
                    .get_mut(system)?
                    .schedule(world, mod_name, asset_id, asset_version, &access)?
                    .in_set(ModSystemSet::All)
                    .in_set(ModSystemSet::Mod(mod_id))
                    .in_set(ModSystemSet::Access(*access));

                world
                    .get_resource_mut::<Schedules>()
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
