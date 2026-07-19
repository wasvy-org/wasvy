# Wasvy

Wasvy is a runtime for composing Bevy applications from replaceable native and WebAssembly behavior while preserving World state.

## Language

**Host Application**:
A Bevy `App` running Wasvy that owns the Module Artifact Catalog, host policy, and one or more Worlds or SubApps with their World Compositions. Prose may shorten this to Host when unambiguous.
_Avoid_: Game, Wasmtime host

**Module**:
A globally stable logical identity for replaceable application behavior. A Module is independent of Module Artifact Kind, Artifact Provenance, use case, and any particular Bevy World.
_Avoid_: Runtime entity, native module, mod, workload, plugin

**Module ID**:
A durable, globally namespaced Module identity independent of version, crate name, filesystem path, and Module Artifact Kind. Its canonical form follows WIT-style `namespace:name`; short names are authoring aliases only.
_Avoid_: Cargo package name, asset path, versioned module name

**Module Instance**:
One Module running in one Bevy World, owning its world-local lifecycle, persistent state relationship, configuration, accounting, and active Module Generation. A World has at most one Instance per Module ID, while first-class multi-World and Bevy SubApp support allows the same Module to have simultaneous independent Instances.
_Avoid_: Module activation, loaded module

**Module Artifact**:
An immutable executable realization of a Module together with its canonical Plan. A Module Artifact may be embedded in the application binary or carried by a self-contained loadable file.
_Avoid_: Implementation, variant, patch, native plugin, metadata sidecar

**Module Artifact ID**:
An immutable, content-derived identity for one Module Artifact. Distinct builds remain distinct Artifacts even when they declare the same Artifact Version.
_Avoid_: Module ID, semantic version

**Artifact Version**:
Optional publisher-declared release metadata used for dependency constraints, selection policy, and diagnostics without defining Artifact identity or proving activation compatibility. Module Plan comparison remains authoritative for swap safety.
_Avoid_: Module Artifact ID, Module Generation, compatibility proof

**Module Artifact Kind**:
The execution form of a Module Artifact. `Native` means ABI-compatible in-process execution with typed Bevy parameters, while `Wasm` means execution as a WebAssembly Component; packaging and delivery remain Artifact Admission concerns.
_Avoid_: Provenance, admission, use case

**Guest**:
Code executing inside a WASM Module Artifact across the WebAssembly Component boundary. Guest is backend-specific and never describes the Module itself.
_Avoid_: Module, third-party extension

**Module Artifact Catalog**:
The set of Module Artifacts currently available to an application for selection and activation.
_Avoid_: World composition, active modules

**Host Contract Catalog**:
The Host Application's canonical source-derived Type, Interface, Schedule, and System Set Contracts, using the same portable schema model expected by Module Plans. It is authoritative for Host-owned contracts and resolves Type IDs to per-World State Bindings.
_Avoid_: Module Artifact Catalog, Bevy reflection registry, handwritten host manifest

**World Composition**:
The host's desired set of Modules and Artifact-selection constraints for one Bevy World. Runtime Module Instances and Generations are the observed state produced by reconciling a World Composition against the Module Artifact Catalog.
_Avoid_: Artifact catalog, active generation list

**Reconcile**:
Compare a World Composition with the Module Artifact Catalog and current Module Instances, then determine the activation and deactivation changes needed to make observed World state satisfy desired state.
_Avoid_: Reload, activate one artifact

**Admit**:
Accept a Module Artifact into the Module Artifact Catalog after policy and validation checks.
_Avoid_: Activate, load

**Activate**:
Select a Module Artifact for a Module in one World and create its next Generation. Initial activation also creates the Module Instance; later activation transitions the existing Instance to a compatible successor Generation. There is no forced in-place compatibility bypass; destructive reset requires explicit deactivation and fresh activation.
_Avoid_: Replace, reload, admit, force reload

**Deactivate**:
Remove a Module Instance from one World without implying eviction of its Artifact from the Catalog.
_Avoid_: Evict, unload artifact

**Evict**:
Remove an unused Module Artifact from the Catalog without implying deactivation of a Module in any World.
_Avoid_: Deactivate, unload module

**Module Generation**:
One execution epoch within a Module Instance, referencing exactly one Module Artifact. Reactivating the same Module Artifact creates a new Module Generation; prose may shorten this to Generation when the Module context is clear.
_Avoid_: Module version, artifact version

