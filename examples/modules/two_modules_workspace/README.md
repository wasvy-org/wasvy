# Two Modules Workspace

Example **Wasvy Modules** workspace with:

- one host crate
- one shared API crate
- two module crates: `combat` and `ai`
- `wasvy.toml` inventory + world composition

## Layout

- `crates/game_api` — shared types used by the host and both modules
- `crates/modules/combat` — one Module Crate that builds as both guest wasm and native adapter
- `crates/modules/ai` — one Module Crate that builds as both guest wasm and native adapter
- `crates/game_host` — host app that activates the manifest-selected world composition

## Run

### Guest mode

From the repo root:

```bash
cargo run -p wasvy_cli -- dev examples/modules/two_modules_workspace/wasvy.toml
```

Or via `just`:

```bash
just dev-two-modules-workspace
```

Lower-level manual build/stage steps are still available if you want to inspect the raw guest artifacts:

```bash
just build-two-modules-workspace-guests
cargo run --manifest-path examples/modules/two_modules_workspace/crates/game_host/Cargo.toml
```

### Native mode

```bash
just run-two-modules-workspace-native
```

## What it shows

- a real `wasvy.toml` workspace inventory and default world composition
- one Module Crate per Module with no parallel `*_guest` crates
- the same module source running in guest and native modes
- guest mode as the default run path for the example
- `wasvy dev` building/staging guest artifacts and watching for module changes
- manifest-driven guest activation by stable module name
- one-time first-load initialization in guest and native paths
- shared API state mutated by both modules in one shared world
- guest artifacts live in `assets/modules/{module-name}.wasm` and are ignored by git
