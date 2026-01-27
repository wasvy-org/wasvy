# wasvy_guest

Helpers for mod-side DSL wrappers over Wasvy component `invoke`.

## Example

```rust,ignore
use wasvy_guest::{component_wrapper, impl_component_invoke_raw};
use my_game_bindings::game::components::health::Health;

impl_component_invoke_raw!(Health);

component_wrapper! {
    pub struct HealthExt(Health);
    impl HealthExt {
        fn heal(amount: f32) -> ();
        fn pct() -> f32;
    }
}
```

`HealthExt` exposes typed methods, internally calling the host's dynamic `invoke` with JSON params.
