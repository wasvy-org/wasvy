use super::*;

pub struct Commands;

impl HostCommands for WasmHost {
    fn spawn(
        &mut self,
        _self: Resource<Commands>,
        _components: Vec<Resource<Component>>,
    ) -> Result<()> {
        bail!("Unimplemented")
    }

    fn drop(&mut self, _rep: Resource<Commands>) -> Result<()> {
        Ok(())
    }
}
