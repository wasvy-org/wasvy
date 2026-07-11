# Wasvy Runtime Vision

## A stateful, hot-swappable execution runtime for data-oriented applications

The opportunity for Wasvy is larger than hot-reloadable Bevy modules:

> **Wasvy is a stateful, hot-swappable execution runtime for data-oriented applications.**

Bevy provides the world, scheduler, and data model. Wasvy adds implementation replacement, artifact distribution, compatibility analysis, lifecycle ownership, and language/backend portability.

This document explores:

1. The architectural pieces still needed to realize that vision.
2. Important blind spots in live code replacement.
3. Applications beyond game development.
4. New capabilities enabled by this architecture.
5. A possible product structure that keeps the core focused.

---

# 1. Missing architectural components

A complete system likely needs the following flow:

```text
Source code
    ↓
Plan compiler
    ↓
Self-describing implementation artifact
    ↓
Implementation catalog + resolver
    ↓
Activation coordinator
    ↓
Wasvy-owned executors
    ↓
Bevy World
```

## 1.1 Plan compiler and canonical model

Plan compilation should be a deep module with a small interface:

```rust
PlanCompiler::compile(fragments) -> CanonicalPlan
PlanDiff::classify(active, candidate) -> SwapStrategy
```

It should own:

- Canonicalization
- Fingerprinting
- Structural comparison
- Diagnostics
- Format versioning
- Native/WASM equivalence checks

No other part of Wasvy should independently implement plan comparison.

## 1.2 Implementation catalog

Wasvy needs a catalog of every available implementation:

```text
ImplementationCatalog {
    "combat": [
        BuiltInNative(1.0.0),
        BundledWasm(1.1.0),
        InstalledPatch(1.2.0),
    ],
}
```

It must track:

- Logical identity
- Version
- Backend
- Provenance
- Trust status
- Compatibility
- Availability
- Whether an implementation is active, staged, blocked, or retained for rollback

## 1.3 Implementation resolver

Selecting an implementation should be a policy decision separate from loading it:

```rust
ImplementationResolver::select(
    module,
    available_implementations,
    host_policy,
) -> Selection
```

Possible policies include:

- Prefer the newest trusted patch.
- Prefer native unless explicitly overridden.
- Pin a version.
- Select by deployment profile.
- Select per world or tenant.
- Roll back automatically after a health failure.
- Disable external implementations in production.

## 1.4 Activation coordinator

One place must own the complete replacement transaction:

```text
Discover
→ Inspect
→ Authenticate
→ Validate
→ Prepare
→ Quiesce
→ Migrate
→ Publish
→ Observe
→ Commit or roll back
```

It should own the safe activation point and guarantee that a module does not execute mixed generations during one logical update boundary.

## 1.5 Executor factory

Wasvy needs a unified interface capable of constructing:

- Typed, native-optimized executors
- Typed native/WASM switchable executors
- Fully dynamic executors for previously unknown implementations

```rust
ExecutorFactory::build(system_plan, available_shape) -> BoxedExecutor
```

This is likely one of the hardest implementation modules because it controls both Bevy scheduling correctness and native performance.

## 1.6 Persistent state registry

The runtime needs explicit knowledge of state ownership:

```text
PersistentStateRegistry {
    type_id,
    owner,
    schema,
    storage,
    active_readers,
    active_writers,
}
```

It must distinguish:

- Application-owned state
- Module-owned persistent state
- Shared state
- Generation-local ephemeral state
- Dynamically introduced state

Without this, compatibility checks can only approximate what must be preserved.

## 1.7 Migration engine

Restart-required incompatibility is acceptable initially, but live patching eventually requires migrations:

```rust
#[wasvy::migration(from = 1, to = 2)]
fn migrate(old: OldState) -> NewState {}
```

The engine will need:

- Migration graphs rather than only one-step migration
- Transactional execution
- Validation before commit
- Field-level diagnostics
- Forward migration
- A rollback story

Migration correctness may become harder than code swapping itself.

## 1.8 Capability engine

Plans describe requested access, while host policy grants it:

```text
Requested capabilities
    ∩ host policy
    ∩ publisher trust
    = granted capabilities
```

Capabilities may include:

- Component and resource access
- Schedule access
- Filesystem paths
- Network destinations
- Clock and randomness
- Process APIs
- Memory and CPU budgets
- Other module interfaces

This engine must work consistently for native and WASM implementations, while documenting that native code cannot be strongly isolated inside the same process.

