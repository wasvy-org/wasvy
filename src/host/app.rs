use super::*;
use bevy::{ecs::schedule::Schedules, prelude::Update};

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

        for system in systems.iter() {
            let system = table.get_mut(system)?;
            let boxed_system = system.build(world, mod_name, asset_id, asset_version)?;

            let schedule = match schedule {
                Schedule::Update => Update,
            };

            let mut schedules = world
                .get_resource_mut::<Schedules>()
                .expect("running in an App");
            schedules.add_systems(schedule, boxed_system);
        }

        Ok(())
    }

    fn drop(&mut self, _rep: Resource<App>) -> Result<()> {
        Ok(())
    }
}
