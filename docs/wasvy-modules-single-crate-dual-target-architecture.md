# Wasvy Modules: Single-Crate Dual-Target Architecture

## Purpose

This document explains the **final implementation** on this branch: how Wasvy Modules now works as a **single-module-crate dual-target system**.

The goal is simple:

- author each module **once** in Rust
- run that same module in **guest wasm mode** or **native mode**
- select active modules by **stable module name** from `wasvy.toml`
- keep guest build/staging/reload workflow in **tooling**, not in game code

This is the architecture we should think about and share going forward.

---

## 1. Core idea

A **Module Crate** is the unit of gameplay authoring, build, identity, and reload.

Each module crate is built as both:

- `rlib` → used for **native execution**
- `cdylib` → used for **guest wasm execution**

The module author writes one crate with one authoring surface:

```rust
wasvy::module! {
    name: "combat"
}

#[wasvy::on_first_load]
fn init(mut commands: Commands) {
    // first activation setup
}

#[wasvy::system(Update)]
fn tick(mut timeline: ResMut<SharedTimeline>, mut actors: Query<&mut Actor>) {
    // gameplay logic
}
```

That same source drives both runtime paths.

---

## 2. Mental model

## What a module is

A Wasvy Module is:

- one crate
- one durable runtime identity (`name: "combat"`)
- one set of systems
- optional first-load initialization
- hot-reloadable in guest mode
- executable natively through a generated adapter

## What the host is responsible for

The host:

- owns the shared Bevy world
- chooses which modules are active through `wasvy.toml`
- registers shared reflected/resource/component types
- installs `WasvyWorkspacePlugin`
- optionally adds generated native adapter plugins in native mode

## What tooling is responsible for

`wasvy dev`:

- reads the workspace manifest
- finds active module crates
- builds guest wasm artifacts
- stages them into `assets/modules/{module-name}.wasm`
- watches for changes
- rebuilds/restages on change

---

## 3. High-level architecture

```text
Module crate source
  ├─ wasvy::module!
  ├─ #[wasvy::system]
  └─ #[wasvy::on_first_load]
           │
           ├─ native target
           │    └─ generated NativeAdapterPlugin
           │
           └─ wasm target
                └─ generated guest exports + bindings

wasvy.toml
  ├─ workspace inventory
  └─ world composition
           │
           ▼
WasvyWorkspacePlugin
  ├─ installs runtime substrate
  ├─ parses manifest
  ├─ seeds inventory + world composition
  └─ auto-spawns guest modules by module name

wasvy dev
  ├─ builds active module crates for wasm32-wasip2
  ├─ stages assets/modules/{module-name}.wasm
  ├─ runs host
  └─ rebuilds/reloads on change
```

---

## 4. Authoring architecture and macro expansion

### Main files
- `crates/wasvy_macros/src/lib.rs`
- `src/authoring.rs`
- `src/module_guest.rs`

This is where most of the “magic” lives.

The implementation works because the macros do **two different jobs at once**:

1. **validate and register authored functions for native/runtime metadata**
2. **generate a wasm guest-facing wrapper layer** that can call the same logic through WIT handles

## 4.1 `wasvy::module!`

`wasvy::module!` is the crate-root declaration that turns a crate into a Module Crate.

It generates:

- `MODULE_NAME`
- `NativeAdapterPlugin`
- module declaration inventory metadata
- wasm-only `wit_bindgen` guest bindings
- the wasm guest export implementation object
- the guest `register` export
- the guest `on-first-load` export
- exported guest functions for every discovered `#[wasvy::system]`

### What `NativeAdapterPlugin` does

The generated native adapter plugin iterates inventory entries and registers only the systems/first-load functions whose `scope` matches the declaring crate’s `module_path!()`.

That scoping logic lives in `src/authoring.rs` via `module_scope_matches(...)`.

This is what lets a module crate declare multiple exported functions without needing manual plugin wiring.

### What `wasvy::module!` does for wasm builds

On `wasm32`, it generates a private `__wasvy_guest_bindings` module using `wit_bindgen::generate!`.

That generated WIT world is not handwritten by the module author. The macro builds it automatically and wires it to:

- the shared host ECS interface: `wasvy:ecs/app@0.0.7`
- a generated guest world that exports:
  - `register(app)`
  - `on-first-load(commands)`
  - one export per annotated module system

It then implements the generated `Guest` trait for a synthetic type like `__WasvyModuleGuest` and publishes the wasm exports through `__wasvy_guest_bindings::export!(...)`.

## 4.2 `#[wasvy::system(...)]`

The `#[wasvy::system(...)]` macro expands one authored function into several pieces.

Given authored code like:

```rust
#[wasvy::system(Update)]
fn tick(
    mut timeline: ResMut<SharedTimeline>,
    mut actors: Query<&mut Actor>,
) {
    timeline.frame += 1;
    for mut actor in &mut actors {
        actor.health -= 1;
    }
}
```

the macro keeps the original function, then additionally generates:

### Native registration function
A hidden function like:

```rust
fn __wasvy_register_module_system_tick(app: &mut App) {
    app.add_systems(Update, tick);
}
```

This is what the generated native adapter plugin calls.

### Inventory metadata
A `WasvyModuleSystemRegistration` entry containing:

- `scope`
- `export_name` (`"tick"`)
- `register_native`
- `referenced_types`

The `referenced_types` list is derived from the allowed parameter set and is used elsewhere in the module runtime, including schema/compatibility logic.

### Wasm-only guest implementation function
A second hidden function is generated for wasm builds with the **same body** as the authored system, but with **guest wrapper params** instead of Bevy ECS params.

Conceptually it looks like:

```rust
#[cfg(target_arch = "wasm32")]
fn __wasvy_guest_impl_tick(
    mut timeline: wasvy::module_guest::ResMut<SharedTimeline, WorldResource>,
    mut actors: wasvy::module_guest::Query<&mut Actor, Query>,
) {
    // same original body
}
```

This is the crucial trick: the macro copies the authored body into a wasm-only function whose parameters are guest shims.

### Wasm export shim
A final hidden export shim is generated that accepts the **raw WIT resource handles** and wraps them before calling the guest impl:

```rust
#[cfg(target_arch = "wasm32")]
fn __wasvy_guest_export_tick(raw0: WorldResource, raw1: Query) {
    let timeline = wasvy::module_guest::ResMut::new(raw0);
    let actors = wasvy::module_guest::Query::new(raw1);
    __wasvy_guest_impl_tick(timeline, actors);
}
```

So there are really three levels:

1. authored native-looking function
2. generated wasm-only wrapper-typed function
3. generated raw-export shim from WIT handles into wrapper types

## 4.3 `#[wasvy::on_first_load]`

`#[wasvy::on_first_load]` uses the same pattern as `#[wasvy::system]`, but currently validates a stricter contract:

- only `Commands` is supported in MVP

It generates:

- native registration on the startup-like module schedule
- inventory metadata
- a wasm-only guest implementation function
- a raw wasm export shim that wraps the incoming `commands` resource

Semantics are the same in both modes:

- runs once per module activation in a world
- does not rerun on a normal hot reload of the same active module generation

## 4.4 Parameter validation and classification

There are **two related classification passes** in the macro implementation.

### Pass A: native/runtime validation metadata

Functions like `validate_fn_shape(...)`, `validate_system_params(...)`, `validate_first_load_params(...)`, `classify_param(...)`, and `classify_query(...)`:

- reject unsupported signatures
- restrict the supported MVP contract
- convert param types into runtime-facing metadata such as:
  - `Param::Commands`
  - `Param::Res(type_path)`
  - `Param::ResMut(type_path)`
  - `Param::Query(Vec<QueryFor>)`

This pass renders concrete Rust type paths into stable strings.

### Pass B: guest codegen classification

Separately, `guest_module_source(...)`, `classify_guest_param(...)`, and friends rescan the module source file and preserve richer `syn::Type` information so the macro can generate:

- inline guest WIT signatures
- guest wrapper generic types
- `QueryFor::{Ref, Mut, With, Without}` registration values using `TypePath`

This second pass is specifically about **guest export generation**, not just validation.

## 4.5 Crate source scan in `wasvy::module!`

`wasvy::module!` currently reads `src/lib.rs` or `src/main.rs`, parses it as a Rust file, and finds top-level functions annotated with:

- `#[wasvy::system(...)]`
- `#[wasvy::on_first_load]`

It uses that to generate the guest world/export surface automatically.

That is why the current implementation assumes crate-root, top-level module functions for guest codegen.

## 4.6 Generated guest WIT surface

The macro emits a small generated WIT world roughly like:

```wit
world guest {
    use wasvy:ecs/app@0.0.7.{app, commands, query, world-resource};

    export register: func(app: app);
    export on-first-load: func(commands: commands);
    export tick: func(arg0: world-resource, arg1: query);
}
```

The exact argument kinds are derived from the authored Rust signature:

- `Commands` → `commands`
- `Query<...>` → `query`
- `Res<T>` / `ResMut<T>` → `world-resource`

So the wasm ABI is intentionally small and generic, while the authored surface stays Rust/Bevy-like.

## 4.7 Inventory model

`src/authoring.rs` defines the inventory entries that hold authoring metadata:

- `WasvyModuleDeclaration`
- `WasvyModuleSystemRegistration`
- `WasvyModuleFirstLoadRegistration`