**Module Plan**:
The immutable, canonical, language-neutral declaration carried by exactly one Module Artifact, defining what that Artifact provides and requires: Module identity, systems, scheduling, invocation shape, state contract, dependencies, requested capabilities, and resource requirements. It contains portable source-derived facts—not host policy, granted resources, Bevy-resolved IDs, compatibility verdicts, or Module Instance and Generation state; a Module Instance's effective Plan is the Plan of its active Generation's Artifact.
_Avoid_: Runtime registration, handwritten manifest, module-wide manifest, runtime state

**Module Semantic Contract**:
The backend-independent part of a Module Plan describing the behavior shape an Artifact provides and requires. Native and WASM Artifacts derived from equivalent source may carry identical Semantic Contracts.
_Avoid_: Artifact binding, host policy, runtime state

**Artifact Binding**:
The backend-specific part of a Module Plan that maps its Semantic Contract to one Module Artifact Kind's executable exports, ABI requirements, and transport encodings. Admission verifies the Binding against the actual Artifact body; changing transport alone does not change logical State Compatibility when a lossless adapter exists.
_Avoid_: Semantic contract, runtime executor, persistent storage representation

**System ID**:
A durable identity unique within one Module for a system declared in its Module Semantic Contract. Authoring derives it from the source item path by default, so renaming or moving that item is explicitly treated as removing one system and adding another unless the author pins an ID; tooling and documentation must make this refactor behavior prominent.
_Avoid_: Export name, Bevy system node ID, display name

**Type Contract**:
A canonical entry in a Module Semantic Contract's deduplicated type table, identified by a stable Type ID and carrying the complete logical State Schema, kind, Schema Authority, permitted lifetime scopes, and derived fingerprints. It excludes Artifact transport encoding, Rust memory layout, runtime IDs, and whether values currently exist in a World.
_Avoid_: Rust type path, Bevy TypeId, fingerprint-only schema, inline schema copy

**Type ID**:
A durable, globally namespaced Type Contract identity independent of Rust `TypeId` and build output. Rust authoring derives it from package/module coordinates and the source item path by default; authors may pin it, while moving or renaming an unpinned persistent type is a state-identity change.
_Avoid_: Rust TypeId, bare Rust type path, schema fingerprint

**State Schema**:
Wasvy's canonical persistence algebra for primitives, records, tuples, variants, options/results, collections, bytes, Type ID references, and declared entity/relationship references, including stable inferred-but-pinnable field/variant IDs and required/defaulted/optional semantics. It may project into WIT, but WIT is not the persistence source of truth; opaque adapters are unverifiable unless they supply compatibility logic.
_Avoid_: WIT type alone, Rust memory layout, Serde implementation detail

**State Reference**:
A schema-declared required or optional reference from one persistent value to an entity or related value. Deactivation assessment applies Host policy to surviving references—block, cascade, or permit staleness—while opaque or undeclared handles receive no referential-integrity guarantee.
_Avoid_: Arbitrary integer handle, ownership, Bevy relation without schema

**Schema Authority**:
The single Host Application or Module ID entitled to define and evolve one stateful Type Contract. A Module Artifact may evolve only its own Module's contracts; Host-authoritative and other-Module-authoritative contracts are exact requirements, and a non-authoritative mismatch is rejected even when no values exist.
_Avoid_: Shared ownership, last writer

**Value Lifetime Owner**:
The Host Application or Module Instance whose lifecycle governs a concrete persistent value, independently of that value's Schema Authority. Host-owned values survive Module lifecycle changes; Module-Instance-owned values survive Generation replacement but are removed when that Instance is deactivated; compatibility examines every value of an affected Type ID regardless of owner.
_Avoid_: Schema authority, creator, last writer

**Generation-Local State**:
Module-visible ephemeral state scoped to one active or retiring Module Generation, including WASM Store/memory and future supported native locals or caches. It is discarded on retirement; uncontrolled native globals are outside the swappable authoring contract.
_Avoid_: Persistent state, reusable executor bookkeeping, Module-Instance-owned state

**Logical System Bookkeeping**:
Wasvy/Bevy executor state tied to a stable System ID—such as archetype caches, access metadata, and change-detection baselines. It may survive `ReuseExecutors` so a plan-compatible Generation replacement does not replay existing changes.
_Avoid_: Generation-local state, Module-visible Local state

**Access Attribution**:
Accounting that identifies the responsible Module Generation and System ID without implying Schema Authority or Value Lifetime Ownership. Ownership-changing operations are attributed exactly through context-aware Commands; zero-overhead native queries/resources use declared potential access and invocation-level attribution, with exact value-level tracing available only as optional instrumentation.
_Avoid_: State ownership, mandatory per-value tracing, guessed ownership

