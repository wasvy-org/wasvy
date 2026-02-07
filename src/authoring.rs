//! Component and method registration helpers for Wasvy authoring.
//!
//! # Overview
//! - [`WasvyComponent`] marks a component as exportable to mods, even if it has
//!   **no methods**. This is important for components that are queried or
//!   serialized but never invoked.
//! - [`WasvyMethods`] registers exported methods (typically via
//!   [`#[wasvy::methods]`](crate::methods)).
//! - [`WasvyAutoRegistrationPlugin`] and [`register_all`] apply all submitted
//!   registrations to a Bevy `App`.
//!
//! # Example (component without methods)
//! ```ignore
//! use bevy_ecs::prelude::*;
//! use bevy_reflect::Reflect;
//! use wasvy::WasvyComponent;
//!
//! #[derive(Component, Reflect, Default, WasvyComponent)]
//! #[reflect(Component)]
//! struct Tag;
//! ```
//!
//! # Example (component with methods)
//! ```ignore
//! use bevy_ecs::prelude::*;
//! use bevy_reflect::Reflect;
//! use wasvy::WasvyComponent;
//!
//! #[derive(Component, Reflect, Default, WasvyComponent)]
//! #[reflect(Component)]
//! struct Health { current: f32, max: f32 }
//!
//! #[wasvy::methods]
//! impl Health {
//!     fn heal(&mut self, amount: f32) {
//!         self.current = (self.current + amount).min(self.max);
//!     }
//! }
//! ```

use std::marker::PhantomData;

pub use bevy_app::App;
use bevy_app::Plugin;
use bevy_ecs::component::Component;
use bevy_ecs::reflect::{AppFunctionRegistry, AppTypeRegistry};
use bevy_reflect::{FromType, GetTypeRegistration, Reflect, TypePath};

/// Inventory entry that registers a component with a Bevy app.
#[derive(Clone, Copy)]
pub struct WasvyComponentRegistration {
    /// Function invoked to register the component (typically registers reflect).
    pub register: fn(&mut App),
}

/// Inventory entry that captures method argument names for WIT generation.
///
/// Emitted by `#[wasvy::methods]` so WIT signatures can use real parameter names.
#[derive(Clone, Copy)]
pub struct WasvyMethodMetadata {
    /// Fully-qualified type path of the receiver.
    pub type_path: &'static str,
    /// Method name as exposed to mods.
    pub method: &'static str,
    /// Ordered argument names (excluding `self`).
    pub arg_names: &'static [&'static str],
}

/// Inventory entry that registers exported methods for a component.
#[derive(Clone, Copy)]
pub struct WasvyMethodsRegistration {
    /// Function invoked to register methods on the Bevy app.
    pub register: fn(&mut App),
}

inventory::collect!(WasvyComponentRegistration);
inventory::collect!(WasvyMethodMetadata);
inventory::collect!(WasvyMethodsRegistration);

#[doc(hidden)]
#[macro_export]
macro_rules! __wasvy_submit_component_registration {
    ($info:expr) => {
        $crate::authoring::inventory::submit! { $info }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __wasvy_submit_methods_registration {
    ($info:expr) => {
        $crate::authoring::inventory::submit! { $info }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __wasvy_submit_method_metadata {
    ($info:expr) => {
        $crate::authoring::inventory::submit! { $info }
    };
}

/// Re-exported inventory crate for proc-macro submissions.
pub use inventory;

/// Marker type data for components exported to Wasvy.
///
/// This data is used to identify which components should appear in WIT.
#[derive(Clone)]
pub struct WasvyExport;

impl<T> FromType<T> for WasvyExport {
    fn from_type() -> Self {
        WasvyExport
    }
}

/// Trait implemented by components that are exported to mods.
///
/// This exists so components without methods can still be exported.
///
/// # Example
/// ```ignore
/// use bevy_ecs::prelude::*;
/// use bevy_reflect::Reflect;
/// use wasvy::WasvyComponent;
///
/// #[derive(Component, Reflect, Default, WasvyComponent)]
/// #[reflect(Component)]
/// struct Tag;
/// ```
pub trait WasvyComponent: Component + Reflect + TypePath + GetTypeRegistration {
    /// Register the component's reflect data with the app.
    fn register(app: &mut App)
    where
        Self: Sized,
    {
        app.register_type::<Self>();
        app.register_type_data::<Self, WasvyExport>();
    }
}

/// Trait implemented by components that export methods to mods.
///
/// Prefer using `#[wasvy::methods]` which implements this trait and registers
/// methods automatically.
///
/// # Example
/// ```ignore
/// #[wasvy::methods]
/// impl Health {
///     fn heal(&mut self, amount: f32) {
///         self.current += amount;
///     }
/// }
/// ```
pub trait WasvyMethods: Reflect + TypePath {
    /// Register all exported methods for this type on the Bevy app.
    fn register_methods(app: &mut App);
}

/// Plugin that registers a single component type.
pub struct WasvyComponentPlugin<T>(PhantomData<T>);

impl<T> Default for WasvyComponentPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: WasvyComponent> Plugin for WasvyComponentPlugin<T> {
    fn build(&self, app: &mut App) {
        T::register(app);
    }
}

/// Plugin that registers a single component's exported methods.
pub struct WasvyMethodsPlugin<T>(PhantomData<T>);

impl<T> Default for WasvyMethodsPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: WasvyMethods> Plugin for WasvyMethodsPlugin<T> {
    fn build(&self, app: &mut App) {
        app.init_resource::<AppFunctionRegistry>();
        T::register_methods(app);
    }
}

/// Plugin that registers all components and methods submitted to inventory.
pub struct WasvyAutoRegistrationPlugin;

impl Plugin for WasvyAutoRegistrationPlugin {
    fn build(&self, app: &mut App) {
        register_all(app);
    }
}

/// Register all components and methods submitted via inventory.
///
/// This is used by `WasvyAutoRegistrationPlugin` and can be called directly
/// in build scripts that generate WIT.
pub fn register_all(app: &mut App) {
    app.init_resource::<AppTypeRegistry>();
    app.init_resource::<AppFunctionRegistry>();

    for registration in inventory::iter::<WasvyComponentRegistration> {
        (registration.register)(app);
    }

    for registration in inventory::iter::<WasvyMethodsRegistration> {
        (registration.register)(app);
    }
}
