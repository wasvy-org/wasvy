# Wasvy Modules: Phase 0 + Phase 1 Engineering Checklist

This is the concrete implementation checklist for the first two phases of `docs/plans/wasvy-modules-implementation-plan.md`.

It intentionally stops before:

- the full new guest ABI
n- resource params
- authoring macros
- CLI workflow polish

The purpose of these phases is to make the runtime capable of hosting a second product surface called **Wasvy Modules** without breaking the existing **Mod** workflow.

---

## Phase 0 objective

Create parallel runtime/documentation/API scaffolding for **Wasvy Modules**.

Success means:

- the crate exports new Module-facing names
- the old Mod-facing names still work unchanged
- the codebase has an obvious place for Module-specific runtime code
- no user-facing behavior needs to change yet

---

## Phase 1 objective

Introduce a dedicated Module runtime model that supports:

- stable Module identity independent from asset path
- active/pending generations
- preserve-world-state reload
- transactional swap of scheduled systems
- keeping the old Module active on reload failure

Success means:

- Module reload no longer depends on `ModDespawnBehaviour`
- Module reload can swap code while preserving ECS state
- the runtime can represent “new artifact loaded but not yet activated”

---

## Current-code constraints to respect

### Reuse, do not rewrite

Keep reusing:

- `src/engine.rs`
- `src/runner.rs`
- `src/host/*`
- `src/query.rs`
- `src/component.rs`
- most of `src/system.rs`
- schedule cleanup in `src/cleanup.rs`
- hot asset events in `src/setup.rs`

### Do not break public Mod APIs yet

Do not rename/remove yet:

- `Mod`
- `Mods`
- `ModLoaderPlugin`
- `ModSystemSet`
- `ModAccess`
- `ModAsset`

Phase 0/1 should add a second surface, not migrate the first.

---

# Phase 0 checklist

## 0.1 Add Module runtime namespace files

Create these new files:

- `src/modules.rs`
- `src/module_plugin.rs`
- `src/workspace.rs`
- `src/module_reload.rs`

Optional split if it gets large:

- `src/modules/access.rs`
- `src/modules/state.rs`
- `src/modules/setup.rs`

### Why

Right now the only top-level runtime surface is mod-centric (`src/mods.rs`, `src/plugin.rs`, `src/setup.rs`).
The Module surface needs a separate home immediately so later changes do not become a rename tangle.

---

## 0.2 Add new public exports

Update `src/lib.rs` to expose the new namespace/modules.

Add at least:

- `pub mod modules;`
- `pub mod module_plugin;`
- `pub mod workspace;`
- `pub(crate) mod module_reload;`

Update `src/prelude.rs` with new exports.

Add at least:

- `pub use crate::modules::{Module, ModuleId, ModuleGeneration, ModuleReloadStatus, ModuleSystemSet};`
- `pub use crate::module_plugin::WasvyWorkspacePlugin;`

Do **not** remove old prelude exports.

---

## 0.3 Define initial core types in `src/modules.rs`

Add concrete initial runtime types.

## `ModuleId`

Recommended shape:

```rust
#[derive(Clone, Debug, Hash, PartialEq, Eq, Reflect)]
pub struct ModuleId(pub String);
```

Requirements:

- durable stable name
- cheap to clone
- usable in maps/sets/system sets
- visible in logs/errors

Add helpers:

- `ModuleId::new(...)`
- `as_str()`
- `Display`

## `ModuleGeneration`

Recommended shape:

```rust
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Reflect)]
pub struct ModuleGeneration(pub u64);
```

Requirements:

- monotonic runtime generation number
- not tied directly to Bevy `Tick`
- easy to compare/log

## `ModuleReloadStatus`

Recommended MVP enum:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub enum ModuleReloadStatus {
    Active,
    Pending,
    Blocked(ReloadBlockedReason),
}
```

Also define:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub enum ReloadBlockedReason {
    RegistrationFailed,
    CompatibilityFailed,
}
```

Do not overdesign yet.

## `Module`

Recommended MVP component/resource record:

```rust
#[derive(Component, Reflect)]
pub struct Module {
    id: ModuleId,
    asset: Handle<ModAsset>,
    active_generation: Option<ModuleGeneration>,
    pending_generation: Option<ModuleGeneration>,
    reload_status: ModuleReloadStatus,
    access: HashSet<ModAccess>,
}
```