These inventory entries are the shared metadata layer between macro expansion and runtime/plugin assembly.

## 4.8 Pseudo-expanded macro example

The following is a **pseudo-expanded** sketch of what the system macro path is conceptually doing for a module like `combat`.
It is intentionally simplified, but it matches the architecture of the real expansion.

### Authored source

```rust
wasvy::module! {
    name: "combat"
}

#[wasvy::system(Update)]
fn tick(
    gate: Res<SimulationGate>,
    mut timeline: ResMut<SharedTimeline>,
    mut actors: Query<&mut Actor>,
) {
    if !gate.running {
        return;
    }

    timeline.frame += 1;

    for mut actor in &mut actors {
        actor.health -= 1;
    }
}
```

### Pseudo-expanded shape

```rust
pub const MODULE_NAME: &str = "combat";

#[derive(Default)]
pub struct NativeAdapterPlugin;

impl wasvy::authoring::Plugin for NativeAdapterPlugin {
    fn build(&self, app: &mut wasvy::authoring::App) {
        let scope = module_path!();
        for registration in wasvy::authoring::inventory::iter::<wasvy::authoring::WasvyModuleSystemRegistration> {
            if wasvy::authoring::module_scope_matches(scope, registration.scope) {
                (registration.register_native)(app);
            }
        }
    }
}

fn tick(
    gate: Res<SimulationGate>,
    mut timeline: ResMut<SharedTimeline>,
    mut actors: Query<&mut Actor>,
) {
    if !gate.running {
        return;
    }

    timeline.frame += 1;

    for mut actor in &mut actors {
        actor.health -= 1;
    }
}

const __WASVY_MODULE_SYSTEM_TYPES_tick: &[&str] = &[
    "game_api::SimulationGate",
    "game_api::SharedTimeline",
    "game_api::Actor",
];

fn __wasvy_register_module_system_tick(app: &mut wasvy::authoring::App) {
    app.add_systems(Update, tick);
}

#[cfg(target_arch = "wasm32")]
fn __wasvy_guest_impl_tick(
    gate: wasvy::module_guest::Res<SimulationGate, __wasvy_guest_bindings::wasvy::ecs::app::WorldResource>,
    mut timeline: wasvy::module_guest::ResMut<SharedTimeline, __wasvy_guest_bindings::wasvy::ecs::app::WorldResource>,
    mut actors: wasvy::module_guest::Query<&mut Actor, __wasvy_guest_bindings::wasvy::ecs::app::Query>,
) {
    if !gate.running {
        return;
    }

    timeline.frame += 1;

    for mut actor in &mut actors {
        actor.health -= 1;
    }
}

#[cfg(target_arch = "wasm32")]
fn __wasvy_guest_export_tick(
    __wasvy_arg_0: __wasvy_guest_bindings::wasvy::ecs::app::WorldResource,
    __wasvy_arg_1: __wasvy_guest_bindings::wasvy::ecs::app::WorldResource,
    __wasvy_arg_2: __wasvy_guest_bindings::wasvy::ecs::app::Query,
) {
    let gate = wasvy::module_guest::Res::new(__wasvy_arg_0);
    let timeline = wasvy::module_guest::ResMut::new(__wasvy_arg_1);
    let actors = wasvy::module_guest::Query::new(__wasvy_arg_2);
    __wasvy_guest_impl_tick(gate, timeline, actors);
}

wasvy::__wasvy_submit_module_system_registration!(
    wasvy::authoring::WasvyModuleSystemRegistration {
        scope: module_path!(),
        export_name: "tick",
        register_native: __wasvy_register_module_system_tick,
        referenced_types: __WASVY_MODULE_SYSTEM_TYPES_tick,
    }
);

#[cfg(target_arch = "wasm32")]
mod __wasvy_guest_bindings {
    wit_bindgen::generate!({ /* generated guest world */ });
}

#[cfg(target_arch = "wasm32")]
impl __wasvy_guest_bindings::Guest for __WasvyModuleGuest {
    fn register(app: App) {
        let system = System::new("tick");
        system.add_res(<SimulationGate as TypePath>::type_path());
        system.add_res_mut(<SharedTimeline as TypePath>::type_path());
        system.add_query(&[
            QueryFor::Mut(<Actor as TypePath>::type_path().to_string())
        ]);
        app.add_systems(&Schedule::Update, &[&system]);
    }

    fn tick(arg0: WorldResource, arg1: WorldResource, arg2: Query) {
        __wasvy_guest_export_tick(arg0, arg1, arg2);
    }
}
```

### What this example is meant to show

The authored function is not directly exported to wasm as-is.
Instead, macro expansion splits concerns into layers:

1. the original authored function remains the native registration target
2. metadata is emitted for inventory/runtime planning
3. a wasm-only guest implementation reuses the same body with guest wrapper params
4. a raw export shim converts WIT handles into wrapper values
5. `register(app)` separately describes the system signature to the host

That is the heart of the dual-target design.

---

## 5. Native execution path

### Main files
- `crates/wasvy_macros/src/lib.rs`
- `src/authoring.rs`
- `examples/modules/two_modules_workspace/crates/game_host/src/main.rs`

In native mode, the host app adds the generated adapter plugins directly:

- `CombatNativeAdapterPlugin`
- `AiNativeAdapterPlugin`

Those plugins register the authored systems and first-load functions straight into the host app.

### Important property

Native mode does **not** use separate native-only gameplay logic.
It executes the same authored module functions.

The only difference from guest mode is the execution substrate:

- native: direct in-process Bevy registration
- guest: wasm guest export calls through the host runtime

This keeps native mode a real fallback/debug path instead of a separate implementation.

---

## 6. Guest execution path and parameter translation

### Main files
- `crates/wasvy_macros/src/lib.rs`
- `src/module_guest.rs`
- `wit/wasvy-modules.wit`
- `wit/wasvy-ecs.wit`
- `src/asset.rs`
- `src/system.rs`
- `src/query.rs`
- `src/resource.rs`
- `src/runner.rs`
- `src/host/*`

The core guest-side idea is:

> the wasm component does not receive real Bevy ECS params directly; it receives host resources/handles that are wrapped into Wasvy guest shims, and those shims translate reads/writes back into the host world.

## 6.1 Two guest phases: registration and execution

The guest contract is split into two phases:

### Phase A: registration (`register`)
The host calls the guest module’s `register(app)` export.

In that phase, the guest does **not** run gameplay logic. It only describes systems to the host.

For each authored system, generated code does the equivalent of:

```rust
let system = System::new("tick");
system.add_res_mut("game_api::SharedTimeline");
system.add_query(&[QueryFor::Mut("game_api::Actor")]);
app.add_systems(Schedule::Update, &[&system]);
```

This records:

- exported function name
- schedule
- param kinds
- query shapes
- resource type paths

The host stores that as runtime `Param` metadata and later builds actual dynamic Bevy systems from it.

### Phase B: execution (`on-first-load` and system exports)
Later, when the module is initialized or a planned system runs, the host calls the exported wasm function by name:

- `on-first-load`
- `tick`
- etc.

At that point the host creates concrete WIT resource handles for the current invocation and passes them into the guest export shim.

## 6.2 Host-side planning flow for `register`

When a module artifact is loaded, `ModAsset::plan_systems(...)` in `src/asset.rs`:

1. instantiates the wasm component
2. creates a host `WasmApp` resource handle
3. runs the guest `register` export (falling back to legacy `setup` when needed)
4. collects the guest-declared systems into `AddSystems`
5. converts them into `PlannedSystems`

During that `register` call, the host is in `Runner::Config::Setup` mode.
In this mode, host implementations like `HostSystem for WasmHost` accept calls such as:

- `System::new(name)`
- `system.add_commands()`
- `system.add_query(...)`
- `system.add_res(type_path)`
- `system.add_res_mut(type_path)`

Those calls become runtime `Param` values that the host later uses to construct the actual dynamic system.

## 6.3 How a dynamic guest-backed system is built on the host

After registration, `src/system.rs` turns the planned metadata into a real Bevy system.

For each guest-declared param, the runtime creates a `BuiltParam`:

- `Commands`
- `Query(QueryId)`
- `Resource(ResourceId)`

Then for each invocation of the dynamic system, `initialize_params(...)` creates the concrete WIT resources passed into the guest function:

- `Commands` → `WasmCommands`
- `Query` → `WasmQuery`
- `Res` / `ResMut` → `WasmResource`

These are stored in the wasmtime resource table and passed to the guest as `Val::Resource(...)` values.

So the guest never receives raw Rust ECS references. It receives **resource handles into the host bridge layer**.

## 6.4 Guest shim layer (`src/module_guest.rs`)

`src/module_guest.rs` defines the wrapper types that make authored wasm logic feel Bevy-like:

- `Commands<B>`
- `Res<T, B>`
- `ResMut<T, B>`
- `Query<T, B>`
- `GuestRef<T, B>`
- `GuestMut<T, B>`

The generic `B` is the raw binding type generated by `wit_bindgen`.

The macros generate glue so these traits are implemented for the actual generated WIT binding types:

- `GuestCommandsBinding` for generated `Commands`
- `GuestWorldResourceBinding` for generated `WorldResource`
- `GuestComponentBinding` for generated `Component`
- `GuestQueryBinding` for generated `Query`
- `GuestQueryResultBinding` for generated `QueryResult`

