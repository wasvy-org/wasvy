# Wasvy Modules: Phase 0 + 1 Rust API Skeletons

This document drafts the concrete Rust type and API skeletons for the new files introduced in `docs/plans/wasvy-modules-phase-0-1-checklist.md`.

These are **design skeletons**, not final implementations.
They are intentionally biased toward:

- minimal viable shape
- compatibility with the current codebase
- preserving the public Mod surface while adding the Wasvy Modules surface

---

## `src/modules.rs`

```rust
use std::fmt;

use bevy_asset::Handle;
use bevy_ecs::{prelude::*, system::SystemParam};
use bevy_platform::collections::HashSet;
use bevy_reflect::Reflect;

use crate::{
    access::ModAccess,
    asset::ModAsset,
    cleanup::DisableSystemSet, // only if temporarily reused
    module_reload::DisableModuleSystemSet,
    schedule::ModSchedules,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq, Reflect)]
pub struct ModuleId(String);

impl ModuleId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Reflect)]
pub struct ModuleGeneration(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub enum ReloadBlockedReason {
    RegistrationFailed,
    CompatibilityFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub enum ModuleReloadStatus {
    Active,
    Pending,
    Blocked(ReloadBlockedReason),
}

#[derive(Component, Reflect)]
#[component(on_despawn = Self::on_despawn)]
pub struct Module {
    id: ModuleId,
    asset: Handle<ModAsset>,
    active_generation: Option<ModuleGeneration>,
    pending_generation: Option<ModuleGeneration>,
    reload_status: ModuleReloadStatus,
    access: HashSet<ModAccess>,
}

impl Module {
    pub fn new(id: ModuleId, asset: Handle<ModAsset>) -> Self {
        Self {
            id,
            asset,
            active_generation: None,
            pending_generation: None,
            reload_status: ModuleReloadStatus::Pending,
            access: HashSet::default(),
        }
    }

    pub fn id(&self) -> &ModuleId {
        &self.id
    }

    pub fn asset(&self) -> Handle<ModAsset> {
        self.asset.clone()
    }

    pub fn active_generation(&self) -> Option<ModuleGeneration> {
        self.active_generation
    }

    pub fn pending_generation(&self) -> Option<ModuleGeneration> {
        self.pending_generation
    }

    pub fn reload_status(&self) -> &ModuleReloadStatus {
        &self.reload_status
    }

    pub fn set_pending_generation(&mut self, generation: ModuleGeneration) {
        self.pending_generation = Some(generation);
        self.reload_status = ModuleReloadStatus::Pending;
    }

    pub fn activate_generation(&mut self, generation: ModuleGeneration) {
        self.active_generation = Some(generation);
        self.pending_generation = None;
        self.reload_status = ModuleReloadStatus::Active;
    }

    pub fn block_reload(&mut self, reason: ReloadBlockedReason) {
        self.pending_generation = None;
        self.reload_status = ModuleReloadStatus::Blocked(reason);
    }

    pub fn enable_access(&mut self, access: ModAccess) -> bool {
        self.access.insert(access)
    }

    pub fn disable_access(&mut self, access: &ModAccess) -> bool {
        self.access.remove(access)
    }

    pub fn accesses(&self) -> impl Iterator<Item = &ModAccess> {
        self.access.iter()
    }

    fn on_despawn(mut world: bevy_ecs::world::DeferredWorld, ctx: bevy_ecs::lifecycle::HookContext) {
        let Some(module) = world.entity(ctx.entity).get::<Self>() else {
            return;
        };

        let schedules = world
            .get_resource::<ModSchedules>()
            .cloned()
            .unwrap_or_else(ModSchedules::empty);

        world.commands().queue(DisableModuleSystemSet {
            set: ModuleSystemSet::Module(module.id.clone()),
            schedules,
        });

        if let Some(generation) = module.active_generation {
            world.commands().queue(DisableModuleSystemSet {
                set: ModuleSystemSet::Generation {
                    id: module.id.clone(),
                    generation,
                },
                schedules: world
                    .get_resource::<ModSchedules>()
                    .cloned()
                    .unwrap_or_else(ModSchedules::empty),
            });
        }
    }
}

#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub enum ModuleSystemSet {
    #[default]
    All,
    Module(ModuleId),
    Generation { id: ModuleId, generation: ModuleGeneration },
    Access(ModAccess),
}

impl ModuleSystemSet {
    pub fn module(id: impl Into<ModuleId>) -> Self {
        Self::Module(id.into())
    }

    pub fn generation(id: impl Into<ModuleId>, generation: ModuleGeneration) -> Self {
        Self::Generation {
            id: id.into(),
            generation,
        }
    }
}

impl From<&str> for ModuleId {
    fn from(value: &str) -> Self {
        ModuleId::new(value)
    }
}

impl From<String> for ModuleId {
    fn from(value: String) -> Self {
        ModuleId::new(value)
    }
}

#[derive(SystemParam)]
pub struct Modules<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, bevy_asset::AssetServer>,
    modules: Query<'w, 's, Entity, With<Module>>,
}

impl Modules<'_, '_> {
    pub fn spawn<'a>(&mut self, id: impl Into<ModuleId>, path: impl Into<bevy_asset::AssetPath<'a>>) -> Entity {
        let id = id.into();
        let asset = self.asset_server.load(path);
        self.commands.spawn((Module::new(id, asset),)).id()
    }

    pub fn despawn(&mut self, entity: Entity) {
        debug_assert!(self.modules.contains(entity));
        self.commands.entity(entity).despawn();
    }
}
```

