# Wasvy

Wasvy is a framework for building Bevy games out of runtime-loadable units while also supporting externally authored extensions.

## Language

**Module**:
An internal gameplay unit authored by the game team that has a single activation surface, is identified by a durable stable name, can run natively or as a guest, and can be hot reloaded while preserving host world state.
_Avoid_: Mod, plugin

**Mod**:
An externally authored add-on package loaded through Wasvy's public modding workflow.
_Avoid_: Module

**Shared API Crate**:
The authoritative crate for gameplay types shared between the host and more than one **Module**, including shared components and resources and later shared messages.
_Avoid_: Common crate, shared types crate

**Module-private Type**:
A gameplay type used only within one **Module**, owned by that **Module Crate**, and allowed to persist in world state across reloads as first-class private component or resource state.
_Avoid_: Shared type

**Game**:
A Bevy application built with Wasvy that may be composed of internal runtime-loadable units and may also support external extensions.
_Avoid_: App

**Module Crate**:
A pure Rust gameplay crate that defines exactly one **Module**, serves as its build and reload boundary, and is the primary authoring unit for that **Module**.
_Avoid_: Module package

**Wasvy Modules**:
The distinct Wasvy product surface for building games out of internal Rust **Modules** with dual-mode execution and preserve-state hot reload.
_Avoid_: Generic modding workflow, nice mods path

**Native Adapter Plugin**:
A generated Bevy plugin used to run a **Module** in native mode.
_Avoid_: Module

**Dual-mode Equivalence**:
The product goal that a **Module** behaves as similarly as possible in native mode and guest mode, with any known differences treated as explicit gaps rather than the intended model.
_Avoid_: Approximate parity

**Module Authoring Contract**:
The explicit documented set of syntax, params, and semantics that **Wasvy Modules** support.
_Avoid_: Implicit support, accidental compatibility

**Workspace Inventory**:
The declared set of available **Modules** in a game workspace.
_Avoid_: Active modules, loaded modules

**World Composition**:
The host-side selection of which available **Modules** are activated in a single shared host world, optionally seeded by configuration defaults or profiles.
_Avoid_: Discovery, inventory

**Reload Compatibility Failure**:
A failed **Module** reload caused by incompatible persisted state, where Wasvy keeps the old module active and reports that a relaunch is required to run the new code, ideally with a best-effort list of incompatible schema changes.
_Avoid_: Soft reload failure, partial reload

**Registration**:
The metadata-only phase where a **Module** declares its runtime systems and scheduling and which reruns on reload.
_Avoid_: Setup, initialization

**Module Declaration**:
The explicit crate-root macro declaration that defines crate-level **Module** metadata beyond what system annotations can infer and whose MVP required field is the durable stable **Module** name.
_Avoid_: Implicit module metadata, crate-level attribute metadata

**First-load Initialization**:
A world-mutating phase that runs once when a **Module** first activates in a given world and does not rerun on hot reload.
_Avoid_: Setup, reload hook

## Relationships

