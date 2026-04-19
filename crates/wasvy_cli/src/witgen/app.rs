use super::*;

pub struct WasmApp;

impl bindings::HostApp for Host {
    fn add_systems(
        &mut self,
        _: Resource<WasmApp>,
        _: bindings::Schedule,
        systems: Vec<Resource<WasmSystem>>,
    ) -> Result<()> {
        for system in systems.iter() {
            let system = self.table.get(system).cloned()?;
            self.systems.push(system);
        }

        Ok(())
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, _: Resource<WasmApp>) -> Result<()> {
        Ok(())
    }
}