**Context-Aware Commands**:
The Native/WASM-equivalent command interface that stages ownership-changing operations with Module Instance and Generation context. Raw mutable World access is outside the replaceable Module contract; any future trusted Native escape hatch makes state guarantees unverifiable and may block replacement.
_Avoid_: Unrestricted `&mut World`, ordinary unattributed Commands

**Shared State**:
State visible to more than one Module. Shared describes access scope only; every shared Type Contract still has one Schema Authority and every shared value has one Value Lifetime Owner.
_Avoid_: Shared ownership, ownerless state

**State Binding**:
A Host-owned, per-World resolution from a logical Type ID to its concrete Bevy storage representation. Linked native types use typed components/resources; unknown types use Wasvy's canonical dynamic value representation; changing representation is a transactional rebind rather than a schema migration.
_Avoid_: Type Contract, Artifact transport encoding, serialized native state requirement

**State Ownership Ledger**:
Host-owned per-World metadata recording the Value Lifetime Owner of entities, component values, and resources without wrapping their native storage. Module-spawned entities and resources default to Module-Instance ownership; inserted components inherit their entity's owner; mutation never transfers ownership.
_Avoid_: Component wrapper, mutation log, Access Attribution

**Ownership Transfer**:
A transactional, policy-checked safe-point change of Value Lifetime Owner that never changes Schema Authority. Transfer to another Instance requires its existence and capabilities; Host adoption is explicit; entity transfer includes inherited component ownership by default; accounting and budgets move with ownership.
_Avoid_: Mutation, schema transfer, implicit adoption

**State Assessment View**:
A bounded, revisioned enumeration of persistent values by Type ID and Value Lifetime Owner used for value-aware compatibility checks. Assessment may run incrementally, but publication requires the assessed revision to remain valid or changed values to be revalidated; it never requires cloning the whole World.
_Avoid_: World snapshot, unversioned iterator, migration

**Persistent State Adapter**:
A Host-provided implementation that gives Wasvy State Bindings, ownership, revisioned enumeration, and transition support for a state store. Components and resources are built in; retained queues, assets, or custom stores opt in, while transient events, Generation-owned effects, and external systems are not implicitly persistent state.
_Avoid_: Capability adapter, universal side-effect tracker

**System Invocation Shape**:
The canonical, source-language-independent parameter algebra for one planned system, preserving ordered parameters and query data while explicitly representing stable Type Contract references and read/write access. Unordered filters and capability sets are canonically sorted, and neither Rust syntax nor Bevy runtime IDs survive normalization.
_Avoid_: Rust function signature, WIT export signature, serialized parameters

**Schedule ID**:
A durable, globally namespaced symbolic schedule identity carried in Module Plans and resolved by a Host Application to a concrete Bevy schedule. An unresolved Schedule ID blocks activation in that World without preventing Artifact Admission.
_Avoid_: Bevy schedule label type, interned schedule label, display name

**System Set ID**:
A stable symbolic identity for a Module-owned or Host-provided Bevy system set referenced by scheduling data. Plans canonically declare membership, ordering, and Planned Conditions; unresolved Host set references block activation in that World.
_Avoid_: Interned system set, source-language type name

**Lifecycle Hook**:
A stable, explicitly phased entry in a Module Semantic Contract. Lifecycle Hooks are distinct from planned systems and are not encoded as special Bevy schedules.
_Avoid_: Startup system, registration function

**First Activation**:
The transactional Lifecycle Hook run once when a Module Instance is created. It does not rerun for successor Generations; deactivation destroys the Instance, so later activation creates a new Instance and runs it again. Staged state uses the pending Instance's ownership defaults and is discarded if activation fails before publication.
_Avoid_: Per-Generation setup, Startup schedule, registration

**Planned Condition**:
A stable callable in a Module Semantic Contract that returns a boolean and controls planned system or system-set execution. It has its own identity, normalized invocation shape, and Artifact Binding rather than carrying an opaque source-language function path.
_Avoid_: Opaque run condition, system metadata flag

**Interface Contract**:
A canonical declaration of a provided or required Module interface using a WIT-qualified identity, version, and structural fingerprint. Artifact Bindings map its operations to native callables or WebAssembly Component imports and exports.
_Avoid_: Rust trait, backend export table, Cargo dependency

**Module Requirement**:
A Module Semantic Contract's dependency on another Module ID and one or more Interface Contracts, optionally marked optional. It never selects a Module Artifact ID; exact Artifact selection belongs to World Composition or host policy.
_Avoid_: Artifact pin, Cargo dependency

**Capability Request**:
A required or optional capability declared by a Module Semantic Contract. It states what an Artifact needs or can use but conveys no authority; the Host independently produces runtime grants.
_Avoid_: Capability grant, permission

