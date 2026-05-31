//! Reload state and transactional swap scaffolding for **Wasvy Modules**.

use anyhow::{Context, Result};
use bevy_asset::{AssetEvent, AssetId, Assets};
use bevy_ecs::{
    prelude::*,
    schedule::{ScheduleCleanupPolicy, ScheduleError},
    system::{SystemParam, SystemState},
};
use bevy_log::prelude::*;
use bevy_platform::collections::{HashMap, HashSet};

use crate::{
    access::ModAccess,
    asset::{ModAsset, PlannedModuleSystems},
    modules::{
        Module, ModuleCompatibilityFailure, ModuleGeneration, ModuleId, ModuleSystemSet,
        ReloadBlockedReason,
    },
    schedule::{ModSchedules, ModStartup},
};

/// Monotonic counter used to allocate new Module generations.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct ModuleGenerationCounter(pub u64);

impl ModuleGenerationCounter {
    pub fn next_generation(&mut self) -> ModuleGeneration {
        self.0 += 1;
        ModuleGeneration(self.0)
    }
}

/// Pending reload request for one Module artifact activation.
#[derive(Debug, Clone, Copy)]
pub struct PendingModuleReload {
    pub module_entity: Entity,
    pub asset_id: AssetId<ModAsset>,
    pub requested_generation: ModuleGeneration,
}

/// Queue of pending module reload transactions.
#[derive(Resource, Default, Debug, Clone)]
pub struct ModuleReloadQueue(pub Vec<PendingModuleReload>);

/// Message used to remove Module system sets from Bevy schedules.
#[derive(Message)]
pub(crate) struct DisableModuleSystemSet {
    pub(crate) set: ModuleSystemSet,
    pub(crate) schedules: ModSchedules,
}