## 1.9 Artifact finalizer and verifier

The finalizer should:

- Merge generated plan fragments
- Attach the canonical plan to the final component
- Validate exports
- Add build provenance
- Compute content hashes
- Optionally optimize the component
- Produce reproducible output
- Sign or prepare the artifact for signing

The runtime verifier must independently repeat every safety-sensitive check. Build-time validation improves developer experience but cannot be trusted at runtime.

## 1.10 Observability and supervision

Every implementation needs an operational identity:

```text
module=combat
implementation=wasm-patch-1.2.2
generation=8
system=combat.tick
```

Wasvy should expose:

- Invocation duration
- Serialization cost
- Traps and panics
- Fuel consumption
- Memory usage
- Reload duration
- Failed swap reasons
- Active generation
- State migration results
- Executor backend
- Schedule conflicts

A supervision policy could disable or roll back repeatedly failing generations.

## 1.11 Conformance suite

Native and WASM equivalence needs executable specifications:

```text
Given the same initial world
and the same update sequence,
native and WASM implementations
must produce equivalent world deltas.
```

The suite should test:

- Query iteration
- Commands
- Deferred application
- Resources
- Change detection
- Ordering
- Errors
- First activation
- Reload
- Migration

This may become one of Wasvy's most valuable development tools.

---

# 2. Important blind spots

## 2.1 Atomic code swap does not imply atomic behavioral rollback

Suppose generation 2 activates successfully and corrupts state over ten frames. Switching the implementation pointer back to generation 1 does not restore the old world.

Real rollback may require:

- State snapshots
- An event log
- Reversible migrations
- World-delta journaling
- Application-specific recovery hooks

We should distinguish:

- **Implementation rollback:** restore old code.
- **State rollback:** restore old data.
- **Operational rollback:** restore the entire application to known-good behavior.

Only the first is easy.

## 2.2 In-flight work can outlive a generation

Modules may eventually create:

- Async tasks
- Background threads
- Timers
- Network requests
- Deferred commands
- Observers
- Coroutines
- External subscriptions

An old generation could continue producing effects after replacement.

Wasvy needs generation-scoped ownership:

```text
GenerationLease
├── tasks
├── deferred commands
├── subscriptions
├── handles
└── external resources
```

On replacement, the runtime must cancel, drain, reject, or deliberately transfer these effects.

## 2.3 Safe points are application-specific

“Swap at the start of a frame” works for games, but not every application has frames.

Other activation boundaries include:

- End of a simulation tick
- End of a transaction
- Between data batches
- After request drain
- At an industrial machine safe state
- At a consensus or log checkpoint

Wasvy should model a configurable **activation boundary**, not hard-code game-frame semantics.

## 2.4 Cross-module upgrades

`combat` may depend on `inventory` version 3 while the active inventory implementation is version 2.

Some patches must activate as a set:

```text
combat 1.2
inventory 3.0
shared-api 4
```

The activation coordinator eventually needs multi-module transactions:

```text
Prepare all
→ verify dependency graph
→ migrate all
→ atomically publish composition generation
```

A module-at-a-time model will be insufficient for larger applications.

## 2.5 Shared state creates upgrade coupling

If several modules use the same resource or component, ownership becomes important:

- Which module may migrate it?
- Can one module require a newer schema?
- Can old and new implementations coexist?
- Is the shared interface versioned independently?
- Can two worlds use different schema generations?

Shared state needs explicit ownership and version governance.

## 2.6 Native/WASM semantics can drift silently

Even with identical source, differences can emerge from:

- Serialization
- Floating-point behavior
- Query iteration
- Change ticks
- Deferred command timing
- Panic versus trap behavior
- Threading
- Randomness
- Time access
- Drop-based write-back
- Integer overflow configuration

Dual-mode equivalence needs defined levels:

1. Same scheduling and access contract
2. Same world-visible results
3. Deterministic byte-for-byte state
4. Merely functionally similar

Not every domain requires the strongest level.

## 2.7 Type identity is harder than type paths

A string such as:

```text
game_api::combat::Health
```

is not necessarily a durable schema identity. Crates can be renamed, types moved, and unrelated publishers can use the same path.

Long-lived patchability may require explicit stable identities:

```rust
#[wasvy::type_id("game.health")]
struct Health { ... }
```

Function and type paths can remain ergonomic defaults, but durable identities must survive refactoring.

