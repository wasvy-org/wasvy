# Wasvy Modules Implementation Plan

This plan translates the `docs/rfcs/rust-modular-game-authoring.md` RFC into concrete engineering work mapped onto the current codebase.

## Objective

Add a distinct product surface called **Wasvy Modules** without breaking the existing **Mod** workflow.

The end state is:

- public/external workflow remains centered on `Mod`, `Mods`, and `ModLoaderPlugin`
- internal Rust modular-game workflow gets a new surface centered on `Module`, `Module Crate`, `WasvyWorkspacePlugin`, crate-root `wasvy::module!`, `#[wasvy::system(...)]`, and `#[wasvy::on_first_load]`
- guest reload preserves world state by default and treats incompatible persisted state as a **Reload Compatibility Failure**

---

## Current codebase baseline

### Pieces we can reuse heavily

- **Wasmtime + WIT host runtime**
  - `src/engine.rs`
  - `src/runner.rs`
  - `src/host/*`
- **Dynamic system scheduling pipeline**
  - `src/system.rs`
  - `src/query.rs`
  - `src/component.rs`
- **Asset-driven wasm loading and hot reload trigger**
  - `src/asset.rs`
  - `src/setup.rs`
- **Type export / reflection / method indexing**
  - `src/authoring.rs`
  - `src/methods.rs`
  - `src/witgen.rs`
- **Procedural macro crate**
  - `crates/wasvy_macros/src/lib.rs`
- **CLI/workspace substrate**
  - `crates/wasvy_cli/src/runtime.rs`
  - `crates/wasvy_cli/src/source.rs`
  - `crates/wasvy_cli/src/languages/*`

### Places where current code contradicts the RFC

- terminology is still centered on **Mod**
  - `src/mods.rs`
  - `src/plugin.rs`
  - `src/prelude.rs`
  - docs/examples
- reload currently defaults to entity cleanup/despawn
  - `src/mods.rs` (`ModDespawnBehaviour::DespawnEntities`)
  - `src/asset.rs` (reload path despawns tracked entities before re-init)
- guest authoring is still low-level and manual
  - explicit `setup(app)`
  - explicit `System::new(...)`
  - explicit query descriptors
  - handwritten or semi-handwritten WIT/bindings in examples
- guest runtime currently supports only commands + query/component access
  - no explicit resource host ABI yet
  - `src/system.rs` contains TODOs for resource param support
- CLI internals exist, but the actual binary workflow is incomplete
  - `crates/wasvy_cli/src/main.rs`
  - `crates/wasvy_cli/src/remote.rs`

### Important constraint from current architecture

The current runtime already has the right shape for **replace systems, keep world state**:

- system registration is separate from system execution
- systems are tagged by per-mod/per-access sets
- old systems already become inert when asset version mismatches

That means **Wasvy Modules** should be implemented mostly as a new authoring/runtime layer over the existing substrate, not a total runtime rewrite.

---

## Implementation strategy

## Principle 1: add, don’t replace

Do not immediately rename the current public modding surface.

Instead:

- keep `Mod`, `Mods`, `ModLoaderPlugin` intact for the public workflow
- add parallel **Wasvy Modules** types/APIs
- refactor shared internals underneath both surfaces where useful

This avoids blocking the new surface on a risky rename migration.

## Principle 2: split product surfaces above shared runtime

Treat the codebase as:

- **shared runtime substrate**
- **public Mod surface**
- **Wasvy Modules surface**

The shared substrate should absorb common concerns:

- loading guest components
- host bindings
- dynamic scheduling
- reflection/serialization
- asset-triggered reload

The two product surfaces should diverge in:

- authoring macros
- lifecycle semantics
- reload policy
- tooling
- docs/examples

## Principle 3: make preserve-state reload the module default

The Module surface should not reuse `ModDespawnBehaviour::DespawnEntities` as its default behavior.

The new surface should:

- preserve entities/components/resources by default
- treat persisted-state incompatibility as a blocking reload failure
- keep prior generation active when reload cannot be applied

---

## Phase 0 - Internal runtime split and naming scaffolding

### Goal

Prepare the crate for a second product surface without destabilizing the current one.

### Changes

1. Add new top-level module namespace(s)
   - likely candidates:
     - `src/modules.rs`
     - `src/module_plugin.rs`
     - `src/workspace.rs`
     - `src/reload.rs`