That is what lets the generic shim layer wrap the concrete generated bindings.

## 6.5 Translation of each authored param kind

## `Commands`

### Guest-facing authored type
```rust
fn init(mut commands: Commands)
```

### Raw wasm export type
```text
commands
```

### Generated guest wrapper path
The export shim does:

```rust
let commands = wasvy::module_guest::Commands::new(raw_commands);
```

### Read/write behavior
`Commands::insert_resource<T>(value)`:

1. uses `T::type_path()`
2. serializes `value` to JSON bytes
3. calls the host binding `insert_resource(type_path, bytes)`

On the host side, `src/host/commands.rs` receives that and queues a command into the host Bevy `Commands` queue.

So `Commands` is not direct world access. It is a command bridge into the host queue.

## `Res<T>`

### Guest-facing authored type
```rust
fn tick(gate: Res<SimulationGate>)
```

### Raw wasm export type
```text
world-resource
```

### Generated guest wrapper path
The export shim does:

```rust
let gate = wasvy::module_guest::Res::new(raw_resource);
```

### Read behavior
`Res::new(...)` immediately:

1. calls `raw_resource.get()` through the WIT binding
2. receives serialized bytes from the host
3. deserializes those bytes into `T`
4. stores the owned `T` inside the guest wrapper

So guest `Res<T>` is currently a **deserialized snapshot value**, not a borrowed host reference.

On the host side, `src/host/resource.rs` + `src/resource.rs`:

1. resolve the resource by stable type path
2. encode the current host resource value
3. return bytes to the guest

## `ResMut<T>`

### Guest-facing authored type
```rust
fn tick(mut timeline: ResMut<SharedTimeline>)
```

### Raw wasm export type
```text
world-resource
```

### Generated guest wrapper path
The export shim does:

```rust
let timeline = wasvy::module_guest::ResMut::new(raw_resource);
```

### Read/write behavior
`ResMut::new(...)`:

1. calls host `get()`
2. deserializes bytes into owned `T`
3. exposes `DerefMut` to authored logic

When the wrapper is dropped, `Drop for ResMut<T, _>`:

1. serializes the mutated `T` back to JSON bytes
2. calls host `set(bytes)`

On the host side, `HostWorldResource::set` resolves the resource and applies the new value.

For known reflected resources, the host uses reflection/codec support to apply the updated value back into the real Bevy resource.
For guest-private resource types, the runtime can store opaque serialized blobs in `WasmResourceValue`.

## `Query<&T>` and `Query<&mut T>`

### Guest-facing authored type
```rust
fn tick(mut actors: Query<&mut Actor>)
```

### Raw wasm export type
```text
query
```

### Registration-time metadata
During `register`, the generated code declares the query shape using `QueryFor` values built from `TypePath`:

```rust
system.add_query(&[
    QueryFor::Mut(<Actor as TypePath>::type_path().to_string())
]);
```

That is how the host knows what component/filter set this guest query means.

### Host-side query planning
`src/query.rs` turns those `QueryFor` values into:

- a real Bevy query builder used by the dynamic host system
- a `QueryResolver` that maps guest component index → concrete host component type

### Guest iteration flow
The guest wrapper for `Query<&mut Actor>` does not hold a whole collection. It holds a raw query handle.

When authored code does:

```rust
for mut actor in &mut actors {
    actor.health -= 1;
}
```

the iterator does this repeatedly:

1. call host `query.iter()`
2. host returns an optional `query-result` handle for the next entity
3. wrapper asks that result for `component(0)`
4. host returns a `component` handle for that entity/component slot
5. guest creates `GuestMut<Actor, _>` from that handle
6. `GuestMut::new(...)` calls `component.get()` and deserializes `Actor`
7. authored code mutates the owned `Actor`
8. when `GuestMut` drops, it serializes the new `Actor` and calls `component.set(...)`
9. host applies the new component value to that entity

For `Query<&T>`, the flow is the same except the wrapper uses `GuestRef` and never calls `set(...)` on drop.

### Important detail: component indexes
The host query result returns components by **index**, not by type path, during execution.
That index order is defined by the order declared during registration.
Filters like `With<T>` and `Without<T>` affect matching, but they do not consume component indexes.

## 6.6 Example end-to-end translation of one authored system

Take this guest-authored system:

```rust
#[wasvy::system(Update)]
fn tick(
    gate: Res<SimulationGate>,
    mut timeline: ResMut<SharedTimeline>,
    mut actors: Query<&mut Actor>,
) {
    if !gate.running {
        return;
    }

    timeline.frame += 1;

    for mut actor in &mut actors {
        actor.health -= 1;
    }
}
```