impl Command<()> for DisableModuleSystemSet {
    fn apply(self, world: &mut World) {
        if !self.schedules.0.is_empty() {
            world.write_message(self);
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct ModuleSetup<'w, 's> {
    events: MessageReader<'w, 's, AssetEvent<ModAsset>>,
    assets: Res<'w, Assets<ModAsset>>,
    modules: Query<'w, 's, (Entity, Ref<'static, Module>, Option<&'static Name>)>,
}

struct PreparedModuleReload {
    module_entity: Entity,
    asset_id: AssetId<ModAsset>,
    module_id: ModuleId,
    old_generation: Option<ModuleGeneration>,
    new_generation: ModuleGeneration,
    accesses: Vec<ModAccess>,
    planned: PlannedModuleSystems,
}

/// Asset-event-driven module reload entry point.
///
/// Phase 1 behavior:
/// - if a module asset loads (or the module component changes while the asset is present), queue a new generation
/// - process queued reloads transactionally
/// - preserve world state on success
/// - keep the old generation active on failure
pub(crate) fn run_module_reload(world: &mut World, param: &mut SystemState<ModuleSetup>) {
    let ModuleSetup {
        mut events,
        assets,
        modules,
    } = param.get_mut(world);

    let mut loaded_assets = Vec::new();
    for event in events.read() {
        if let AssetEvent::LoadedWithDependencies { id } = event {
            loaded_assets.push(*id);
        }
    }

    let mut to_queue = Vec::new();
    for (entity, module, name) in modules.iter().filter(|(_, module, _)| {
        module.is_changed() || loaded_assets.contains(&module.asset().id())
    }) {
        let asset_id = module.asset().id();
        if assets.get(asset_id).is_none() {
            continue;
        }

        // Don't enqueue duplicate pending work for the same module while a generation is already pending.
        if module.pending_generation().is_some() {
            continue;
        }

        let display = name
            .map(Name::as_str)
            .unwrap_or_else(|| module.id().as_str());
        to_queue.push((entity, asset_id, display.to_string()));
    }

    let mut pending_reloads = Vec::with_capacity(to_queue.len());
    for (entity, asset_id, module_name) in to_queue {
        let generation = {
            let mut counter = world
                .get_resource_mut::<ModuleGenerationCounter>()
                .expect("ModuleGenerationCounter initialized by WasvyWorkspacePlugin");
            counter.next_generation()
        };

        if let Some(mut module) = world.get_mut::<Module>(entity) {
            module.set_pending_generation(generation);
        }
        trace!(
            "Queued module reload for {module_name} generation {}",
            generation.0
        );
        pending_reloads.push(PendingModuleReload {
            module_entity: entity,
            asset_id,
            requested_generation: generation,
        });
    }

    world
        .get_resource_mut::<ModuleReloadQueue>()
        .expect("ModuleReloadQueue initialized by WasvyWorkspacePlugin")
        .0
        .extend(pending_reloads);

    let ran_startup = process_module_reload_queue(world);
    if ran_startup {
        ModStartup::run(world);
    }
}

pub(crate) fn process_module_reload_queue(world: &mut World) -> bool {
    let mut queued = {
        let mut queue = world
            .get_resource_mut::<ModuleReloadQueue>()
            .expect("ModuleReloadQueue initialized by WasvyWorkspacePlugin");
        std::mem::take(&mut queue.0)
    };

    let mut ran_startup = false;
    for pending in queued.drain(..) {
        match prepare_module_reload(world, pending) {
            Ok(prepared) => {
                if let Err(err) = commit_module_reload(world, prepared) {
                    error!("Module reload activation failed: {err:?}");
                } else {
                    ran_startup = true;
                }
            }
            Err(err) => {
                error!("Module reload blocked: {err:?}");
            }
        }
    }

    ran_startup
}

fn prepare_module_reload(
    world: &mut World,
    pending: PendingModuleReload,
) -> Result<PreparedModuleReload> {
    let (module_id, old_generation, accesses) = {
        let module = world
            .get::<Module>(pending.module_entity)
            .context("missing Module during reload transaction")?;
        (
            module.id().clone(),
            module.active_generation(),
            module.accesses().copied().collect::<Vec<_>>(),
        )
    };

    let planned = match ModAsset::plan_systems(world, &pending.asset_id) {
        Ok(planned) => planned,
        Err(err) => {
            if let Some(mut module) = world.get_mut::<Module>(pending.module_entity) {
                module.block_reload(ReloadBlockedReason::RegistrationFailed);
            }
            return Err(err).context(format!(
                "module {} registration planning failed; previous generation remains active",
                module_id
            ));
        }
    };

    Ok(PreparedModuleReload {
        module_entity: pending.module_entity,
        asset_id: pending.asset_id,
        module_id,
        old_generation,
        new_generation: pending.requested_generation,
        accesses,
        planned,
    })
}

fn commit_module_reload(world: &mut World, prepared: PreparedModuleReload) -> Result<()> {
    if let Some(module) = world.get::<Module>(prepared.module_entity)
        && let Some(active_schema) = module.active_schema()
    {
        let issues = active_schema.diff(&prepared.planned.schema_snapshot);
        if !issues.is_empty() {
            if let Some(mut module) = world.get_mut::<Module>(prepared.module_entity) {
                module.block_reload(ReloadBlockedReason::CompatibilityFailed);
            }
            return Err(ModuleCompatibilityFailure {
                module_id: prepared.module_id.clone(),
                issues,
            }
            .into());
        }
    }

    if prepared.old_generation.is_none() {
        let access = prepared
            .accesses
            .first()
            .copied()
            .unwrap_or(ModAccess::World);
        ModAsset::run_first_load(world, &prepared.asset_id, access).with_context(|| {
            format!(
                "module {} first-load initialization failed; previous generation remains active",
                prepared.module_id
            )
        })?;
    }

    if let Some(old_generation) = prepared.old_generation {
        let schedules = schedules_for_accesses(&prepared.accesses, world);
        world.commands().queue(DisableModuleSystemSet {
            set: ModuleSystemSet::Generation {
                id: prepared.module_id.clone(),
                generation: old_generation,
            },
            schedules,
        });
    }

    prepared.planned.systems.add_module_systems(
        world,
        &prepared.accesses,
        &prepared.module_id,
        prepared.module_entity,
        prepared.new_generation,
        &prepared.asset_id,
        &prepared.planned.asset_version,
    )?;

    let Some(mut module) = world.get_mut::<Module>(prepared.module_entity) else {
        return Ok(());
    };
    module.activate_generation(
        prepared.new_generation,
        prepared.planned.schema_snapshot.clone(),
    );

    Ok(())
}

pub(crate) fn disable_module_system_sets(
    world: &mut World,
    param: &mut SystemState<MessageReader<DisableModuleSystemSet>>,
) {
    let mut messages = param.get_mut(world);

    let mut remove = HashMap::new();
    for DisableModuleSystemSet { set, schedules } in messages.read() {
        for schedule in schedules.0.iter() {
            remove
                .entry(schedule.schedule_label())
                .or_insert(HashSet::new())
                .insert(set.clone());
        }
    }

    for (label, sets) in remove {
        let mut schedules = world
            .get_resource_mut::<Schedules>()
            .expect("Running in a bevy App");
        let Some(mut schedule) = schedules.remove(label) else {
            continue;
        };

        for set in sets {
            if let Err(error) = schedule.remove_systems_in_set(
                set.clone(),
                world,
                ScheduleCleanupPolicy::RemoveSetAndSystems,
            ) && !matches!(error, ScheduleError::SetNotFound)
            {
                warn!(
                    "Unable to remove module system set {set:?}. Systems from old module generations might still be running!\nError: {error}."
                );
            }
        }

        world
            .get_resource_mut::<Schedules>()
            .expect("Running in a bevy App")
            .insert(schedule);
    }
}

fn schedules_for_accesses(accesses: &[ModAccess], world: &World) -> ModSchedules {
    let mut out = Vec::new();
    for access in accesses {
        for schedule in access.schedules(world).0 {
            if !out.contains(&schedule) {
                out.push(schedule);
            }
        }
    }

    ModSchedules(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{ModuleReloadStatus, ModuleSchemaSnapshot, ModuleTypeSchema};
    use bevy_asset::{Assets, Handle};

    #[test]
    fn generation_counter_increments() {
        let mut counter = ModuleGenerationCounter::default();
        let a = counter.next_generation();
        let b = counter.next_generation();

        assert_eq!(a, ModuleGeneration(1));
        assert_eq!(b, ModuleGeneration(2));
        assert_ne!(a, b);
    }

    #[test]
    fn registration_failure_keeps_old_generation_active() {
        let mut world = World::new();
        world.init_resource::<ModuleReloadQueue>();
        world.init_resource::<Assets<ModAsset>>();
        world.insert_resource(ModSchedules::default());

        let module_entity = world
            .spawn(Module::new(
                ModuleId::new("combat"),
                Handle::<ModAsset>::default(),
            ))
            .id();
        world
            .get_mut::<Module>(module_entity)
            .expect("module exists")
            .activate_generation(ModuleGeneration(1), ModuleSchemaSnapshot::default());

        world
            .resource_mut::<ModuleReloadQueue>()
            .0
            .push(PendingModuleReload {
                module_entity,
                asset_id: Handle::<ModAsset>::default().id(),
                requested_generation: ModuleGeneration(2),
            });

        let ran_startup = process_module_reload_queue(&mut world);
        assert!(!ran_startup);

        let module = world
            .get::<Module>(module_entity)
            .expect("module still exists");
        assert_eq!(module.active_generation(), Some(ModuleGeneration(1)));
        assert_eq!(
            module.reload_status(),
            &ModuleReloadStatus::Blocked(ReloadBlockedReason::RegistrationFailed)
        );
    }

    #[test]
    fn successful_commit_preserves_existing_world_state() {
        let mut world = World::new();
        world.insert_resource(ModSchedules::default());
        world.init_resource::<Schedules>();

        let module_entity = world
            .spawn(Module::new(
                ModuleId::new("combat"),
                Handle::<ModAsset>::default(),
            ))
            .id();
        world
            .get_mut::<Module>(module_entity)
            .expect("module exists")
            .activate_generation(ModuleGeneration(1), ModuleSchemaSnapshot::default());

        let preserved_entity = world.spawn_empty().id();

        let prepared = PreparedModuleReload {
            module_entity,
            asset_id: Handle::<ModAsset>::default().id(),
            module_id: ModuleId::new("combat"),
            old_generation: Some(ModuleGeneration(1)),
            new_generation: ModuleGeneration(2),
            accesses: vec![ModAccess::World],
            planned: PlannedModuleSystems {
                asset_version: world.change_tick(),
                systems: crate::system::PlannedSystems::default(),
                schema_snapshot: ModuleSchemaSnapshot::default(),
            },
        };

        commit_module_reload(&mut world, prepared).expect("commit succeeds");

        let module = world
            .get::<Module>(module_entity)
            .expect("module still exists");
        assert_eq!(module.active_generation(), Some(ModuleGeneration(2)));
        assert_eq!(module.reload_status(), &ModuleReloadStatus::Active);
        assert!(world.get_entity(preserved_entity).is_ok());
    }

    #[test]
    fn compatibility_failure_keeps_old_generation_active() {
        let mut world = World::new();
        world.insert_resource(ModSchedules::default());
        world.init_resource::<Schedules>();

        let module_entity = world
            .spawn(Module::new(
                ModuleId::new("combat"),
                Handle::<ModAsset>::default(),
            ))
            .id();
        world
            .get_mut::<Module>(module_entity)
            .expect("module exists")
            .activate_generation(
                ModuleGeneration(1),
                ModuleSchemaSnapshot::from_type_schemas(vec![ModuleTypeSchema {
                    type_path: "combat::State".to_string(),
                    fields: Some(vec!["value".to_string()]),
                }]),
            );

        let prepared = PreparedModuleReload {
            module_entity,
            asset_id: Handle::<ModAsset>::default().id(),
            module_id: ModuleId::new("combat"),
            old_generation: Some(ModuleGeneration(1)),
            new_generation: ModuleGeneration(2),
            accesses: vec![ModAccess::World],
            planned: PlannedModuleSystems {
                asset_version: world.change_tick(),
                systems: crate::system::PlannedSystems::default(),
                schema_snapshot: ModuleSchemaSnapshot::from_type_schemas(vec![ModuleTypeSchema {
                    type_path: "combat::State".to_string(),
                    fields: Some(vec!["value".to_string(), "source".to_string()]),
                }]),
            },
        };

        let err = commit_module_reload(&mut world, prepared).unwrap_err();
        assert!(err.to_string().contains("Relaunch required"));

        let module = world
            .get::<Module>(module_entity)
            .expect("module still exists");
        assert_eq!(module.active_generation(), Some(ModuleGeneration(1)));
        assert_eq!(
            module.reload_status(),
            &ModuleReloadStatus::Blocked(ReloadBlockedReason::CompatibilityFailed)
        );
    }
}