2. Add new public exports in `src/lib.rs` and `src/prelude.rs`
   - `Module`
   - `ModuleId` / stable name type
   - `WasvyWorkspacePlugin`
   - `NativeAdapterPlugin` or generated plugin traits/types
3. Keep old names intact
   - `Mod`, `Mods`, `ModLoaderPlugin`
4. Add doc comments making the split explicit

### Files most affected

- `src/lib.rs`
- `src/prelude.rs`
- new `src/modules*.rs` files
- README / examples later

### Deliverable

A compilable crate with parallel namespaces for Mods and Modules, even before the new surface does much.

---

## Phase 1 - Module runtime identity and preserve-state reload core

### Goal

Introduce a dedicated runtime representation for Modules that preserves world state and versions code generations separately.

### Recommended model

Add a dedicated component/resource model instead of forcing `Mod` semantics to serve both worlds.

Suggested concepts:

- `ModuleComponent` or `LoadedModule`
  - stable module name
  - wasm asset handle
  - active generation/version
  - activation status
- `ModuleGeneration`
  - stable identity + code generation/version
- `ModuleSystemSet`
  - analogous to `ModSystemSet` but keyed by stable module identity and generation
- `ModuleReloadStatus`
  - active / pending / blocked-by-compatibility

### Changes

1. Create a dedicated Module component/runtime record
2. Separate “asset identity” from “module identity”
3. Track currently active generation independently from last loaded artifact
4. Rework reload transaction logic to:
   - plan registration for new artifact
   - validate compatibility
   - swap systems only on success
   - keep old generation active on failure

### Current code to adapt

- `src/mods.rs`
  - do not reuse directly as-is; too mod-centric and entity-despawn-oriented
- `src/asset.rs`
  - current `version: Option<Tick>` mechanism is useful and should be generalized
- `src/setup.rs`
  - useful event-driven trigger point, but should split Module reload transaction from old Mod setup flow
- `src/cleanup.rs`
  - schedule cleanup machinery is reusable

### Deliverable

A parallel Module runtime that can swap system generations without despawning module-owned world state.

---

## Phase 2 - New guest ABI for Wasvy Modules

### Goal

Add the guest contract needed by crate-root Module authoring.

### Why a new ABI layer is needed

The current `wit/wasvy-ecs.wit` contract assumes:

- explicit guest `setup(app)`
- commands + query/component-oriented params only
- no first-load init concept
- no explicit resource param support

Wasvy Modules needs more structure.

### Recommendation

Add a new internal/versioned guest ABI for Modules rather than overloading the public mod ABI immediately.

Possible shapes:

- new WIT package/version under `wit/`
- or an additive new world/interface alongside current `wasvy:ecs`

### Minimum new capabilities

1. **Registration export**
   - metadata-only
   - equivalent to generated system declaration export(s)
2. **First-load initialization export**
   - optional
   - run once per module activation in a world
3. **Resource param support**
   - `Res<T>` / `ResMut<T>` equivalents
4. Optional future place for compatibility/schema metadata

### Current files to change

- `wit/wasvy-ecs.wit` or new sibling WIT file
- `src/lib.rs` bindgen block
- `src/host/*`
- `src/system.rs`
- `src/runner.rs`

### Deliverable

A Module-specific guest ABI that supports Registration, First-load Initialization, queries, commands, and resources.

---

## Phase 3 - Resource support in runtime execution

### Goal

Implement the MVP resource subset promised by the RFC.

### Current gap

`src/system.rs` explicitly shows resource support is not implemented yet:

- TODO around `FilteredResourcesMut`
- params currently centered on commands + query only

### Work

1. Extend system param model
   - add `Param::Res(TypePath)`
   - add `Param::ResMut(TypePath)`
2. Extend generated guest metadata and WIT resource types
3. Add host-side resource access adapters
   - likely new `src/host/resource.rs` or equivalent
4. Extend `Runner` / `State` to carry resource-access context
5. Decide how supported resources are filtered/validated

### Files most affected

- `src/system.rs`
- `src/runner.rs`
- `src/host/*`
- `src/component.rs` or sibling resource serialization helpers
- WIT files

### Deliverable

`Res<T>` / `ResMut<T>` support in guest-mode Module execution for supported reflected/serializable types.

---

## Phase 4 - Procedural macros for Wasvy Modules authoring