### Notes

- `Modules` SystemParam is optional for Phase 0/1, but sketching it early helps keep the API shape parallel to `Mods`.
- `ModAccess` is intentionally reused for now.
- `on_despawn` should remove systems only, not entities/resources.

---

## `src/module_plugin.rs`

```rust
use bevy_app::prelude::*;

use crate::{
    cleanup::disable_mod_system_sets, // reused only if desired
    module_reload::{
        disable_module_system_sets,
        run_module_reload,
        ModuleGenerationCounter,
        ModuleReloadQueue,
    },
    workspace::{WorkspaceInventory, WorldComposition},
};

#[derive(Default)]
pub struct WasvyWorkspacePlugin {
    config_path: Option<std::path::PathBuf>,
    requested_modules: Vec<crate::modules::ModuleId>,
}

impl WasvyWorkspacePlugin {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            config_path: Some(path.into()),
            requested_modules: Vec::new(),
        }
    }

    pub fn with_modules<I, S>(mut self, modules: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<crate::modules::ModuleId>,
    {
        self.requested_modules = modules.into_iter().map(Into::into).collect();
        self
    }
}

impl Plugin for WasvyWorkspacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModuleGenerationCounter>()
            .init_resource::<ModuleReloadQueue>()
            .init_resource::<WorkspaceInventory>()
            .init_resource::<WorldComposition>()
            .add_message::<crate::module_reload::DisableModuleSystemSet>()
            .add_systems(First, (run_module_reload, disable_module_system_sets));

        // Phase 0/1 intentionally omit real config parsing / discovery.
        // Later phases will populate WorkspaceInventory and WorldComposition.
    }
}
```

### Notes

- Keep `WasvyWorkspacePlugin` small in Phase 0.
- Do not parse `wasvy.toml` yet unless trivial.

---

## `src/workspace.rs`

```rust
use std::path::PathBuf;

use bevy_ecs::prelude::*;

use crate::modules::ModuleId;

#[derive(Debug, Clone)]
pub struct WorkspaceModuleEntry {
    pub id: ModuleId,
    pub path: PathBuf,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct WorkspaceInventory {
    pub modules: Vec<WorkspaceModuleEntry>,
}

impl WorkspaceInventory {
    pub fn module(&self, id: &ModuleId) -> Option<&WorkspaceModuleEntry> {
        self.modules.iter().find(|entry| &entry.id == id)
    }

    pub fn insert(&mut self, entry: WorkspaceModuleEntry) {
        self.modules.push(entry);
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct WorldComposition {
    pub active_modules: Vec<ModuleId>,
}

impl WorldComposition {
    pub fn includes(&self, id: &ModuleId) -> bool {
        self.active_modules.iter().any(|candidate| candidate == id)
    }
}
```

