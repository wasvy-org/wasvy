//! Component and method registration helpers for Wasvy authoring.
//!
//! These traits, plugins, and inventory records are used by the
//! `#[wasvy::component]` and `#[wasvy::methods]` macros to register components
//! and exported methods without extra boilerplate.

use std::marker::PhantomData;

use bevy_app::Plugin;
pub use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_reflect::{GetTypeRegistration, Reflect, TypePath};

use crate::methods::MethodRegistry;

/// Inventory entry that registers a component with a Bevy app.
#[derive(Clone, Copy)]
pub struct WasvyComponentRegistration {
    /// Function invoked to register the component (typically registers reflect).
    pub register: fn(&mut App),
}

/// Inventory entry that registers exported methods for a component.
#[derive(Clone, Copy)]
pub struct WasvyMethodsRegistration {
    /// Function invoked to register the methods in the method registry.
    pub register: fn(&mut MethodRegistry),
}

inventory::collect!(WasvyComponentRegistration);
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

/// Re-exported inventory crate for proc-macro submissions.
pub use inventory;

/// Trait implemented by components that are exported to mods.
pub trait WasvyComponent: Component + Reflect + TypePath + GetTypeRegistration {
    /// Register the component's reflect data with the app.
    fn register(app: &mut App)
    where
        Self: Sized,
    {
        app.register_type::<Self>();
    }

}

/// Trait implemented by components that export methods to mods.
pub trait WasvyMethods: Reflect + TypePath {
    /// Register all exported methods for this type.
    fn register_methods(registry: &mut MethodRegistry);
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
        app.init_resource::<MethodRegistry>();
        let mut registry = app
            .world_mut()
            .get_resource_mut::<MethodRegistry>()
            .expect("MethodRegistry to be initialized");
        T::register_methods(&mut registry);
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
pub fn register_all(app: &mut App) {
    for registration in inventory::iter::<WasvyComponentRegistration> {
        (registration.register)(app);
    }

    app.init_resource::<MethodRegistry>();
    let mut registry = app
        .world_mut()
        .get_resource_mut::<MethodRegistry>()
        .expect("MethodRegistry to be initialized");

    for registration in inventory::iter::<WasvyMethodsRegistration> {
        (registration.register)(&mut registry);
    }
}