### Compile-time / macro phase
The macros derive:

- native registration metadata
- referenced type paths:
  - `SimulationGate`
  - `SharedTimeline`
  - `Actor`
- guest WIT export signature:
  - `tick(arg0: world-resource, arg1: world-resource, arg2: query)`
- guest registration code:
  - `add_res("SimulationGate")`
  - `add_res_mut("SharedTimeline")`
  - `add_query([Mut("Actor")])`
- guest export shim
- guest wrapper-typed impl function

### Registration-time runtime phase
The host calls `register(app)`.
The generated guest code creates a `System::new("tick")`, attaches those params, and adds it to `Update`.
The host captures that as a planned dynamic system.

### Execution-time runtime phase
When the dynamic host system runs:

1. the host creates WIT resources:
   - `WasmResource(id_gate)`
   - `WasmResource(id_timeline)`
   - `WasmQuery(id_actors)`
2. those handles are passed into the wasm export `tick(...)`
3. the generated export shim wraps them into:
   - `Res<SimulationGate, _>`
   - `ResMut<SharedTimeline, _>`
   - `Query<&mut Actor, _>`
4. the guest impl executes the authored body
5. reads happen through `get()` + deserialize
6. writes happen through `set()` on wrapper drop or queued commands
7. the host applies those changes to the real shared Bevy world

That is the full translation chain from authored Bevy-looking system to host-managed guest execution.

## 6.7 Pseudo sequence diagram for guest system execution

The following is a **pseudo sequence diagram**. It is intentionally simplified and is meant to explain the runtime flow, not mirror every internal call exactly.

```text
Participant HostDynamicSystem
Participant Runner/Wasmtime
Participant GuestExportShim
Participant GuestWrapper
Participant HostBridge
Participant BevyWorld

HostDynamicSystem -> Runner/Wasmtime: instantiate component + call exported wasm function `tick`
HostDynamicSystem -> Runner/Wasmtime: pass WIT resources
note over HostDynamicSystem,Runner/Wasmtime: e.g. WorldResource(gate), WorldResource(timeline), Query(actors)

Runner/Wasmtime -> GuestExportShim: tick(raw_gate, raw_timeline, raw_query)
GuestExportShim -> GuestWrapper: Res::new(raw_gate)
GuestExportShim -> GuestWrapper: ResMut::new(raw_timeline)
GuestExportShim -> GuestWrapper: Query::new(raw_query)
GuestExportShim -> GuestWrapper: call __wasvy_guest_impl_tick(...)

GuestWrapper -> HostBridge: raw_gate.get()
HostBridge -> BevyWorld: read SimulationGate resource
BevyWorld -> HostBridge: current value
HostBridge -> GuestWrapper: serialized bytes
GuestWrapper -> GuestWrapper: deserialize SimulationGate

GuestWrapper -> HostBridge: raw_timeline.get()
HostBridge -> BevyWorld: read SharedTimeline resource
BevyWorld -> HostBridge: current value
HostBridge -> GuestWrapper: serialized bytes
GuestWrapper -> GuestWrapper: deserialize SharedTimeline

loop for each query iteration
  GuestWrapper -> HostBridge: raw_query.iter()
  HostBridge -> BevyWorld: advance planned query cursor
  BevyWorld -> HostBridge: next entity / none
  HostBridge -> GuestWrapper: query-result handle

  GuestWrapper -> HostBridge: query_result.component(0)
  HostBridge -> BevyWorld: resolve Actor component for entity
  BevyWorld -> HostBridge: component handle backing entity+slot
  HostBridge -> GuestWrapper: component handle

  GuestWrapper -> HostBridge: component.get()
  HostBridge -> BevyWorld: read Actor component
  BevyWorld -> HostBridge: current Actor value
  HostBridge -> GuestWrapper: serialized bytes
  GuestWrapper -> GuestWrapper: deserialize Actor

  GuestWrapper -> GuestWrapper: authored logic mutates Actor
  GuestWrapper -> HostBridge: component.set(serialized mutated Actor)
  HostBridge -> BevyWorld: apply updated Actor component
end

GuestWrapper -> GuestWrapper: authored logic mutates SharedTimeline
GuestWrapper -> HostBridge: raw_timeline.set(serialized mutated timeline)
note over GuestWrapper,HostBridge: happens on ResMut drop
HostBridge -> BevyWorld: apply updated SharedTimeline resource

Runner/Wasmtime -> HostDynamicSystem: wasm call returns
HostDynamicSystem -> BevyWorld: continue frame with updated shared world state
```

### What to notice in the sequence

- the guest does not hold live Bevy references
- every guest param is backed by a host resource handle
- reads cross the bridge via `get()`
- mutations cross the bridge via `set()` or queued commands
- `ResMut<T>` and `Query<&mut T>` write back when their wrapper values drop
- the host remains the sole owner of the real Bevy world