### Notes

- This is intentionally config-agnostic.
- Parsing and profile support belong later.

---

## `src/module_reload.rs`

```rust
use anyhow::{Context, Result};
use bevy_asset::{AssetEvent, AssetId, Assets};
use bevy_ecs::{
    prelude::*,
    schedule::{ScheduleCleanupPolicy, ScheduleError},
    system::SystemState,
};
use bevy_log::prelude::*;
use bevy_platform::collections::{HashMap, HashSet};

use crate::{
    access::ModAccess,
    asset::{AssetNotFound, ModAsset},
    modules::{Module, ModuleGeneration, ModuleId, ModuleReloadStatus, ModuleSystemSet, ReloadBlockedReason},
    schedule::ModSchedules,
    system::AddSystems,
};

#[derive(Resource, Default)]
pub(crate) struct ModuleGenerationCounter(u64);

impl ModuleGenerationCounter {
    pub fn next_generation(&mut self) -> ModuleGeneration {
        self.0 += 1;
        ModuleGeneration(self.0)
    }
}

pub(crate) struct PendingModuleReload {
    pub module_entity: Entity,
    pub asset_id: AssetId<ModAsset>,
    pub requested_generation: ModuleGeneration,
}

#[derive(Resource, Default)]
pub(crate) struct ModuleReloadQueue(pub Vec<PendingModuleReload>);

pub(crate) struct PlannedModuleSystems {
    pub asset_version: bevy_ecs::change_detection::Tick,
    pub add_systems: AddSystems,
}

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
    modules: Query<'w, 's, (Entity, &'static mut Module, Option<&'static Name>)>,
}

pub(crate) fn run_module_reload(
    world: &mut World,
    param: &mut SystemState<ModuleSetup>,
    mut counter: Local<ModuleGenerationCounter>,
    mut queue: Local<Vec<PendingModuleReload>>,
) {
    let ModuleSetup {
        mut events,
        assets,
        mut modules,
    } = param.get_mut(world);

    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };

        for (entity, mut module, _name) in modules.iter_mut().filter(|(_, module, _)| module.asset().id() == *id) {
            let generation = counter.next_generation();
            module.set_pending_generation(generation);
            queue.push(PendingModuleReload {
                module_entity: entity,
                asset_id: *id,
                requested_generation: generation,
            });
        }
    }

    // Phase 1: process queue serially, one transaction at a time.
    while let Some(pending) = queue.pop() {
        if let Err(err) = apply_module_reload(world, pending) {
            error!("Module reload transaction failed: {err:?}");
        }
    }
}

fn apply_module_reload(world: &mut World, pending: PendingModuleReload) -> Result<()> {
    let (module_id, old_generation, accesses) = {
        let mut query = world.query::<&Module>();
        let module = query.get(world, pending.module_entity)
            .context("missing Module during reload transaction")?;
        (
            module.id().clone(),
            module.active_generation(),
            module.accesses().copied().collect::<Vec<_>>(),
        )
    };

    let planned = plan_module_systems(world, pending.asset_id)
        .context("module registration planning failed")?;

    if let Some(old_generation) = old_generation {
        let schedules = schedules_for_accesses(world, &accesses);
        world.commands().queue(DisableModuleSystemSet {
            set: ModuleSystemSet::Generation {
                id: module_id.clone(),
                generation: old_generation,
            },
            schedules,
        });
    }

    planned.add_systems.add_module_systems(
        world,
        &accesses,
        &module_id,
        pending.module_entity,
        pending.requested_generation,
        pending.asset_id,
        &planned.asset_version,
    )?;

    if let Some(mut module) = world.get_mut::<Module>(pending.module_entity) {
        module.activate_generation(pending.requested_generation);
    }

    Ok(())
}

fn plan_module_systems(
    world: &mut World,
    asset_id: AssetId<ModAsset>,
) -> Result<PlannedModuleSystems> {
    let change_tick = world.change_tick();
    let mut assets = world.get_resource_mut::<Assets<ModAsset>>().expect("ModAsset store exists");
    let asset = assets.get_mut(asset_id).ok_or(AssetNotFound)?;

    let asset_version = match asset.version() {
        Some(version) => version,
        None => {
            // this will need an actual setter/refactor in src/asset.rs
            change_tick
        }
    };

    // Placeholder shape only.
    // Real implementation will refactor `ModAsset::initiate` into planning vs activation.
    Ok(PlannedModuleSystems {
        asset_version,
        add_systems: AddSystems::default(),
    })
}

fn schedules_for_accesses(world: &World, accesses: &[ModAccess]) -> ModSchedules {
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

pub(crate) fn disable_module_system_sets(
    world: &mut World,
    param: &mut SystemState<MessageReader<DisableModuleSystemSet>>,
) {
    let mut messages = param.get_mut(world);

    let mut remove: HashMap<_, HashSet<_>> = HashMap::default();
    for DisableModuleSystemSet { set, schedules } in messages.read() {
        for schedule in schedules.0.iter() {
            remove.entry(schedule.schedule_label()).or_default().insert(set.clone());
        }
    }

    for (label, sets) in remove {
        let mut schedules = world.get_resource_mut::<Schedules>().expect("running in App");
        let Some(mut schedule) = schedules.remove(label) else {
            continue;
        };

        for set in sets {
            if let Err(error) = schedule.remove_systems_in_set(
                set.clone(),
                world,
                ScheduleCleanupPolicy::RemoveSetAndSystems,
            ) {
                if !matches!(error, ScheduleError::SetNotFound) {
                    warn!("Unable to remove module system set {set:?}: {error}");
                }
            }
        }

        world.resource_mut::<Schedules>().insert(schedule);
    }
}
```

