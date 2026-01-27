use std::marker::PhantomData;

use bevy_app::{App, Plugin};
use bevy_ecs::component::Component;
use bevy_reflect::{GetTypeRegistration, Reflect, TypePath};

use crate::methods::MethodRegistry;

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