## 6.8 Why this is the key architectural move

This bridge is what makes the single-crate model possible.

Without it, the module author would need to write guest-specific binding code or avoid Bevy-like authoring entirely.
With it, the author writes normal Wasvy Module functions once, and generated guest wrappers plus host runtime plumbing do the translation.

---

## 7. Manifest-driven module identity

### Main files
- `src/workspace.rs`
- `examples/modules/two_modules_workspace/wasvy.toml`

`wasvy.toml` is the source of truth for two separate concepts:

## 7.1 Workspace inventory

Which modules exist in the workspace.

Example:

```toml
[[module]]
name = "combat"
path = "crates/modules/combat"
```

This maps stable module name → module crate path.

## 7.2 World composition

Which modules are active in a specific world.

Example:

```toml
[world]
modules = ["combat", "ai"]
```

This maps world → active module ids.

## 7.3 Parsed host resources

`src/workspace.rs` parses the manifest into:

- `WorkspaceManifest`
- `WorkspaceInventory`
- `WorldComposition`

These become host resources used by the workspace plugin and dev tooling.

## 7.4 Important invariant

Runtime activation is based on **module name**, not crate path and not manual asset strings in app code.

That is why the runtime can consistently treat `combat` as the module identity even though Cargo package names, lib names, and filesystem paths may differ.

---

## 8. Host workspace plugin

### Main file
- `src/module_plugin.rs`

`WasvyWorkspacePlugin` is the host-side entry point for Wasvy Modules.

On native targets it does two jobs:

## 8.1 Install shared runtime substrate

It initializes the shared machinery required for guest module loading and execution, including:

- `ModAsset` and `ModAssetLoader`
- `Engine`
- linker via `create_linker`
- `CodecResource`
- `WasmComponentRegistry`
- `WasmResourceRegistry`
- `AppTypeRegistry`
- `ModSchedules`
- `ModStartup`
- `ModuleGenerationCounter`
- `ModuleReloadQueue`
- module reload systems
- `AutoRegistrationPlugin`
- `Sandboxed` component registration

This is the substrate that guest modules rely on.

## 8.2 Seed workspace/module configuration

It reads `wasvy.toml` and inserts:

- `WorkspaceConfigPath`
- `WorkspaceInventory`
- `WorldComposition`

If explicit modules are requested, it uses those; otherwise it uses the default world from the manifest.

## 8.3 Auto-spawn guest modules

By default, the plugin adds a startup system that spawns guest modules for every active module id in `WorldComposition`.

Artifact path convention:

```text
assets/modules/{module-name}.wasm
```

This is important: the host app no longer needs to manually spawn modules by wasm path.

## 8.4 Native mode behavior

In native mode, the host uses:

```rust
WasvyWorkspacePlugin::new(manifest).without_auto_spawn()
```

So the plugin still:

- parses the manifest
- seeds inventory/composition
- installs substrate/resources

but it does **not** auto-load guest artifacts. The app then adds the generated native adapter plugins instead.

---

## 9. CLI architecture (`wasvy dev`)

### Main files
- `crates/wasvy_cli/src/main.rs`
- `crates/wasvy_cli/src/dev.rs`

The CLI owns the development loop for guest mode.

## 9.1 Session loading

`load_dev_session(...)`:

- resolves the manifest path
- parses the workspace manifest
- finds the host manifest
- computes build specs for each active module in the world composition

## 9.2 Module build specs

For each active module, the CLI derives:

- `id`: stable module id, e.g. `combat`
- `package_name`: Cargo package name, e.g. `two_modules_combat`
- `artifact_stem`: built library stem, e.g. `combat`
- `built_wasm`: `target/wasm32-wasip2/debug/{artifact_stem}.wasm`
- `staged_wasm`: `assets/modules/{module-id}.wasm`

This deliberately separates:

- Cargo build identity
- final runtime activation identity

## 9.3 Build/stage flow

`run_dev(...)` in guest mode:

1. builds active module crates for `wasm32-wasip2`
2. stages each resulting artifact into `assets/modules/{module-name}.wasm`
3. starts the host app
4. watches for file changes
5. rebuilds changed guest modules
6. atomically restages artifacts
7. waits for host-side module swap completion

That is the ergonomic path intended for module development.

## 9.4 Native dev mode

`wasvy dev --native` skips guest artifact staging and restarts the host in native mode when relevant code changes.

---

## 10. Example workspace structure

