use super::*;
use anyhow::anyhow;
use bevy::prelude::Update;

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
            table, schedules, ..
        } = self.access()
        else {
            unreachable!()
        };

        for system in systems.iter() {
            let system = table.get_mut(system)?;
            let boxed_system = system
                .0
                .take()
                .ok_or(anyhow!("System was already added to the app"))?;

            let schedule = match schedule {
                Schedule::Update => Update,
            };

            schedules.add_systems(schedule, boxed_system);
        }

        Ok(())
    }

    fn drop(&mut self, _rep: Resource<App>) -> Result<()> {
        Ok(())
    }
}