**Resource Requirement**:
A Module Semantic Contract's declared minimum and preferred capacity for a typed runtime resource. Enforced maximum budgets and actual allocations remain host-owned runtime state.
_Avoid_: Resource allocation, quota grant

**Retained Type Contract**:
A Module-authoritative persistent Type Contract deliberately kept in a candidate Plan despite no current code using it, preserving existing values and rollback compatibility. Omitting such a contract while Instance-owned values exist is migration-required; omission never implies deletion.
_Avoid_: Orphaned state, implicit deletion, unused type removal

**State Preconditions**:
Optional declarative invariants or explicitly bound pure validators in a candidate state contract for semantic assumptions that structural schemas cannot express. Activation evaluates them read-only against a revisioned State Assessment View under resource limits; they may reject activation but never mutate state.
_Avoid_: Migration hook, runtime assertion after publication, implicit code assumption

**State Compatibility**:
The directional state-axis result of comparing an active Plan with a candidate: `Unchanged`, `StructurallyCompatible` with optional value preconditions, `MigrationRequired`, or `UnknownOrUnverifiable`. Structural compatibility requires lossless candidate representation and round-trip of active values; activation evaluates structural and semantic State Preconditions against existing values, while failure never permits silent reset, dropping, or partial conversion.
_Avoid_: Read-only decodability, schema equality, symmetric compatibility, automatic migration

**Plan Compatibility**:
A directional structural comparison from an active Module Plan to a candidate successor, reported across independent identity, execution, scheduling, state, dependency, capability, resource, and Artifact Binding axes. Fingerprints accelerate this comparison but never constitute or override its verdict; rollback is assessed separately in the reverse direction.
_Avoid_: Single compatibility level, symmetric version comparison, trusted fingerprint

**Activation Assessment**:
The context-dependent evaluation of Plan Compatibility against one World, its Module Instance state, resolved schedules, grants, allocations, dependencies, and host policy. A state-incompatible Artifact may remain admitted while activation is blocked only for affected Instances; their active Generations continue unchanged with value-aware diagnostics.
_Avoid_: Plan diff, artifact validation, artifact rejection

**State Transition**:
A reserved transactional step in an Activation Strategy that may transform persistent values before Generation publication. V1 supplies no implicit migration; future migration declarations require an explicit supported Plan feature and migration planner rather than an opaque candidate hook.
_Avoid_: Automatic conversion, first-activation hook, silent reset

**Activation Strategy**:
The runtime transaction derived from an Activation Assessment. Every successful Strategy prepares fully and commits at a safe point; its Execution Transition may reuse executors for dispatch-only publication, replace affected executors, or replan schedule topology, while composition reconciliation, grants, allocations, and state checks supply transaction actions and preconditions.
_Avoid_: User-selected reload mode, compatibility verdict, atomic-only fast path

**Execution Transition**:
The executor and schedule portion of an Activation Strategy: `ReuseExecutors`, `ReplaceExecutors`, or `ReplanSchedules`. Atomicity is a guarantee of the whole activation transaction, not a special property limited to `ReuseExecutors`.
_Avoid_: Compatibility level, reload mode

**Implementation Rollback**:
A new reverse-direction Activation selecting a previous Module Artifact against the current World state. Pre-publication failure leaves code and state unchanged, but post-publication rollback never implies undoing state mutations; future checkpointing may build on stable state enumeration/export seams.
_Avoid_: State rewind, transaction abort, restoring a version number

**Plan Annotation**:
Optional non-semantic metadata in a Module Plan for display names, documentation, deprecation messages, and namespaced publisher annotations. It is covered by Artifact identity and signatures but excluded from compatibility and executor decisions; nondeterministic or private build data does not belong here.
_Avoid_: Semantic contract field, source map, runtime policy

**Artifact Provenance**:
Immutable claims about who produced a Module Artifact and the source/build from which it was produced. Provenance travels with or is cryptographically associated with the Artifact without defining its Kind.
_Avoid_: Admission, delivery channel, artifact kind

**Artifact Admission**:
The host-local context and decision by which a Module Artifact is accepted, such as built-in, bundled, downloaded, or user-supplied. The same Artifact may have different Admission contexts in different installations.
_Avoid_: Provenance, artifact identity

**Authoring Source of Truth**:
The source code from which Wasvy derives a Module Plan during development.
_Avoid_: Hand-maintained plan

**Artifact Source of Truth**:
The embedded canonical Module Plan that represents a Module Artifact to tooling and runtimes after build finalization.
_Avoid_: Metadata sidecar
