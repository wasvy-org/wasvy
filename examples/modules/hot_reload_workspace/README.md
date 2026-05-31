# Hot Reload Workspace

This workspace showcases the intended **Wasvy Modules** guest workflow with `wasvy dev`:

- one host crate
- one shared API crate
- one module crate (`counter`)
- manifest-driven activation via `wasvy.toml`
- automatic guest rebuild + restage + hot reload when module code changes

## Run

From the repository root:

```bash
cargo run -p wasvy_cli -- dev examples/modules/hot_reload_workspace/wasvy.toml
```

The CLI will:

1. build the active guest module crates for `wasm32-wasip2`
2. stage them into `assets/modules/{module-name}.wasm`
3. run the host with Bevy asset watching enabled
4. watch the module workspace for changes
5. rebuild and restage changed modules so the running host hot reloads them

## Try a reload

Open:

```text
examples/modules/hot_reload_workspace/crates/modules/counter/src/lib.rs
```

Change:

```rust
const STEP: i32 = 1;
```

to:

```rust
const STEP: i32 = 5;
```

Save the file.

You should see:

- `wasvy dev` rebuild `counter`
- the running host report a new module generation
- `StatusBoard.module_ticks` continue increasing instead of resetting to zero
- `Runner.energy` start increasing by the new `STEP`

That demonstrates **preserve-state hot reload**: the module code changed, but the world and module state stayed alive.

## Native fallback

```bash
cargo run --manifest-path examples/modules/hot_reload_workspace/crates/game_host/Cargo.toml -- --native
```