### Notes

- This file intentionally mirrors the current `src/setup.rs` + `src/cleanup.rs` split but for Modules.
- The `plan_module_systems` stub should drive the `src/asset.rs` refactor.
- `Local<ModuleGenerationCounter>` can later become `ResMut<ModuleGenerationCounter>` if desired.

---

## `src/system.rs` additions

Add a parallel helper rather than mutating the Mod path too aggressively.

```rust
impl AddSystems {
    pub(crate) fn add_module_systems(
        self,
        world: &mut World,
        accesses: &[crate::access::ModAccess],
        module_id: &crate::modules::ModuleId,
        module_entity: Entity,
        generation: crate::modules::ModuleGeneration,
        asset_id: AssetId<ModAsset>,
        asset_version: &Tick,
    ) -> anyhow::Result<()> {
        for access in accesses {
            let mod_schedules = access.schedules(world);
            for (schedule, systems) in self.0.iter() {
                let Some(schedule) = mod_schedules
                    .evaluate(schedule)
                    .map(|schedule| schedule.schedule_label())
                else {
                    bevy_log::warn!(
                        "Module tried adding systems to disabled schedule {schedule:?}"
                    );
                    continue;
                };

                for system in systems {
                    let table = wasmtime_wasi::ResourceTable::new(); // placeholder only
                    let _ = &table;
                    // Real implementation should parallel existing `add_system` path.
                }
            }
        }

        Ok(())
    }

    pub(crate) fn module_schedule(
        sys: &crate::host::WasmSystem,
        world: &mut World,
        module_id: &crate::modules::ModuleId,
        module_entity: Entity,
        generation: crate::modules::ModuleGeneration,
        asset_id: &AssetId<ModAsset>,
        asset_version: &Tick,
        access: &crate::access::ModAccess,
    ) -> anyhow::Result<bevy_ecs::schedule::ScheduleConfigs<bevy_ecs::system::BoxedSystem>> {
        let schedule = Self::schedule(
            sys,
            world,
            module_entity,
            module_id.as_str(),
            asset_id,
            asset_version,
            access,
        )?
        .in_set(crate::modules::ModuleSystemSet::All)
        .in_set(crate::modules::ModuleSystemSet::Module(module_id.clone()))
        .in_set(crate::modules::ModuleSystemSet::Generation {
            id: module_id.clone(),
            generation,
        })
        .in_set(crate::modules::ModuleSystemSet::Access(*access));

        Ok(schedule)
    }
}
```