- A **Game** may be composed of many **Modules**
- A **Module** is defined by exactly one **Module Crate**
- A **Module Crate** defines exactly one **Module**
- The primary authoring unit for a **Module** is the crate root of its **Module Crate**, not a nested Rust source module
- A **Module Declaration** lives at the crate root of a **Module Crate** and uses the canonical macro form
- Systems in **Wasvy Modules** use the canonical `#[wasvy::system(...)]` attribute form
- **First-load Initialization** in **Wasvy Modules** uses the canonical `#[wasvy::on_first_load]` attribute form
- The stable name declared in a **Module Declaration** is the durable runtime identity of the **Module** and is distinct from crate or package naming
- A **Workspace Inventory** lists the available **Modules** in a game workspace
- A **World Composition** selects which **Modules** from the **Workspace Inventory** are activated in a given world by stable **Module** name
- Configuration may provide default or profiled **World Composition** inputs, but host code owns the final active **World Composition**
- In the MVP authoring model, **Module** systems target Bevy schedules directly rather than Wasvy-defined semantic phases
- A **Module** has exactly one activation surface at runtime
- A **Module** is not itself a Bevy plugin, but it may be executed in native mode through a **Native Adapter Plugin**
- **Dual-mode Equivalence** is a product goal for **Module** authoring, even if MVP documents known gaps
- Guest mode is the default development workflow for modular games, while native mode is the explicit fallback for debugging and testing
- The Rust modular-game authoring model is a distinct Wasvy product surface named **Wasvy Modules** and is not described merely as the nice path through the generic **Mod** workflow
- A **Module Crate** contains gameplay/runtime logic only and excludes host-only integration code
- A gameplay type used by the host or by more than one **Module** belongs in the **Shared API Crate**
- A **Module-private Type** belongs to exactly one **Module Crate**
- A **Module-private Type** may persist in host world state across **Module** reloads, subject to schema compatibility rules
- **Module-private Types** may be stored as first-class private components or private resources
- In MVP, **Module-private** ownership is an architectural contract rather than a hard-enforced runtime boundary
- In MVP, **Module** authoring supports shared and private resource access for supported types
- Messages/events are deferred from the MVP **Module** authoring model until systems, queries, and resources are solid
- MVP **Module** authoring supports a curated, compile-time validated subset of Bevy-like system signatures rather than claiming immediate parity with all Bevy signatures
- The supported MVP **Module** authoring subset is the **Module Authoring Contract**, not an implicit implementation detail
- **Registration** reruns on **Module** reload and must remain metadata-only
- **First-load Initialization** runs once per **Module** activation in a given world and may mutate world state
- **First-load Initialization** does not imply inter-Module ordering guarantees; world composition order is owned outside the **Module** authoring model
- A **Reload Compatibility Failure** leaves the previous **Module** generation active and blocks the new one from becoming active until the **Game** is relaunched or the incompatibility is resolved
- A **Reload Compatibility Failure** should report best-effort field-level schema changes and fall back to type-level reporting when exact diffs are unavailable
- A **Module** may later be packaged or exposed through the **Mod** workflow, but the two concepts are not equivalent

## Example dialogue

> **Dev:** "Should combat be a **Mod** or a **Module**?"
> **Domain expert:** "Combat is a **Module** because it's part of the core game and needs native-or-guest execution with preserve-state reload. A **Mod** is an externally authored add-on."

## Flagged ambiguities

- "mod" was being used to mean both an internal runtime-loaded gameplay unit and an external extension — resolved: these are distinct concepts, named **Module** and **Mod**.
- "module" could have meant either a Rust source module or a runtime unit — resolved: the runtime unit is a **Module**, its crate boundary is a **Module Crate**, and Wasvy authoring targets the crate root rather than a nested Rust source module.
- "shared type" vs local implementation state was fuzzy — resolved: cross-module or host-visible gameplay types belong in the **Shared API Crate**, while single-module gameplay types are **Module-private Types**.
- "private" could imply hard runtime isolation — resolved: in MVP, **Module-private** means architecturally owned by one Module, not yet enforced as a strict runtime boundary.
- "incompatible reload" was fuzzy — resolved: a persisted-state schema mismatch is a **Reload Compatibility Failure**, which keeps the previous **Module** active and instructs the developer to relaunch the **Game** for the latest code once the incompatibility is accepted or resolved.
- "setup" was overloaded — resolved: metadata-only reload-safe declaration is **Registration**, while one-time world mutation is **First-load Initialization**.
- "module initialization order" was ambiguous — resolved: **First-load Initialization** has no inter-Module ordering guarantees in the **Module** authoring model.
- "module metadata" could have been fully implicit — resolved: crate-level **Module** metadata is expressed through an explicit **Module Declaration** at the crate root.
- "module declaration syntax" could have been a crate attribute or a macro — resolved: the canonical **Module Declaration** uses a crate-root macro form.
- "what modules exist" vs "what modules are active" was fuzzy — resolved: **Workspace Inventory** is discovery of available Modules, while **World Composition** is the host-side selection of active Modules for a world.
- "module phase" vs Bevy schedule was fuzzy — resolved: MVP system authoring targets Bevy schedules directly, not Wasvy-defined semantic phases.
- "supporting Bevy signatures" was ambiguous — resolved: Wasvy may mirror Bevy syntax, but MVP support is limited to a curated subset whose guest semantics are explicitly supported and compile-time validated.