### Main files
- `examples/modules/two_modules_workspace/wasvy.toml`
- `examples/modules/two_modules_workspace/crates/game_api/src/lib.rs`
- `examples/modules/two_modules_workspace/crates/modules/combat/src/lib.rs`
- `examples/modules/two_modules_workspace/crates/modules/ai/src/lib.rs`
- `examples/modules/two_modules_workspace/crates/game_host/src/main.rs`

## 10.1 `game_api`

This crate contains shared types used by the host and modules.

Examples:
- `Actor`
- `SharedTimeline`
- `SimulationGate`

These types carry the traits needed for the bridge/runtime model, such as serialization, reflection, resource/component metadata, and type identity.

## 10.2 Module crates

Each module crate is dual-target:

```toml
[lib]
crate-type = ["rlib", "cdylib"]
```

That single crate is the source for:

- native plugin registration
- guest wasm artifact generation

## 10.3 Host crate

The host app:

- builds a Bevy app
- installs `WasvyWorkspacePlugin`
- registers shared types
- uses guest mode by default
- switches to native mode with `--native`

In native mode it adds the generated adapter plugins.
In guest mode it waits until manifest-selected guest modules are loaded and active.

## 10.4 `SimulationGate`

The example includes a small `SimulationGate` resource so both modes can begin ticking after initialization is complete. That keeps the example output aligned between guest and native runs.

---

## 11. End-to-end runtime flow

## Guest mode

```text
wasvy dev
  → parse wasvy.toml
  → discover active modules in world composition
  → build each active module crate to wasm
  → stage assets/modules/combat.wasm, ai.wasm
  → launch host
  → WasvyWorkspacePlugin parses manifest and installs runtime substrate
  → plugin auto-spawns modules by stable module id
  → guest register() export describes planned systems
  → first-load runs once on first activation
  → guest systems execute against the shared host world
  → file changes rebuild/stage/swap module artifacts
```

## Native mode

```text
host --native
  → WasvyWorkspacePlugin parses manifest and installs shared substrate/resources
  → guest auto-spawn disabled
  → host adds generated NativeAdapterPlugins
  → first-load runs once
  → systems execute directly in the host process
```

---

## 12. Key invariants of the design

These are the most important architectural rules to keep in mind:

### 1. One module crate per module
No parallel guest crate.

### 2. One authored logic source
No native/guest gameplay duplication.

### 3. Stable module identity lives in `wasvy::module!` and `wasvy.toml`
Not in Cargo package names or asset strings.

### 4. Guest activation comes from world composition
Host app code should not manually wire module artifact paths as the composition model.

### 5. Tooling owns guest build/staging workflow
`wasvy dev` is the intended developer entry point.

### 6. Native and guest should stay behaviorally close
They are two execution modes for the same authored module, not two separate products.

---

## 13. Current implementation details worth knowing

### Wasm-safe crate surface
`src/lib.rs`, `src/authoring.rs`, and `src/module_plugin.rs` are target-gated so module crates can depend on `wasvy` when building to wasm without dragging in host-only runtime code.

### Guest source discovery
The macro implementation currently scans the module crate source file (`src/lib.rs` or `src/main.rs`) to discover top-level `#[wasvy::system]` and `#[wasvy::on_first_load]` functions for guest export generation.

### Shared type requirements
Types that cross the guest boundary generally need traits such as:

- `Serialize`
- `Deserialize`
- `TypePath`
- reflect/resource/component traits when relevant

### File-based staged artifact convention
The current runtime/dev flow still stages guest wasm files into:

```text
assets/modules/{module-name}.wasm
```

That is now a tooling/runtime convention, not an app-level authoring concern.

---

## 14. File map

### Core architecture
- `crates/wasvy_macros/src/lib.rs`
- `src/authoring.rs`
- `src/module_guest.rs`
- `src/module_plugin.rs`
- `src/workspace.rs`
- `crates/wasvy_cli/src/dev.rs`
- `crates/wasvy_cli/src/main.rs`

### Example workspace
- `examples/modules/two_modules_workspace/wasvy.toml`
- `examples/modules/two_modules_workspace/crates/game_api/src/lib.rs`
- `examples/modules/two_modules_workspace/crates/modules/combat/src/lib.rs`
- `examples/modules/two_modules_workspace/crates/modules/ai/src/lib.rs`
- `examples/modules/two_modules_workspace/crates/game_host/src/main.rs`

---

## 15. Short summary

This branch implements Wasvy Modules as a **single-crate dual-target architecture**:

- each module is authored once in one crate
- that crate builds for native and guest modes
- Wasvy macros generate both the native adapter path and the guest export path
- `wasvy.toml` selects modules by stable identity
- `WasvyWorkspacePlugin` activates manifest-selected modules
- `wasvy dev` owns build/stage/watch/reload workflow for guest mode

That is the final idea behind the implementation, and it is the lens the branch should be understood through.