### Notes

- This is deliberately a thin fork of the existing Mod helper shape.
- Later, the Mod and Module helpers can be unified under a generic tagging strategy.

---

## `src/asset.rs` refactor target skeleton

The key Phase 1 change is separating planning from activation.

```rust
impl ModAsset {
    pub(crate) fn plan_systems(
        world: &mut World,
        asset_id: &AssetId<ModAsset>,
    ) -> Result<crate::module_reload::PlannedModuleSystems> {
        let change_tick = world.change_tick();

        let mut assets = world
            .get_resource_mut::<Assets<Self>>()
            .expect("ModAssets be registered");
        let asset = assets.get_mut(*asset_id).ok_or(AssetNotFound)?;

        let asset_version = match asset.version {
            Some(version) => version,
            None => {
                asset.version = Some(change_tick);
                change_tick
            }
        };

        let instance_pre = asset.instance_pre.clone();
        let engine = world
            .get_resource::<crate::engine::Engine>()
            .expect("Engine should never be removed from world");

        let mut runner = crate::runner::Runner::new(engine);
        let mut systems = crate::system::AddSystems::default();

        let config = crate::runner::Config::Setup(crate::runner::ConfigSetup {
            world,
            add_systems: &mut systems,
        });

        let app = runner.new_resource(crate::host::WasmApp).expect("Table has space left");
        super::asset::call(
            &mut runner,
            &instance_pre,
            config,
            "setup",
            &[wasmtime::component::Val::Resource(app)],
            &mut [],
        )?;

        Ok(crate::module_reload::PlannedModuleSystems {
            asset_version,
            add_systems: systems,
        })
    }
}
```

### Explicit design rule

- `plan_systems` must not despawn entity state
- old `initiate` can remain for Mod workflow
- Module workflow should call `plan_systems`

---

## `src/lib.rs` export skeleton

```rust
pub mod module_plugin;
pub mod modules;
pub(crate) mod module_reload;
pub mod workspace;
```

Keep existing exports unchanged.

---

## `src/prelude.rs` export skeleton

```rust
pub use crate::module_plugin::WasvyWorkspacePlugin;
pub use crate::modules::{
    Module,
    ModuleGeneration,
    ModuleId,
    ModuleReloadStatus,
    ModuleSystemSet,
    Modules,
    ReloadBlockedReason,
};
pub use crate::workspace::{WorldComposition, WorkspaceInventory, WorkspaceModuleEntry};
```

Keep all current Mod-facing exports too.

---

## Test skeletons

## `tests/modules_runtime.rs`

```rust
use wasvy::prelude::*;

#[test]
fn module_id_is_stable() {
    let a = ModuleId::new("combat");
    let b = ModuleId::new("combat");
    assert_eq!(a, b);
}

#[test]
fn module_generation_counter_increments() {
    let mut counter = wasvy::module_reload::ModuleGenerationCounter::default();
    let a = counter.next_generation();
    let b = counter.next_generation();
    assert_ne!(a, b);
}
```

## `tests/modules_reload.rs`

```rust
#[test]
fn module_reload_keeps_old_generation_active_when_registration_fails() {
    // create world
    // install module runtime resources
    // spawn a Module with active_generation = Some(...)
    // inject a pending reload that will fail planning
    // assert active_generation unchanged
    // assert reload_status == Blocked(RegistrationFailed)
}

#[test]
fn module_reload_does_not_despawn_entities_by_default() {
    // create world
    // create a module-owned entity/resource
    // perform successful module reload transaction
    // assert entity/resource still exists
}
```

---

## Immediate implementation guidance

If you actually start coding these phases now, the safest order is:

1. add `src/modules.rs` + exports
2. add `src/workspace.rs`
3. add `src/module_plugin.rs`
4. add `src/module_reload.rs` with queue/message scaffolding only
5. refactor `src/asset.rs` to add `plan_systems`
6. add `AddSystems::add_module_systems` in `src/system.rs`
7. wire the reload transaction
8. add tests

That gives you a working runtime base before touching macros or CLI.