Notes:

- reusing `Handle<ModAsset>` in Phase 0/1 is good enough
- reuse `ModAccess` initially rather than inventing Module-specific access yet
- do not bake compatibility details into this type yet

Also add convenience methods mirroring `Mod` where useful:

- `new(id, asset)`
- `id()`
- `asset()`
- `active_generation()`
- `pending_generation()`
- `reload_status()`
- `enable_access(...)`
- `disable_access(...)`
- `accesses()`

---

## 0.4 Define `ModuleSystemSet`

Add a parallel system-set type instead of reusing `ModSystemSet`.

Recommended shape:

```rust
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub enum ModuleSystemSet {
    #[default]
    All,
    Module(ModuleId),
    Generation { id: ModuleId, generation: ModuleGeneration },
    Access(ModAccess),
}
```

### Why include both `Module` and `Generation`

- `Module(ModuleId)` gives a stable grouping for all systems belonging to a module
- `Generation { ... }` gives exact removal/swap targeting during reload
- `Access(ModAccess)` preserves existing parallelism model

Do not key the new set by `Entity`; that is too tied to old `Mod` runtime semantics.

---

## 0.5 Add a generation counter resource

In `src/modules.rs` or `src/module_reload.rs`, add:

```rust
#[derive(Resource, Default)]
pub(crate) struct ModuleGenerationCounter(u64);
```

Add helper:

- `next_generation(&mut self) -> ModuleGeneration`

### Why

Current `ModAsset.version()` uses `Tick`, which is fine for stale-system no-op checks, but Phase 1 needs a runtime generation identity not derived from Bevy change ticks.

---

## 0.6 Add placeholder `WasvyWorkspacePlugin`

In `src/module_plugin.rs`, add a minimal plugin that only installs the scaffolding resources.

MVP install list:

- `ModuleGenerationCounter`
- any future `WorkspaceInventory` placeholder resource
- maybe a `ModuleReloadQueue` placeholder resource
- `DisableModuleSystemSet` message/system if introduced in Phase 1

This plugin does **not** need to load real modules yet.

### Goal

Compile-time surface first, behavior second.

---

## 0.7 Add placeholder `WorkspaceInventory` and `WorldComposition`

In `src/workspace.rs` add small initial types:

```rust
#[derive(Resource, Debug, Clone, Default)]
pub struct WorkspaceInventory {
    pub modules: Vec<WorkspaceModuleEntry>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceModuleEntry {
    pub id: ModuleId,
    pub path: PathBuf,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct WorldComposition {
    pub active_modules: Vec<ModuleId>,
}
```

No parser needed yet.

These are placeholders so later plugin work does not have to invent public shapes under pressure.

---

## 0.8 Add docs and comments in code to make the split explicit

Add module-level docs in new files saying:

- `mods.rs` = public modding workflow
- `modules.rs` = Wasvy Modules workflow

Add doc comments in `src/prelude.rs` if useful.

This matters because the current codebase vocabulary is entirely mod-centric.

---

## 0.9 Add Phase 0 tests

Create new test file(s):

- `tests/modules_runtime.rs`

Initial tests:

- `module_id_is_hashable_and_stable`
- `module_generation_counter_increments`
- `module_system_set_compares_by_id_and_generation`
- `workspace_plugin_builds`

No runtime reload behavior yet.

---

# Phase 1 checklist

## 1.1 Add module reload transaction resource(s)

In `src/module_reload.rs`, define the runtime queue/state for pending activations.

Recommended types:

```rust
#[derive(Resource, Default)]
pub(crate) struct ModuleReloadQueue(pub Vec<PendingModuleReload>);

pub(crate) struct PendingModuleReload {
    pub module_entity: Entity,
    pub asset_id: AssetId<ModAsset>,
    pub requested_generation: ModuleGeneration,
}
```

Optional richer type:

```rust
pub(crate) enum PendingModuleReloadState {
    Loaded,
    Registered(AddSystems),
    ReadyToSwap,
    Blocked(ReloadBlockedReason),
}
```

For Phase 1, simple queue + world lookups is enough.

---

## 1.2 Split old mod setup path from new module setup path

Do **not** jam Modules into `src/setup.rs` directly without separation.