### Goal

Build the new crate-root Rust authoring surface.

### New macros

1. `wasvy::module! { name: "combat" }`
2. `#[wasvy::system(Update)]`
3. `#[wasvy::on_first_load]`

### Responsibilities

#### `module!`

- crate-root declaration
- emit module metadata
- capture stable module name
- provide a registry anchor for guest/native codegen

#### `#[wasvy::system(...)]`

- validate supported signature subset at compile time
- emit system metadata for:
  - guest Registration generation
  - native adapter generation
- reject unsupported params with good diagnostics

#### `#[wasvy::on_first_load]`

- validate signature subset
- emit first-load metadata/export
- enforce no ambiguity with Registration

### Current macro code to reuse or extend

- `crates/wasvy_macros/src/lib.rs`
  - existing machinery for type-path extraction, generated bindings, inventory submissions, and diagnostics is useful
- `src/authoring.rs`
  - inventory-style registration patterns can be reused or mirrored

### New compile-time validation duties

Macros should enforce the **Module Authoring Contract** explicitly.
They should fail early on unsupported:

- system params
- query shapes
- lifecycle misuse
- duplicate module declarations
- multiple `on_first_load` declarations if disallowed

### Deliverable

A new Rust-first authoring layer where Module crates no longer hand-write guest `setup()` or query metadata.

---

## Phase 5 - Native Adapter Plugin generation

### Goal

Run the same Module source in native mode.

### Design

For each Module Crate, generated metadata should be consumable by:

- guest Registration generation
- native adapter registration

The native path should build a `NativeAdapterPlugin` from the same system declarations.

### Work

1. Generate a native registration function/plugin type from module metadata
2. Implement host-side glue to add the generated systems directly to Bevy schedules
3. Make sure `#[wasvy::on_first_load]` is supported in native mode with the same once-per-world semantics
4. Add tests that compare native and guest behavior for representative supported shapes

### Current code to reuse

- `src/system.rs`
  - current dynamic registration logic helps define schedule/order semantics
- macro-generated metadata from Phase 4

### Deliverable

A working native fallback path for Modules and the start of automated **Dual-mode Equivalence** tests.

---

## Phase 6 - Persisted state compatibility and reload failure reporting

### Goal

Implement **Reload Compatibility Failure** as a first-class developer experience.

### Required behavior

When a Module-private Type or shared persisted type becomes incompatible:

- new generation must not activate
- prior generation must remain active
- the error must tell the developer to relaunch
- the error should include best-effort field-level diffs where possible

### Work

1. Decide how schema identity is recorded
   - reflect/type metadata
   - serialized schema descriptors
   - generated metadata from macros
2. Capture previous and new schema shapes for supported persisted state
3. Diff them best-effort
4. Teach the reload transaction to surface a structured failure
5. Expose failure state through logs/devtools/CLI

### Likely implementation areas

- new `src/reload.rs` or sibling module
- `src/component.rs`
- `src/serialize.rs`
- macro-emitted type metadata
- devtools if live reporting is desired

### Deliverable

Module reload failures become understandable and transactional instead of mysterious or destructive.

---

## Phase 7 - Workspace Inventory and World Composition host surface

### Goal

Add ergonomic host APIs for Module discovery and activation.

### New runtime surface

Suggested types:

- `WasvyWorkspacePlugin`
- `WorkspaceInventory`
- `WorldComposition`

### Responsibilities

#### WorkspaceInventory

- parse workspace config (`wasvy.toml`)
- know available Module Crates and stable names

#### WorldComposition

- select which Modules are active in one host world
- operate by stable Module name only
- optionally seed from config profile/defaults
- allow host code override

#### WasvyWorkspacePlugin

- load inventory
- build activation list
- spawn/load Module runtime entities/records
- integrate with guest reload watcher/tooling later

### Current code to reuse

- `crates/wasvy_cli/src/runtime.rs`
- `crates/wasvy_cli/src/source.rs`
- `crates/wasvy_cli/src/languages/rust.rs`

These already understand source discovery/build identification and should become the basis of Module workspace tooling rather than being treated as a separate experiment.

### Deliverable

A host can opt into Modules by naming them, instead of manually loading wasm asset paths.

---

## Phase 8 - Tooling and `wasvy dev`

### Goal

Make guest-mode modular iteration the default workflow.

### Current gap