## 2.8 The plan is derived but becomes authoritative at runtime

The code is the authoring source of truth. A deployed runtime, however, does not possess that source code.

The model should be explicit:

- **Authoring truth:** source code
- **Artifact truth:** embedded canonical plan
- **Runtime authority:** host policy and runtime validation

## 2.9 Custom-section durability

WASM tooling may:

- Strip custom sections
- Reorder sections
- Nest core modules inside components
- Rewrite components
- Produce non-reproducible binaries

The finalizer must be the last transformation before signing, and CI should verify that every released artifact still contains a valid plan.

## 2.10 Bevy stability risk

Dynamic system removal, typed executor construction, schedule mutation, and change tracking may rely on Bevy details that evolve.

Wasvy should isolate Bevy-specific integration behind a narrow adapter rather than letting those details leak into planning, artifacts, and distribution.

## 2.11 Dynamic modules may reduce scheduler performance

Bevy's performance depends on accurate access patterns. If dynamic plans become overly broad, parallelism collapses.

Plan precision is therefore both a correctness feature and a performance feature.

## 2.12 Security differs radically between native and WASM

A built-in native implementation can bypass Wasvy capability rules because it shares the process.

That may be acceptable because native code is trusted, but Wasvy should not claim native sandboxing. Wasvy can provide semantic capability discipline for native code; WASM provides the enforceable isolation path.

## 2.13 Resource exhaustion

Correct code can still be operationally unsafe:

- A system runs for seconds.
- A module allocates gigabytes.
- A patch spawns millions of entities.
- A module floods logs or network requests.
- Repeated reloads exhaust compilation caches.

Wasvy will eventually need quotas, fuel, timeouts, and failure policies, especially outside local development.

## 2.14 Platform restrictions

Consoles, mobile platforms, browsers, and locked-down enterprise environments may restrict:

- JIT compilation
- Runtime executable memory
- Filesystem access
- Downloaded executable content
- Unsigned code
- Background threads

Wasvy may need interpreter or AOT backends and platform-specific artifact strategies.

## 2.15 Product scope explosion

Runtime, SDK, package manager, updater, signing system, schema engine, and fleet-control platform could each become products.

The core should remain deep and narrow:

```rust
runtime.offer(artifact)
runtime.activate(module_id)
runtime.observe(module_id)
```

Distribution servers, marketplaces, and fleet management should sit above this seam rather than enter the core runtime.

---

# 3. Broader real-world uses

## 3.1 Live server logic

Long-running authoritative servers could patch:

- Matchmaking rules
- Combat balance
- Fraud detection
- Rate limiting
- NPC logic
- Economy calculations

without restarting worlds or disconnecting users.

This may be a stronger commercial use case than client-side game patches because server operators control trust and deployment.

## 3.2 Digital twins and industrial simulation

A factory or infrastructure simulation often has:

- Persistent world state
- Many data-oriented systems
- Long-running processes
- Models that evolve independently

Wasvy could replace a machine model or control simulation while preserving the digital twin's state. The activation boundary might be between simulation ticks or when machinery enters a safe state.

## 3.3 Robotics

Robot behavior can be decomposed into:

- Perception
- Planning
- Navigation
- Safety policy
- Device adapters

A WASM implementation could be tested in simulation and later promoted to an edge device. Signed patches could replace one behavior module without reflashing the entire application.

This requires strict real-time and safety constraints, making it a longer-term but potentially powerful use case.

## 3.4 Data pipelines and streaming systems

Bevy's scheduler can model data transformations as systems over ECS data.

Wasvy could support hot-swappable:

- Enrichment rules
- Parsers
- Classifiers
- Routing logic
- Validation stages
- Aggregations

The activation boundary becomes the boundary between batches or stream offsets.

## 3.5 Agent and AI simulation

Agent systems naturally map to ECS:

- Perception
- Memory
- Planning
- Tool selection
- Behavior policy

Wasvy could swap agent policies while preserving agent memory and world state. A candidate policy could even run in shadow mode before activation.

## 3.6 Business rule engines

Insurance, logistics, pricing, tax, entitlement, and workflow systems need frequently updated rules.

Wasvy could offer:

- Signed rule implementations
- Versioned state
- Explainable activation decisions
- Tenant-specific compositions
- Rapid rollback
- Auditable artifact identities

WASM is attractive because it supports multiple implementation languages and constrained execution.

## 3.7 Scientific and engineering models

Researchers could replace:

- Physics models
- Environmental models
- Epidemiological behavior
- Optimization strategies
- Numerical solvers

without rebuilding the surrounding visualization and state-management application.

Native mode supports performance; WASM supports portable experiments and distribution.

## 3.8 Extensible desktop applications

Editors, CAD tools, creative applications, and developer tools could represent document state in ECS and load:

- Importers and exporters
- Analysis passes
- Automation tools
- Validators
- Rendering passes
- User extensions

Wasvy would provide a richer extension runtime than traditional callbacks because extensions participate in scheduled data-oriented processing.

## 3.9 Multi-tenant SaaS customization

Each tenant could have a different world composition:

```text
Tenant A:
  standard-billing native
  custom-routing WASM

Tenant B:
  standard-billing native
  regional-tax WASM
```

The host maintains one application platform while Wasvy selects tenant-specific implementations and policies.

This requires strong per-world isolation and resource accounting.

## 3.10 Feature experimentation and canary releases

The same logical module can have several implementations:

```text
recommendations:
├── native-stable
├── wasm-candidate-a
└── wasm-candidate-b
```

Different worlds or users can receive different variants. Wasvy could support:

- A/B testing
- Canary rollout
- Percentage-based activation
- Immediate rollback
- Side-by-side metrics

## 3.11 Fault injection and resilience testing

A testing implementation could intentionally:

- Delay operations
- Return malformed data
- Drop messages
- Consume CPU
- Simulate unavailable dependencies

Because implementations are replaceable, Wasvy can become a fault-injection framework for ECS applications.

## 3.12 Education and interactive programming

Students could modify Bevy-like systems and see them reload into a running simulation without learning the complete build and deployment process.

This could support interactive labs for:

- ECS concepts
- Physics
- AI
- Distributed systems
- Biology and ecosystems
- Data-oriented design

---

# 4. Creative capabilities enabled by the architecture

## 4.1 Shadow execution

Before activating a patch, run it against a copied or recorded world slice:

```text
Active implementation → authoritative output
Candidate implementation → shadow output
                            ↓
                         compare
```

This enables behavioral validation before promotion.

## 4.2 World-delta testing

Record only what an implementation changes:

```rust
WorldDelta {
    modified_components,
    inserted_resources,
    spawned_entities,
    emitted_messages,
}
```

Native and WASM implementations could then be compared automatically.

## 4.3 Per-world implementations

Different Bevy worlds—or isolated world partitions—could run different generations simultaneously. This supports tests, tenants, matches, and canaries inside one process.

## 4.4 Time-travel deployment debugging

Record:

- Active implementation generation
- Input events
- World snapshots or deltas
- Swap events

Then replay a production failure with exactly the implementation sequence that produced it.

## 4.5 Automatic patch rehearsal

A deployment tool could:

1. Load a production snapshot.
2. Run the candidate for a fixed number of ticks.
3. Verify invariants.
4. Compare performance.
5. Approve or reject rollout.

## 4.6 Partial export replacement

Eventually, one artifact might override only selected exports:

```text
combat native implementation:
  movement
  damage
  cleanup

WASM emergency patch:
  damage only
```

This could make patches small, but it complicates generation consistency and should come only after whole-module replacement is solid.

---

# 5. Strategic product layers

To avoid scope confusion, Wasvy could be separated into product layers.

## Wasvy Runtime

Backend-neutral execution, plans, generations, state, and activation.

## Wasvy Authoring

Rust macros, guest SDKs, conformance tests, and Bevy-like ergonomics.

## Wasvy Dev

Build, watch, inspect, diff, shadow testing, and local reload workflows.

## Wasvy Distribution

Packaging, signatures, repositories, patch selection, and rollback.

## Wasvy Fleet

An optional future product for remote deployment, canaries, metrics, policy, and update orchestration across servers or edge devices.

The first three belong naturally in the core project. Distribution and fleet capabilities should build on the runtime's interface rather than complicate it.

---

# Conclusion

The greatest missing concept is not another execution backend. It is **operational ownership**.

Once Wasvy can replace code in a persistent world, it becomes responsible for:

- When replacement is safe
- What state belongs to whom
- Whether the candidate is trustworthy
- What happens to in-flight work
- How failure is detected
- Whether rollback actually restores correctness
- How operators understand what is running

If these become first-class concepts, Wasvy can grow from a convenient mod loader into a general platform for safely evolving long-running data-oriented applications.