Recommended approach:

- keep `src/setup.rs::run_setup` for Mods
- add `src/module_reload.rs::run_module_reload` (or `src/modules_setup.rs`)
- register it from `WasvyWorkspacePlugin`, not from `ModLoaderPlugin`

### Why

Current `run_setup` is built around:

- `Query<(Entity, Ref<Mod>, Option<&Name>)>`
- `RanWith { mod_id, access }`
- immediate call to `ModAsset::initiate(...)`
- running `ModStartup`

This is too coupled to old Mod semantics and entity-despawn defaults.

---

## 1.3 Add a Module-side asset event scanner

In the new module reload system, mirror the useful part of `src/setup.rs`:

- listen for `AssetEvent<ModAsset>`
- find Modules whose asset handle matches the loaded asset
- allocate a fresh `ModuleGeneration`
- mark module `reload_status = Pending`
- set `pending_generation`
- enqueue `PendingModuleReload`

Do **not** activate immediately.

### Important behavior

A newly loaded artifact should be represented as:

- known by runtime
- not yet active
- old generation still active until swap succeeds

That is the key new semantic absent from current Mods.

---

## 1.4 Generalize `ModAsset::initiate` or add parallel `ModuleAsset::register`

Current `src/asset.rs::ModAsset::initiate(...)` does too much:

- acquires asset version
- despawns old mod-owned entities depending on `ModDespawnBehaviour`
- runs guest `setup`
- immediately registers systems

For Modules we need a two-step flow.

### Recommended refactor

Extract the reusable part into something like:

```rust
pub(crate) fn plan_systems(
    world: &mut World,
    asset_id: &AssetId<ModAsset>,
    module_name: &str,
) -> Result<PlannedModuleSystems>
```

Where `PlannedModuleSystems` holds at least:

- `asset_version: Tick`
- `add_systems: AddSystems`
- maybe future registration metadata

Alternative if you want less churn now:
- add a parallel `register_without_cleanup(...)`

But the better long-term move is to split **planning** from **activation**.

### Explicitly remove for Modules

Do not execute this old logic in the Module path:

```rust
if ModDespawnBehaviour::should_despawn_entities(world) { ... }
```

That belongs only to public Mods.

---

## 1.5 Introduce `PlannedModuleSystems`

Add a concrete struct in `src/module_reload.rs` or `src/system.rs`:

```rust
pub(crate) struct PlannedModuleSystems {
    pub asset_version: Tick,
    pub add_systems: AddSystems,
}
```

This is the output of registration/planning before activation.

### Why

The Module path needs to separate:

- artifact loaded
- registration successful
- swap committed

This struct becomes the boundary object.

---

## 1.6 Add `DisableModuleSystemSet`

Do not reuse `DisableSystemSet` directly because it is hard-coded to `ModSystemSet`.

In `src/module_reload.rs` or `src/cleanup.rs`, add parallel message + cleanup system:

```rust
#[derive(Message)]
pub(crate) struct DisableModuleSystemSet {
    pub(crate) set: ModuleSystemSet,
    pub(crate) schedules: ModSchedules,
}
```

Add cleanup function:

- `disable_module_system_sets(...)`

### Possible refactor

If desired, you can extract a generic cleanup helper that takes any set type implementing `SystemSet + Clone + ...`.
But for Phase 1, a parallel message/system is acceptable and lower risk.

---

## 1.7 Activate new generation by schedule swap, not entity reset

Implement swap logic in `run_module_reload`:

1. take next pending reload
2. plan registration (`PlannedModuleSystems`)
3. if planning fails:
   - keep `active_generation` unchanged
   - clear or retain `pending_generation` as appropriate
   - set `reload_status = Blocked(RegistrationFailed)`
   - log clearly
4. if planning succeeds:
   - enqueue removal of old `ModuleSystemSet::Generation { ... }` for all accesses/schedules
   - register new systems tagged with:
     - `ModuleSystemSet::All`
     - `ModuleSystemSet::Module(module_id.clone())`
     - `ModuleSystemSet::Generation { id: module_id.clone(), generation }`
     - access set
   - set `active_generation = Some(generation)`
   - clear `pending_generation`
   - set `reload_status = Active`

### Important

Do not remove the old generation until the new generation’s planning succeeded.

