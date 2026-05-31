# Wasvy Modules Runtime Prototype

**PROTOTYPE — throwaway code.**

## Question

Does the proposed **Wasvy Modules** runtime model feel right when driven by hand?

This prototype specifically explores:

- **Workspace Inventory** vs **World Composition**
- one shared host world
- **First-load Initialization** running once per world
- successful reload preserving world state while swapping code generation
- blocked reload keeping the old generation active
- a **Reload Compatibility Failure** requiring a relaunch

## Run

```bash
just prototype-wasvy-modules
```

Fallback:

```bash
cargo run --example wasvy_modules_runtime_prototype
```

## What to try

1. Seed the inventory with `s`
2. Add `combat` and `ai` to composition with `c` / `a`
3. Boot the world with `b`
4. Mutate shared and private state with `h`, `k`, `j`
5. Successful reload with `r` or `g` and verify state is preserved
6. Registration failure with `f` and verify old generation stays active
7. Compatibility failure with `x` and verify old generation stays active plus relaunch guidance
8. Restart the world with `w` and verify first-load init reruns
