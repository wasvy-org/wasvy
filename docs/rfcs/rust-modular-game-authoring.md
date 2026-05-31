# RFC: Wasvy Modules

Status: Draft  
Audience: Wasvy core maintainers  
Primary use case: Rust game developers splitting a single game into multiple hot-reloadable gameplay modules

Wasvy should expose a distinct product surface called **Wasvy Modules** for internal modular game development.
A **Module** is not a **Mod**. A Module is an internal gameplay unit authored by the game team, identified by a durable stable name, authored at the crate root of a pure **Module Crate**, runnable in native or guest mode, and hot reloadable with preserve-state semantics.

## Summary

This RFC defines the first-class authoring model for **Wasvy Modules**.

The goal is to let a game developer write gameplay code that feels close to normal Bevy code while Wasvy handles:

- guest registration generation
- guest bindings and WIT generation
- query/resource metadata generation
- wasm build/output plumbing
- preserve-state hot reload
- native/guest dual-mode execution

The central idea is:

> A developer writes one pure Rust gameplay crate per Module, and Wasvy turns it into a runtime-loadable gameplay unit with strong Rust DX.

## Product stance

This RFC is explicitly about **Wasvy Modules**, not the generic public multi-language **Mod** workflow.

This product surface is allowed to be:

- Rust-first
- macro-heavy
- tightly integrated with a shared Rust API crate
- more ergonomic than the portable public modding surface

The portable guest ABI remains important, but it is not the primary authoring surface here.

## Goals

1. Make internal modular game development feel close to normal Bevy gameplay authoring.
2. Eliminate handwritten WIT and manual guest registration for internal Rust Modules.
3. Allow the same Module source to run in both native and guest mode.
4. Support preserve-state reload by replacing systems while keeping host world state alive.
5. Let developers organize a game as a workspace of runtime-loadable Module Crates.
6. Define an explicit **Module Authoring Contract** for supported syntax, params, and semantics.
7. Make **Dual-mode Equivalence** an explicit product goal.

## Non-goals

1. Perfect parity with every Bevy system param and lifecycle feature in MVP.
2. Solving the public multi-language Mod authoring UX in this RFC.
3. Preserving guest-local runtime memory across reloads.
4. Defining module-level semantic schedule phases in MVP.
5. Introducing multiple activation surfaces per Module.
6. Introducing multi-world runtime topology for Module composition.

## Core terminology

### Module
An internal gameplay unit authored by the game team that has a single activation surface, is identified by a durable stable name, can run natively or as a guest, and can be hot reloaded while preserving host world state.

### Mod
An externally authored add-on package loaded through Wasvy's public modding workflow.

### Module Crate
A pure Rust gameplay crate that defines exactly one Module, serves as its build and reload boundary, and is the primary authoring unit for that Module.

### Module Declaration
The explicit crate-root macro declaration that defines crate-level Module metadata beyond what system annotations can infer. In MVP its only required field is the durable stable Module name.

### Registration
The metadata-only phase where a Module declares its runtime systems and scheduling and which reruns on reload.

### First-load Initialization
A world-mutating phase that runs once when a Module first activates in a given world and does not rerun on hot reload.

### Reload Compatibility Failure
A failed Module reload caused by incompatible persisted state, where Wasvy keeps the old module active and reports that a relaunch is required to run the new code, ideally with a best-effort list of incompatible schema changes.

### Shared API Crate
The authoritative crate for gameplay types shared between the host and more than one Module, including shared components and resources and later shared messages.

### Module-private Type
A gameplay type used only within one Module, owned by that Module Crate, and allowed to persist in world state across reloads as first-class private component or resource state.

### Native Adapter Plugin
A generated Bevy plugin used to run a Module in native mode.

### Workspace Inventory
The declared set of available Modules in a game workspace.

### World Composition
The host-side selection of which available Modules are activated in a single shared host world, optionally seeded by configuration defaults or profiles.

### Dual-mode Equivalence
The product goal that a Module behaves as similarly as possible in native mode and guest mode, with any known differences treated as explicit gaps rather than the intended model.

