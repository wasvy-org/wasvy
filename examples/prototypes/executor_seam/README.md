# PROTOTYPE — Wasvy executor seam

This is throwaway code for [Prototype the zero-overhead native and WASM executor seam](https://github.com/wasvy-org/wasvy/issues/81).

## Question

Can one stable, Wasvy-owned typed Bevy system invoke native code directly, expose the same live `ResMut` and `Query` parameters to a real WebAssembly Component through resource handles, and atomically switch the active Module Generation without replacing the Bevy executor or weakening Bevy's access analysis?

The prototype intentionally models only the dispatch-compatible fast path. It also lets you inspect candidate Plan changes that would require `ReplaceExecutors` or `ReplanSchedules`; it does not pretend to perform those transitions.

## Run

```bash
just prototype-executor-seam
```

The command builds the fixture guest for `wasm32-wasip2`, then launches the interactive host.

Try this sequence:

1. `t` — run the built-in Native Generation. The authored callable receives real typed Bevy parameters directly.
2. `w` — publish a compatible Wasm Generation. Executor installations remain one.
3. `t` — the same executor invokes a real Wasmtime Component. Imported resource handles synchronously read and mutate the executor's live `Counter` and `Actor` query values.
4. `n` — atomically return to a new Native Generation.
5. `i` and `s` — inspect why changed invocation or scheduling Plans cannot use executor reuse.

The screen exposes World state, backend call counts, Component-to-host bridge calls, and Bevy change-detection observations after every action.

## Deliberate prototype shortcuts

- The typed invocation lifetime is erased behind raw pointers, following the same synchronous-store idea explored in PR #76 and Wasvy's existing `Runner`. The Store is created, called, invalidated, and dropped entirely inside one executor invocation.
- The atomic slot intentionally leaks retired descriptors so in-flight readers remain safe without adding production reclamation machinery.
- The fixture has one resource, one query, and no `Commands`, async work, traps, executor replacement, or schedule replan.
- The rough benchmark measures complete `App::update` calls; it is not a reliable dispatch-overhead benchmark.
