# PROTOTYPE — Wasvy executor seam

This is throwaway code for [Prototype the zero-overhead native and WASM executor seam](https://github.com/wasvy-org/wasvy/issues/81).

## Question

Can one stable, Wasvy-owned typed Bevy system invoke native code directly, invoke a plan-compatible WASM Artifact through a bridge, and atomically switch the active Module Generation without replacing the Bevy executor or weakening Bevy's access analysis?

The prototype intentionally models only the dispatch-compatible fast path. It also lets you inspect candidate Plan changes that would require `ReplaceExecutors` or `ReplanSchedules`; it does not pretend to perform those transitions.

The WASM path is a local serialization bridge simulation, not a real WebAssembly Component. That keeps the experiment focused on the executor seam and atomic dispatch target.

## Run

```bash
just prototype-executor-seam
```

Try this sequence:

1. `t` — run the built-in Native Generation. Serialization remains zero.
2. `w` — publish a compatible Wasm Generation. Executor installations remain one.
3. `t` — the same executor takes the bridge path and preserves existing World state.
4. `n` — atomically return to a new Native Generation.
5. `i` and `s` — inspect why changed invocation or scheduling Plans cannot use executor reuse.

The atomic slot intentionally leaks retired descriptors so in-flight readers remain safe without adding production reclamation machinery to the prototype.