That transaction boundary is the main value of Phase 1.

---

## 1.8 Add generation-aware scheduling helper

Current `src/system.rs::AddSystems::add_systems(...)` takes:

- `mod_id: Entity`
- `mod_name: &str`
- `asset_id`
- `asset_version`
- `access`

For Modules, add a parallel path or generic helper that takes:

- `module_id: &ModuleId`
- `generation: ModuleGeneration`
- maybe `module_entity: Entity` if still needed for ownership markers later

### Recommendation

Do not mutate the current Mod path unless extraction is trivial.
Add a new helper first, then deduplicate later.

Likely shapes:

- `AddSystems::add_module_systems(...)`
- `AddSystems::schedule_module(...)`

### Minimal tagging change needed

The new helper must tag generated systems with `ModuleSystemSet`, not `ModSystemSet`.

---

## 1.9 Keep stale-system no-op check, but scope it to Module generation path later

Current `dynamic_system` uses `asset.version() != Some(input.asset_version)` to skip stale systems.
That is still useful.

For Phase 1:

- keep using asset-version stale skipping for old/new wasm artifact mismatch
- additionally rely on `DisableModuleSystemSet` to remove old generation systems

Later, if needed, you can add explicit `ModuleGeneration` to the system input too.

For now, do not overcomplicate this.

---

## 1.10 Decide what `Module` despawn means in Phase 1

Add `ComponentHooks::on_despawn` for `Module` analogous to `Mod`, but module-focused.

On Module despawn:

- remove all active generation systems
- remove all pending generation systems if any were scheduled
- do **not** auto-despawn module-owned entities/resources in the Module workflow

This is a key semantic split from `Mod`.

---

## 1.11 Introduce minimal reload failure diagnostics now

Even before compatibility diffing exists, add structured errors for:

- missing asset
- registration/setup failure
- swap failure

Recommended log pattern:

- `Module reload blocked: {module_id}`
- `Previous generation remains active`
- root cause text

Do not wait for full compatibility diffing to start making the failure transactional and understandable.

---

## 1.12 Add Phase 1 tests

Add tests in new file(s):

- `tests/modules_reload.rs`

Recommended tests:

1. `module_reload_assigns_new_generation_on_asset_load`
2. `module_reload_keeps_old_generation_active_when_registration_fails`
3. `module_reload_replaces_system_generation_without_entity_despawn`
4. `module_despawn_removes_module_system_sets`
5. `module_reload_status_moves_pending_to_active_on_success`

### One very important regression test

Create a test proving the Module path does **not** execute old mod despawn behavior during reload.

Because this is the main semantic divergence from `src/asset.rs` today.

---

# Exact file edit map

## Must create

- `src/modules.rs`
- `src/module_plugin.rs`
- `src/module_reload.rs`
- `src/workspace.rs`
- `tests/modules_runtime.rs`
- `tests/modules_reload.rs`

## Must edit

- `src/lib.rs`
- `src/prelude.rs`
- `src/system.rs`
- `src/asset.rs`
- `src/cleanup.rs`

## Probably edit

- `src/plugin.rs` (docs only, maybe note split later)
- `README.md` (later, not required for Phase 0/1)

---

# Recommended implementation order inside Phase 0/1

1. create new files and public exports
2. add `ModuleId`, `ModuleGeneration`, `Module`, `ModuleSystemSet`
3. add `WasvyWorkspacePlugin` placeholder
4. add generation counter + queue types
5. add `DisableModuleSystemSet`
6. add module asset-event scan system
7. refactor `src/asset.rs` into planning vs activation boundary
8. add module schedule registration helper in `src/system.rs`
9. implement transactional swap logic
10. add despawn behavior for Module
11. add tests

---

# Definition of done for Phase 0 + 1

Phase 0 + 1 are done when all of these are true:

- crate exports parallel Module-facing runtime types without breaking Mod-facing ones
- a Module has a stable `ModuleId` and tracked active/pending generation
- asset reload can produce a pending Module generation
- Module activation swaps scheduled systems only after successful planning
- old generation remains active on failure
- Module reload does not despawn Module-owned ECS state by default
- tests prove the Module path is semantically distinct from the Mod path

Once that is true, we have a real runtime base for:

- Phase 2 ABI work
- Phase 3 resources
- Phase 4 authoring macros