### Module Authoring Contract
The explicit documented set of syntax, params, and semantics that Wasvy Modules support.

## Architectural decisions

## 1. Module and Mod are distinct concepts

Wasvy must stop using one term for both internal runtime-loaded gameplay units and external extensions.

- **Module** = internal Rust gameplay unit
- **Mod** = external/public extension package

This distinction should appear in docs, APIs, examples, and runtime concepts.

## 2. One Module per Module Crate

A Module maps 1:1 to a Module Crate.

This means:

- one Module Crate defines exactly one Module
- one Module Crate is one build boundary
- one Module Crate is one reload boundary
- one Module has one activation surface

A Module Crate may internally contain many normal Rust `mod`s, but those are source structure only, not runtime Modules.

## 3. Module Crates are pure gameplay crates

A Module Crate contains gameplay/runtime logic only and excludes host-only integration code.

Host-only concerns belong in:

- the host crate
- the Shared API Crate
- future explicit support crates if needed

This keeps dual-mode authoring honest and reduces accidental coupling to host-only APIs.

## 4. Module identity is explicit and durable

A Module is identified by a durable stable name declared explicitly at crate root.
This identity is distinct from Cargo package name, crate name, or filesystem path.

Changing the declared Module name is a meaningful identity change.

## 5. Wasvy Modules is a distinct product surface

The Rust modular-game workflow is not “the nice path through mods”.
It is a separate named product surface: **Wasvy Modules**.

This should be reflected in:

- docs
- API naming
- examples
- runtime concepts
- future code organization

## Workspace model

## Workspace Inventory vs World Composition

Wasvy should distinguish discovery from activation.

### Workspace Inventory
Defines what Modules exist in a workspace.

### World Composition
Defines which Modules from the Workspace Inventory are active in a given host world.

Configuration may seed defaults or profiles, but host code owns the final active World Composition.

## Recommended config shape

```toml
[workspace]
host = "crates/game_host"
api = "crates/game_api"

[[module]]
name = "combat"
path = "crates/modules/combat"

[[module]]
name = "ai"
path = "crates/modules/ai"
```

This config defines the **Workspace Inventory**, not necessarily the active World Composition.

## Host composition

World Composition should reference Modules by stable Module name only.
It should not use package names, crate names, or filesystem paths as fallback identifiers.

Conceptual host API:

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(GameApiPlugin)
    .add_plugins(WasvyWorkspacePlugin::new("wasvy.toml").with_modules([
        "combat",
        "ai",
    ]))
    .run();
```

A future profile-driven configuration layer is compatible with this as long as host code owns the final World Composition.

## Authoring model

## Crate-root Module Declaration

The canonical Module Declaration syntax is a crate-root macro:

```rust
wasvy::module! {
    name: "combat"
}
```

MVP required field:

- `name`

This macro form is preferred over a crate attribute because it is explicit, evolvable, and well-suited to future crate-level metadata.

## System declaration syntax

The canonical system declaration syntax is:

```rust
#[wasvy::system(Update)]
fn regen(...) { ... }
```

This avoids namespace ambiguity and makes it explicit that the function participates in the Wasvy Modules authoring surface.

## First-load Initialization syntax

The canonical syntax for one-time initialization is:

```rust
#[wasvy::on_first_load]
fn init(...) { ... }
```

This should be named explicitly after the agreed lifecycle concept, not hidden behind a vague `init` term.

## Example Module Crate

```rust
use bevy::prelude::*;
use game_api::prelude::*;

wasvy::module! {
    name: "combat"
}

#[wasvy::on_first_load]
fn init(mut commands: Commands) {
    commands.insert_resource(CombatRuntimeState::default());
}

#[wasvy::system(Update)]
fn regen(mut q: Query<&mut Health>) {
    for mut health in &mut q {
        health.current = (health.current + 1.0).min(health.max);
    }
}