`crates/wasvy_cli` has useful internals but the binary is incomplete:

- `main.rs` stops at `todo!()`
- `remote.rs` is `todo!()`

### Recommendation

Repurpose the CLI around **Wasvy Modules** first.
Public mod tooling can remain a later layer.

### MVP CLI behavior

`wasvy dev`

- read `wasvy.toml`
- determine Workspace Inventory
- determine default World Composition
- start host in guest mode
- watch active Module Crates
- rebuild changed Module Crates to wasm
- trigger/rely on reload
- report build/reload failures clearly

`wasvy dev --native`

- start the same game with generated Native Adapter Plugins instead of guest artifacts

### Work

1. finish workspace-aware CLI flow
2. integrate build pipeline with Rust module authoring model
3. surface Reload Compatibility Failures clearly
4. make guest mode the default path

### Deliverable

The product story becomes real: `wasvy dev` is the headline workflow.

---

## Phase 9 - Docs, examples, migration, and tests

### Goal

Make the new surface understandable and trustworthy.

### Docs

1. Add top-level docs section for **Wasvy Modules**
2. Keep existing **Mods** docs separate
3. Add explicit terminology and comparison docs
4. Explain Registration vs First-load Initialization clearly

### Examples

Add new examples dedicated to Modules, not just mods:

- shared API crate
- one Module Crate with `module!` and `#[wasvy::system]`
- host using `WasvyWorkspacePlugin`
- guest-mode reload demo
- native-mode equivalence demo

### Tests

Add tests for:

- macro validation failures
- module identity handling
- one-time first-load semantics
- preserve-state reload success
- Reload Compatibility Failure behavior
- guest/native equivalence for supported shapes

### Migration guidance

Document that:

- existing `ModLoaderPlugin` remains for Mods
- Wasvy Modules is a new internal workflow
- old examples stay valid but are not the preferred modular-game DX story

---

## Recommended sequencing

1. **Phase 0** - namespace/runtime scaffolding
2. **Phase 1** - module runtime identity + preserve-state reload core
3. **Phase 2** - module guest ABI
4. **Phase 4** - authoring macros
5. **Phase 3** - resource support
6. **Phase 5** - native adapter generation
7. **Phase 6** - compatibility failures and diagnostics
8. **Phase 7** - workspace plugin and composition
9. **Phase 8** - CLI/dev workflow
10. **Phase 9** - docs/examples/tests hardening

Reasoning:

- phases 1+2 define the runtime shape
- phase 4 gives developers the new authoring layer
- phase 3 is required for a believable MVP contract
- phase 5 is needed to make dual-mode real
- phases 7+8 make the workflow ergonomic instead of merely possible

---

## Proposed first shippable milestone

A useful first milestone is not the entire RFC.
It is:

- one Module per Module Crate
- crate-root `wasvy::module! { name: ... }`
- `#[wasvy::system(Update)]`
- supported subset: `Commands`, queries, `Res<T>`, `ResMut<T>`
- guest Registration generation
- preserve-state reload without schema diffing yet
- generated Native Adapter Plugin
- a host example activating Modules by stable name

That milestone would prove the core architecture before full CLI and compatibility diff polish.

---

## Risks to manage

1. **Trying to rename the whole old surface too early**
   - avoid; add parallel Module APIs first
2. **Overpromising Bevy parity**
   - enforce the Module Authoring Contract strictly
3. **Entangling public Mod ABI with Module ABI prematurely**
   - keep the Module surface versioned/separate until stable
4. **Implementing resource semantics late**
   - do not call the workflow ergonomic before `Res<T>` / `ResMut<T>` exist
5. **Letting First-load Initialization become setup-by-another-name**
   - keep Registration metadata-only
6. **Shipping without transactional reload failure handling**
   - preserve-state semantics need clear failure behavior

---

## Success criteria

This plan succeeds when a game team can:

1. create a `game_api` crate and a few pure Module Crates
2. declare each Module with `wasvy::module! { name: ... }`
3. write gameplay systems with `#[wasvy::system(...)]`
4. optionally use `#[wasvy::on_first_load]`
5. run `wasvy dev`
6. change one Module Crate
7. see only that Module’s code reload while the world keeps running
8. switch to native mode and run the same Module source through generated native adapters

That is the bar for **Wasvy Modules** becoming a real product surface rather than just an RFC.