//! PROTOTYPE — real Wasmtime Component bridge for the typed executor.

use std::path::Path;

use bevy_ecs::prelude::*;
use wasmtime::{
    Engine, Store,
    component::{Component, HasSelf, InstancePre, Linker, Resource, Val},
};
use wasmtime_wasi::ResourceTable;

use super::{Actor, Counter};

mod bindings {
    wasmtime::component::bindgen!({
        path: "examples/prototypes/executor_seam/wit",
        world: "prototype",
        imports: { default: trappable },
        with: {
            "wasvy:executor-seam/bridge.counter": super::CounterHandle,
            "wasvy:executor-seam/bridge.actors": super::ActorsHandle,
            "wasvy:executor-seam/bridge.actor": super::ActorHandle,
        },
    });
}

use bindings::wasvy::executor_seam::bridge::{Host, HostActor, HostActors, HostCounter};

type ErasedActorQuery<'w, 's> = Query<'w, 's, (Entity, &'static mut Actor)>;
type HostResult<T> = wasmtime::Result<T>;

#[derive(Clone, Copy)]
struct InvocationPointers {
    counter: usize,
    actors: usize,
}

pub struct CounterHandle;

pub struct ActorsHandle {
    entities: Vec<Entity>,
    cursor: usize,
}

#[derive(Clone, Copy)]
pub struct ActorHandle {
    entity: Entity,
}

struct HostState {
    table: ResourceTable,
    invocation: Option<InvocationPointers>,
    host_calls: u64,
}

impl HostState {
    fn new<'w, 's, 'a>(
        counter: &mut Counter,
        actors: &mut Query<'w, 's, (Entity, &'a mut Actor)>,
    ) -> Self {
        Self {
            table: ResourceTable::new(),
            invocation: Some(InvocationPointers {
                counter: std::ptr::from_mut(counter) as usize,
                actors: std::ptr::from_mut(actors) as usize,
            }),
            host_calls: 0,
        }
    }

    fn called(&mut self) {
        self.host_calls += 1;
    }

    fn invocation(&self) -> HostResult<InvocationPointers> {
        self.invocation
            .ok_or_else(|| wasmtime::Error::msg("typed Bevy invocation has expired"))
    }

    fn counter(&self) -> HostResult<&Counter> {
        let ptr = self.invocation()?.counter as *const Counter;
        // SAFETY: the Store is used synchronously while the typed executor owns
        // the referenced Bevy parameters, and is dropped before they are released.
        Ok(unsafe { &*ptr })
    }

    fn counter_mut(&mut self) -> HostResult<&mut Counter> {
        let ptr = self.invocation()?.counter as *mut Counter;
        // SAFETY: see `counter`; the typed executor holds exclusive ResMut access.
        Ok(unsafe { &mut *ptr })
    }

    fn actors(&mut self) -> HostResult<&mut ErasedActorQuery<'static, 'static>> {
        let ptr = self.invocation()?.actors as *mut ErasedActorQuery<'static, 'static>;
        // SAFETY: the synchronous component call cannot outlive the typed Query.
        Ok(unsafe { &mut *ptr })
    }
}

impl HostCounter for HostState {
    fn get(&mut self, resource: Resource<CounterHandle>) -> HostResult<u64> {
        self.called();
        let _ = self.table.get(&resource)?;
        Ok(self.counter()?.ticks)
    }

    fn set(&mut self, resource: Resource<CounterHandle>, value: u64) -> HostResult<()> {
        self.called();
        let _ = self.table.get(&resource)?;
        self.counter_mut()?.ticks = value;
        Ok(())
    }

    fn drop(&mut self, resource: Resource<CounterHandle>) -> HostResult<()> {
        let _ = self.table.delete(resource)?;
        Ok(())
    }
}

impl HostActors for HostState {
    fn next(
        &mut self,
        resource: Resource<ActorsHandle>,
    ) -> HostResult<Option<Resource<ActorHandle>>> {
        self.called();
        let entity = {
            let actors = self.table.get_mut(&resource)?;
            let entity = actors.entities.get(actors.cursor).copied();
            actors.cursor += usize::from(entity.is_some());
            entity
        };
        Ok(entity
            .map(|entity| self.table.push(ActorHandle { entity }))
            .transpose()?)
    }

    fn drop(&mut self, resource: Resource<ActorsHandle>) -> HostResult<()> {
        let _ = self.table.delete(resource)?;
        Ok(())
    }
}

impl HostActor for HostState {
    fn get(&mut self, resource: Resource<ActorHandle>) -> HostResult<i64> {
        self.called();
        let entity = self.table.get(&resource)?.entity;
        let (_, actor) = self.actors()?.get_mut(entity)?;
        Ok(actor.energy)
    }

    fn set(&mut self, resource: Resource<ActorHandle>, value: i64) -> HostResult<()> {
        self.called();
        let entity = self.table.get(&resource)?.entity;
        let (_, mut actor) = self.actors()?.get_mut(entity)?;
        actor.energy = value;
        Ok(())
    }

    fn drop(&mut self, resource: Resource<ActorHandle>) -> HostResult<()> {
        let _ = self.table.delete(resource)?;
        Ok(())
    }
}

impl Host for HostState {}

#[derive(Resource)]
pub struct WasmArtifact {
    engine: Engine,
    instance_pre: InstancePre<HostState>,
}

impl WasmArtifact {
    pub fn load(path: impl AsRef<Path>) -> HostResult<Self> {
        let engine = Engine::default();
        let component = Component::from_file(&engine, path.as_ref())?;
        let mut linker = Linker::new(&engine);
        type Data = HasSelf<HostState>;
        bindings::wasvy::executor_seam::bridge::add_to_linker::<_, Data>(&mut linker, |state| {
            state
        })?;
        let instance_pre = linker.instantiate_pre(&component)?;
        Ok(Self {
            engine,
            instance_pre,
        })
    }

    pub fn invoke<'w, 's, 'a>(
        &self,
        counter: &mut Counter,
        actors: &mut Query<'w, 's, (Entity, &'a mut Actor)>,
    ) -> HostResult<u64> {
        let entities = actors.iter_mut().map(|(entity, _)| entity).collect();
        let mut store = Store::new(&self.engine, HostState::new(counter, actors));

        let counter = store.data_mut().table.push(CounterHandle)?;
        let counter = counter.try_into_resource_any(&mut store)?;
        let actors = store.data_mut().table.push(ActorsHandle {
            entities,
            cursor: 0,
        })?;
        let actors = actors.try_into_resource_any(&mut store)?;

        let instance = self.instance_pre.instantiate(&mut store)?;
        let tick = instance
            .get_func(&mut store, "tick")
            .ok_or_else(|| wasmtime::Error::msg("prototype component is missing export `tick`"))?;
        tick.call(
            &mut store,
            &[Val::Resource(counter), Val::Resource(actors)],
            &mut [],
        )?;
        store.data_mut().invocation = None;
        Ok(store.data().host_calls)
    }
}
