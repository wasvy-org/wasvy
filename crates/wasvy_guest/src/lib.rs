//! Helpers for mod-side component invocation.
//!
//! Example with generated WIT bindings:
//! ```ignore
//! use wasvy_guest::{component_wrapper, impl_component_invoke_raw};
//! use my_game_bindings::game::components::health::Health;
//!
//! impl_component_invoke_raw!(Health);
//!
//! component_wrapper! {
//!     pub struct HealthExt(Health);
//!     impl HealthExt {
//!         fn heal(amount: f32) -> ();
//!         fn pct() -> f32;
//!     }
//! }
//! ```
//! The wrapper converts Rust args to JSON and calls the host's dynamic `invoke`.
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Errors returned when invoking methods with JSON serialization.
#[derive(Debug, Error)]
pub enum InvokeError {
    #[error("failed to serialize args: {0}")]
    Serialize(serde_json::Error),
    #[error("failed to deserialize response: {0}")]
    Deserialize(serde_json::Error),
}

/// Minimal invocation surface expected from generated WIT bindings.
pub trait ComponentInvokeRaw {
    fn invoke_raw(&self, method: &str, params: &str) -> String;
}

/// JSON-based invocation helper built on top of `ComponentInvokeRaw`.
pub trait ComponentInvokeJson: ComponentInvokeRaw {
    fn invoke_json<Args, Ret>(&self, method: &str, args: Args) -> Result<Ret, InvokeError>
    where
        Args: Serialize,
        Ret: DeserializeOwned,
    {
        let args_value = serde_json::to_value(args).map_err(InvokeError::Serialize)?;
        let args_json = serde_json::to_string(&args_value).map_err(InvokeError::Serialize)?;
        let raw = self.invoke_raw(method, &args_json);
        let value: Value = serde_json::from_str(&raw).map_err(InvokeError::Deserialize)?;
        serde_json::from_value(value).map_err(InvokeError::Deserialize)
    }
}

impl<T: ComponentInvokeRaw> ComponentInvokeJson for T {}

/// Typed invocation helper (alias of the JSON helper with a clearer name).
pub trait ComponentInvokeTyped {
    fn invoke_typed<Args, Ret>(&self, method: &str, args: Args) -> Result<Ret, InvokeError>
    where
        Args: Serialize,
        Ret: DeserializeOwned;
}

impl<T: ComponentInvokeRaw> ComponentInvokeTyped for T {
    fn invoke_typed<Args, Ret>(&self, method: &str, args: Args) -> Result<Ret, InvokeError>
    where
        Args: Serialize,
        Ret: DeserializeOwned,
    {
        ComponentInvokeJson::invoke_json(self, method, args)
    }
}

/// Implement `ComponentInvokeRaw` for a generated WIT resource.
///
/// This forwards to the resource's `invoke` method.
#[macro_export]
macro_rules! impl_component_invoke_raw {
    ($ty:path) => {
        impl $crate::ComponentInvokeRaw for $ty {
            fn invoke_raw(&self, method: &str, params: &str) -> String {
                self.invoke(method, params)
            }
        }
    };
}

/// Generate a small wrapper type with typed methods that call `invoke`.
///
/// # Example
/// ```ignore
/// component_wrapper! {
///     pub struct HealthExt(Health);
///     impl HealthExt {
///         fn pct() -> f32;
///     }
/// }
/// ```
#[macro_export]
macro_rules! component_wrapper {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident($inner:ty);
        impl $impl_name:ident {
            $(fn $method:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret:ty;)*
        }
    ) => {
        $(#[$meta])*
        $vis struct $name(pub $inner);

        impl $name {
            $vis fn new(inner: $inner) -> Self {
                Self(inner)
            }

            $vis fn inner(&self) -> &$inner {
                &self.0
            }

            $vis fn inner_mut(&mut self) -> &mut $inner {
                &mut self.0
            }

            $(
                $vis fn $method(&self, $($arg: $arg_ty),*) -> Result<$ret, $crate::InvokeError> {
                    let args = ($($arg,)*);
                    $crate::ComponentInvokeJson::invoke_json(&self.0, stringify!($method), args)
                }
            )*
        }
    };
}
