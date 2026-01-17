use anyhow::Result;
use bevy_ecs::prelude::*;
use wasmtime::component::Resource;

use crate::{bindings::wasvy::ecs::app::HostEntity, host::WasmHost};

pub struct WasmEntity(pub(crate) Entity);

impl Into<Entity> for &WasmEntity {
    fn into(self) -> Entity {
        self.0
    }
}

impl From<Entity> for WasmEntity {
    fn from(value: Entity) -> Self {
        Self(value)
    }
}

impl HostEntity for WasmHost {
    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, commands: Resource<WasmEntity>) -> Result<()> {
        let _ = self.table().delete(commands)?;

        Ok(())
    }
}
