use std::any::type_name;

use anyhow::{Result, bail};
use bevy_ecs::prelude::*;
use bevy_log::prelude::*;
use wasmtime::component::Resource;
use wasmtime_wasi::ResourceTable;

use crate::{
    access::ModAccess,
    bindings::wasvy::ecs::app::{Bundle, BundleTypes},
    cleanup::DespawnModEntity,
    component::{insert_component, remove_component},
    host::WasmHost,
    runner::State,
};

pub(crate) trait ToEntity
where
    Self: Send + 'static,
{
    fn entity(&self) -> Entity;
}

pub(crate) trait FromEntity
where
    Self: Send + 'static,
{
    fn from_entity(entity: Entity) -> Self;
}

/// A helper to ingest one host resource and create another with the same entity
pub(crate) fn map_entity<I, O>(host: &mut WasmHost, input: Resource<I>) -> Result<Resource<O>>
where
    I: ToEntity,
    O: FromEntity,
{
    let State::RunSystem { table, .. } = host.access() else {
        bail!(
            "{} resource is only accessible when running systems",
            type_name::<I>()
        )
    };

    let input = table.get(&input)?;
    let entity = input.entity();
    entity_resource(entity, table)
}

pub(crate) fn spawn_empty<T>(host: &mut WasmHost) -> Result<Resource<T>>
where
    T: FromEntity,
{
    let State::RunSystem {
        commands,
        table,
        insert_despawn_component,
        access,
        ..
    } = host.access()
    else {
        bail!("Commands resource is only accessible when running systems",)
    };

    let mut entity_commands = commands.spawn_empty();

    // Make sure the entity is not spawned outside the sandbox
    // The mod can still override the ChildOf with its own value
    // Note: We can't currently prevent a mod from creating a component that has a relation to a component outside the sandbox
    // TODO: Restrict what entities a mod can reference via permissions
    if let ModAccess::Sandbox(entity) = access {
        entity_commands.insert(ChildOf(*entity));
    };

    // Make sure this entity is despawned when the mod is despawned. See [ModDespawnBehaviour]
    if let Some(mod_id) = insert_despawn_component.0 {
        entity_commands.insert(DespawnModEntity(mod_id));
    }

    let entity = entity_commands.id();
    trace!("Spawn empty ({entity})");

    entity_resource(entity, table)
}

pub(crate) fn insert<T>(host: &mut WasmHost, input: &Resource<T>, bundle: Bundle) -> Result<()>
where
    T: ToEntity,
{
    if bundle.is_empty() {
        return Ok(());
    }

    let State::RunSystem {
        commands,
        table,
        type_registry,
        ..
    } = host.access()
    else {
        bail!(
            "{} resource is only accessible when running systems",
            type_name::<T>()
        )
    };

    let input = table.get(input)?;
    let entity = input.entity();
    trace!("Insert components to ({entity})");
    for (type_path, serialized_component) in bundle {
        trace!("- {type_path}: {serialized_component}");
        insert_component(
            commands,
            type_registry,
            entity,
            type_path,
            serialized_component,
        )?;
    }

    Ok(())
}

pub(crate) fn remove<T>(host: &mut WasmHost, input: Resource<T>, bundle: BundleTypes) -> Result<()>
where
    T: ToEntity,
{
    if bundle.is_empty() {
        return Ok(());
    }

    let State::RunSystem {
        commands,
        table,
        wasm_registry,
        ..
    } = host.access()
    else {
        bail!(
            "{} resource is only accessible when running systems",
            type_name::<T>()
        )
    };

    let input = table.get(&input)?;
    let entity = input.entity();
    trace!("Remove components from ({entity})");
    for type_path in bundle {
        trace!("- {type_path}");
        remove_component(commands, wasm_registry, entity, type_path)?;
    }

    Ok(())
}

fn entity_resource<T>(entity: Entity, table: &mut ResourceTable) -> Result<Resource<T>>
where
    T: FromEntity,
{
    let output = T::from_entity(entity);
    let output = table.push(output)?;
    Ok(output)
}