#[wasvy::system(Update)]
fn despawn_dead(mut commands: Commands, q: Query<(Entity, &Health)>) {
    for (entity, health) in &q {
        if health.current <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
```

The author should not manually write:

- guest `setup()` / registration functions
- WIT worlds
- query metadata
- component type path strings
- serialization glue for ordinary component/resource access

## Shared vs private state

## Shared API Crate rule

If a gameplay type is used by:

- the host, or
- more than one Module

it belongs in the **Shared API Crate**.

Examples:

- `Health`
- `Faction`
- `TransformLikeSharedState`
- shared resources such as `GameRules`

## Module-private Type rule

If a gameplay type is used only within one Module, it may live in that Module Crate as a **Module-private Type**.

Examples:

- `CombatCooldown`
- `ComboWindow`
- `CombatRuntimeState`

Module-private Types are first-class state. They may be stored as:

- private components
- private resources

In MVP, “Module-private” is an architectural ownership concept, not yet a hard-enforced runtime boundary.

## Lifecycle model

## Registration

Registration is metadata-only and reruns on reload.

Registration may declare:

- systems
- schedules
- ordering metadata
- param/query/resource metadata

Registration must not mutate gameplay world state.

This distinction is necessary for preserve-state hot reload.

## First-load Initialization

First-load Initialization:

- runs once when the Module first activates in a given world
- may mutate world state
- does not rerun on hot reload
- has no inter-Module ordering guarantees in MVP

This is not guest setup/registration.
It is a separate lifecycle concept.

### Lifetime semantics

First-load Initialization means:

- restart the game -> runs again
- create a new world -> runs again
- hot reload in the same world -> does not run again

## No inter-Module initialization ordering in MVP

Wasvy Modules should not provide bootstrap-order orchestration between Modules in MVP.

If module initialization ordering matters, world composition or host composition must own it externally.

This avoids introducing a second orchestration system inside the Module authoring model.

## Guest runtime semantics

Wasvy Modules may mirror Bevy syntax, but guest-mode semantics are not identical to native Bevy internals.

### Important truth

A guest-mode `Query<&mut Health>` is not a real cross-ABI host borrow.

Conceptually, guest-mode execution works like:

1. host finds matching entities
2. host serializes component/resource values
3. guest deserializes them into Rust values
4. guest mutates those values
5. host writes modifications back

The promise is:

- Bevy-like Rust authoring
- explicit and tested semantics
- not accidental equivalence to every Bevy internal mechanism

This is why the supported surface must be deliberate.

## Module Authoring Contract

MVP must define an explicit **Module Authoring Contract**.
Supported syntax and semantics are a documented contract, not an implementation accident.

### Design rule

Wasvy should not claim support for all Bevy signatures just because some syntax is parseable.
It should support a curated subset whose guest semantics are explicitly understood and compile-time validated.

## MVP supported subset

### Supported in MVP

- `Commands`
- `Query<&T>`
- `Query<&mut T>`
- `Query<(Entity, &T)>`
- `Query<(Entity, &mut T)>`
- tuples of supported query items
- `With<T>`
- `Without<T>`
- `Res<T>`
- `ResMut<T>`
- Bevy schedules directly (`Update`, `FixedUpdate`, `PreUpdate`, `PostUpdate`, etc.)
- `#[wasvy::on_first_load]`

### Deferred from MVP

- messages/events
- `Local<T>`
- `Changed<T>` / `Added<T>` / removed-component tracking parity
- exotic Bevy system params
- Wasvy-defined semantic phases

### Validation rule

Unsupported signatures should fail at compile time with explicit diagnostics.
They should not drift into obscure runtime or codegen failures.

## Reload model

## Preserve-state reload

The default reload model for Wasvy Modules is:

> Preserve world state, replace module code.

On reload:

1. keep the host ECS world intact
2. load the rebuilt guest artifact
3. rerun Registration in planning mode
4. if successful, remove old module systems and add new ones
5. continue running against the same world state

### Preserved across reload

- entities
- components
- resources
- Module-private Types stored in world state
- Shared API Crate types stored in world state

### Not preserved across reload

- guest-local runtime memory
- `Local<T>`
- guest static caches
- anything not stored in host world state

## Reload Compatibility Failure

If persisted Module-private or shared state is incompatible with the new Module code, Wasvy should:

1. keep the previous Module generation active
2. refuse activation of the new generation
3. emit a clear error
4. instruct the developer to relaunch the game to run the latest code

### Required error quality

Wasvy should provide best-effort field-level schema change reporting and fall back to type-level reporting when exact diffs are unavailable.

Ideal error style:

- Reload blocked for module `combat`
- Relaunch required to run latest code
- Incompatible persisted state:
  - `combat::CombatCooldown`: added field `source: AttackId`
  - `combat::ComboWindow`: removed field `legacy_end`

This behavior must be transactional and explicit. Silent state dropping is not the default.

## Native mode vs guest mode

## Native Adapter Plugin

A Module is not itself a Bevy Plugin.
For native mode, Wasvy generates a **Native Adapter Plugin** that runs the Module in-process.

Conceptually:

```rust
combat::NativeAdapterPlugin
```

This is an implementation artifact, not the primary domain concept.

## Dual-mode Equivalence

Native-mode and guest-mode behavioral equivalence is an explicit product goal.

Meaning:

- differences should be minimized
- known divergences should be documented
- semantic gaps are product gaps, not the intended model

MVP may still have documented exceptions, but equivalence remains the north star.

## Default dev workflow

Guest mode is the default Wasvy Modules development workflow.
Native mode is the explicit fallback for debugging and testing.

Recommended command shape:

```bash
wasvy dev
```

Default behavior:

- run host in guest-mode modular workflow
- watch Module Crates
- rebuild changed guest artifacts
- hot reload Modules with preserve-state semantics

Explicit fallback:

```bash
wasvy dev --native
```

This should run the same Module source through generated Native Adapter Plugins.

## Tooling expectations

Wasvy tooling should own or hide:

- WIT generation
- guest registration generation
- guest export naming
- query/resource metadata derivation
- artifact naming/output placement
- watch/rebuild/reload flow

A game developer should not regularly interact with these implementation details during normal Wasvy Modules usage.

## Why not “all Bevy signatures” in MVP?

Syntactic acceptance is not the same as semantic support.
Many Bevy params encode runtime semantics that do not map trivially across the guest boundary.

Examples include:

- `Local<T>`
- change-tracking filters
- event semantics
- unusual system params
- exact host borrow behavior

Therefore Wasvy Modules should prefer:

- curated support
- compile-time validation
- explicit contract

over accidental broad syntax acceptance with surprising runtime behavior.

## Recommended MVP scope

### Product surface

- distinct named surface: **Wasvy Modules**
- clear separation from public **Mod** workflow

### Authoring

- crate-root `wasvy::module! { name: ... }`
- `#[wasvy::system(...)]`
- `#[wasvy::on_first_load]`
- one Module per pure Module Crate

### State model

- Shared API Crate for host-visible or cross-module types
- Module-private Types as first-class private components/resources
- preserve-state reload with compatibility checks

### Runtime

- metadata-only Registration
- First-load Initialization once per world activation
- transactional system swap on reload
- Reload Compatibility Failure keeps old Module active

### Tooling

- Workspace Inventory in config
- host-owned World Composition by Module name
- default guest-mode dev workflow
- native-mode fallback workflow

## Open follow-up questions

1. What exact compile-time diagnostics should Wasvy emit for unsupported Module Authoring Contract shapes?
2. How should additive-compatible schema changes be detected and surfaced?
3. When should explicit migration hooks be introduced after MVP?
4. How should native-mode test helpers expose World Composition ergonomically?
5. How should Wasvy Modules docs and APIs coexist with existing Mod/ModLoader naming in the codebase during transition?

## Conclusion

Wasvy should treat **Wasvy Modules** as a first-class Rust product surface for building games out of internal runtime-loadable gameplay units.

The correct mental model is not “mods, but nicer”.
It is:

> Write one pure Rust gameplay crate per Module, run it natively when you want, or load it as a hot-reloadable guest Module when you want, while Wasvy preserves world state and owns the runtime plumbing.
