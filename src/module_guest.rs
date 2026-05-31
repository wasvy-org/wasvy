use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use bevy_reflect::TypePath;
use serde::{Serialize, de::DeserializeOwned};

#[cfg(not(target_arch = "wasm32"))]
pub use crate::schedule::ModStartup;
#[cfg(not(target_arch = "wasm32"))]
pub use bevy_app::{
    FixedPostUpdate, FixedPreUpdate, FixedUpdate, PostUpdate, PreUpdate, Startup, Update,
};

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct ModStartup;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct PreUpdate;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct Startup;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct Update;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct PostUpdate;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct FixedPreUpdate;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct FixedUpdate;
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct FixedPostUpdate;

pub trait GuestCommandsBinding {
    fn insert_resource(&self, type_path: &str, value: &[u8]);
    fn remove_resource(&self, type_path: &str);
}

pub trait GuestWorldResourceBinding {
    fn get(&self) -> Vec<u8>;
    fn set(&self, value: &[u8]);
}

pub trait GuestComponentBinding {
    fn get(&self) -> Vec<u8>;
    fn set(&self, value: &[u8]);
}

pub trait GuestQueryResultBinding {
    type Component: GuestComponentBinding;

    fn component(&self, index: u8) -> Self::Component;
}

pub trait GuestQueryBinding {
    type QueryResult: GuestQueryResultBinding;

    fn next(&mut self) -> Option<Self::QueryResult>;
}

pub struct Commands<B> {
    inner: B,
}

impl<B> Commands<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }
}

impl<B: GuestCommandsBinding> Commands<B> {
    pub fn insert_resource<T>(&mut self, value: T)
    where
        T: Serialize + TypePath,
    {
        self.inner.insert_resource(
            T::type_path(),
            &serde_json::to_vec(&value).expect("resource to serialize"),
        );
    }

    pub fn remove_resource<T>(&mut self)
    where
        T: TypePath,
    {
        self.inner.remove_resource(T::type_path());
    }
}

pub struct Res<T, B> {
    value: T,
    _marker: PhantomData<B>,
}

impl<T, B> Res<T, B>
where
    T: DeserializeOwned,
    B: GuestWorldResourceBinding,
{
    pub fn new(inner: B) -> Self {
        let value = serde_json::from_slice(&inner.get()).expect("resource to deserialize");
        Self {
            value,
            _marker: PhantomData,
        }
    }
}

impl<T, B> Deref for Res<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub struct ResMut<T: Serialize, B: GuestWorldResourceBinding> {
    inner: B,
    value: T,
}

impl<T, B> ResMut<T, B>
where
    T: Serialize + DeserializeOwned,
    B: GuestWorldResourceBinding,
{
    pub fn new(inner: B) -> Self {
        let value = serde_json::from_slice(&inner.get()).expect("resource to deserialize");
        Self { inner, value }
    }
}

impl<T: Serialize, B: GuestWorldResourceBinding> Deref for ResMut<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: Serialize, B: GuestWorldResourceBinding> DerefMut for ResMut<T, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T: Serialize, B: GuestWorldResourceBinding> Drop for ResMut<T, B> {
    fn drop(&mut self) {
        self.inner
            .set(&serde_json::to_vec(&self.value).expect("resource to serialize"));
    }
}

pub struct Query<T, B> {
    inner: B,
    _marker: PhantomData<fn() -> T>,
}

impl<T, B> Query<T, B> {
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

pub struct GuestRef<T, B> {
    value: T,
    _marker: PhantomData<B>,
}

impl<T, B> GuestRef<T, B>
where
    T: DeserializeOwned,
    B: GuestComponentBinding,
{
    fn new(inner: B) -> Self {
        let value = serde_json::from_slice(&inner.get()).expect("component to deserialize");
        Self {
            value,
            _marker: PhantomData,
        }
    }
}

impl<T, B> Deref for GuestRef<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub struct GuestMut<T: Serialize, B: GuestComponentBinding> {
    inner: B,
    value: T,
}

impl<T, B> GuestMut<T, B>
where
    T: Serialize + DeserializeOwned,
    B: GuestComponentBinding,
{
    fn new(inner: B) -> Self {
        let value = serde_json::from_slice(&inner.get()).expect("component to deserialize");
        Self { inner, value }
    }
}

impl<T: Serialize, B: GuestComponentBinding> Deref for GuestMut<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: Serialize, B: GuestComponentBinding> DerefMut for GuestMut<T, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T: Serialize, B: GuestComponentBinding> Drop for GuestMut<T, B> {
    fn drop(&mut self) {
        self.inner
            .set(&serde_json::to_vec(&self.value).expect("component to serialize"));
    }
}

pub struct QueryRefIter<'a, T, B>
where
    B: GuestQueryBinding,
{
    query: &'a mut B,
    _marker: PhantomData<T>,
}

impl<'a, T, B> Iterator for QueryRefIter<'a, T, B>
where
    T: DeserializeOwned,
    B: GuestQueryBinding,
{
    type Item = GuestRef<T, <B::QueryResult as GuestQueryResultBinding>::Component>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.query.next()?;
        Some(GuestRef::new(result.component(0)))
    }
}

pub struct QueryMutIter<'a, T, B>
where
    B: GuestQueryBinding,
{
    query: &'a mut B,
    _marker: PhantomData<T>,
}

impl<'a, T, B> Iterator for QueryMutIter<'a, T, B>
where
    T: Serialize + DeserializeOwned,
    B: GuestQueryBinding,
{
    type Item = GuestMut<T, <B::QueryResult as GuestQueryResultBinding>::Component>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.query.next()?;
        Some(GuestMut::new(result.component(0)))
    }
}

impl<'a, T, B> IntoIterator for &'a mut Query<&T, B>
where
    T: DeserializeOwned,
    B: GuestQueryBinding,
{
    type Item = GuestRef<T, <B::QueryResult as GuestQueryResultBinding>::Component>;
    type IntoIter = QueryRefIter<'a, T, B>;

    fn into_iter(self) -> Self::IntoIter {
        QueryRefIter {
            query: &mut self.inner,
            _marker: PhantomData,
        }
    }
}

impl<'a, T, B> IntoIterator for &'a mut Query<&mut T, B>
where
    T: Serialize + DeserializeOwned,
    B: GuestQueryBinding,
{
    type Item = GuestMut<T, <B::QueryResult as GuestQueryResultBinding>::Component>;
    type IntoIter = QueryMutIter<'a, T, B>;

    fn into_iter(self) -> Self::IntoIter {
        QueryMutIter {
            query: &mut self.inner,
            _marker: PhantomData,
        }
    }
}
