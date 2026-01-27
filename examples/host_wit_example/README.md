# host_wit_example

This host registers a `Health` component and exposes methods to mods via a generated WIT interface.
It uses `wasvy::auto_host_components!` so no manual resource implementations are required.
The build script generates `wit/components.wit` and `target/wasvy/components.wit` for the guest example.

Run:

```bash
RUSTUP_SKIP_SELF_UPDATE=1 cargo run --manifest-path examples/host_wit_example/Cargo.toml
```

Build the guest, then copy the wasm to `examples/host_wit_example/assets/mods/guest_wit_example.wasm`.
