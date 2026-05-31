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
just run-two-modules-workspace
```

Equivalent manual steps:

```bash
cargo build --manifest-path examples/modules/two_modules_workspace/Cargo.toml --target wasm32-wasip2 -p two_modules_combat -p two_modules_ai
mkdir -p examples/modules/two_modules_workspace/assets/modules
cp examples/modules/two_modules_workspace/target/wasm32-wasip2/debug/combat.wasm examples/modules/two_modules_workspace/assets/modules/combat.wasm
cp examples/modules/two_modules_workspace/target/wasm32-wasip2/debug/ai.wasm examples/modules/two_modules_workspace/assets/modules/ai.wasm
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
- manifest-driven guest activation by stable module name
- one-time first-load initialization in guest and native paths
- shared API state mutated by both modules in one shared world
- guest artifacts live in `assets/modules/{module-name}.wasm` and are ignored by git
