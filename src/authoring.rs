use std::marker::PhantomData;

use bevy_app::Plugin;
pub use bevy_app::App;
use bevy_ecs::component::Component;
use bevy_reflect::{GetTypeRegistration, Reflect, TypePath};

use crate::methods::MethodRegistry;

#[derive(Clone, Copy)]
pub struct WasvyComponentRegistration {
    pub register: fn(&mut App),
}

#[derive(Clone, Copy)]
pub struct WasvyMethodsRegistration {
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

pub use inventory;

pub trait WasvyComponent: Component + Reflect + TypePath + GetTypeRegistration {
    fn register(app: &mut App)
    where
        Self: Sized,
    {
        app.register_type::<Self>();
    }

}

pub trait WasvyMethods: Reflect + TypePath {
    fn register_methods(registry: &mut MethodRegistry);
}

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

pub struct WasvyAutoRegistrationPlugin;

impl Plugin for WasvyAutoRegistrationPlugin {
    fn build(&self, app: &mut App) {
        register_all(app);
    }
}

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
