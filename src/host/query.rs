use anyhow::{Result, bail};
use wasmtime::component::Resource;

use crate::{
    bindings::wasvy::ecs::app::HostQuery,
    host::{WasmHost, WasmQueryResult},
    query::{QueryCursor, QueryId},
    runner::State,
};

pub struct WasmQuery {
    id: QueryId,
    cursor: QueryCursor,
}

impl WasmQuery {
    pub(crate) fn new(id: QueryId) -> Self {
        Self {
            id,
            cursor: QueryCursor::default(),
        }
    }
}

impl HostQuery for WasmHost {
    fn iter(&mut self, query: Resource<WasmQuery>) -> Result<Option<Resource<WasmQueryResult>>> {
        let State::RunSystem { table, queries, .. } = self.access() else {
            bail!("Query can only be accessed in systems")
        };

        let query = table.get_mut(&query)?;
        let cursor = query.cursor.increment();
        let Some(entity) = cursor.entity(queries, query.id) else {
            // We've reached the end of the results
            return Ok(None);
        };

        let result = WasmQueryResult::new(query.id, entity);
        let result = table.push(result)?;
        Ok(Some(result))
    }

    // Note: this is never guaranteed to be called by the wasi binary
    fn drop(&mut self, query: Resource<WasmQuery>) -> Result<()> {
        let _ = self.table().delete(query)?;

        Ok(())
    }
}
