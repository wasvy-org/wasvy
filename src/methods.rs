use anyhow::{Result, bail};
use bevy_ecs::resource::Resource;
use bevy_platform::collections::HashMap;
use bevy_reflect::{Reflect, TypePath};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

#[derive(Default, Resource)]
pub struct MethodRegistry {
    methods: HashMap<MethodKey, MethodCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MethodKey {
    type_path: String,
    method: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodAccess {
    Read,
    Write,
}

enum MethodCall {
    Read(Box<dyn Fn(&dyn Reflect, Value) -> Result<Value> + Send + Sync>),
    Write(Box<dyn Fn(&mut dyn Reflect, Value) -> Result<Value> + Send + Sync>),
}

pub enum MethodTarget<'a> {
    Read(&'a dyn Reflect),
    Write(&'a mut dyn Reflect),
}

impl MethodRegistry {
    pub fn register_method_mut<T, Args, Ret, F>(&mut self, method: impl Into<String>, f: F)
    where
        T: Reflect + TypePath + 'static,
        Args: DeserializeOwned + 'static,
        Ret: Serialize,
        F: Fn(&mut T, Args) -> Ret + Send + Sync + 'static,
    {
        let key = MethodKey {
            type_path: T::type_path().to_string(),
            method: method.into(),
        };

        let handler = move |target: &mut dyn Reflect, args: Value| -> Result<Value> {
            let target = target
                .as_any_mut()
                .downcast_mut::<T>()
                .ok_or_else(|| anyhow::anyhow!("Target does not match {}", T::type_path()))?;
            let args: Args = serde_json::from_value(args)?;
            let output = f(target, args);
            Ok(serde_json::to_value(output)?)
        };

        self.methods.insert(key, MethodCall::Write(Box::new(handler)));
    }

    pub fn register_method_ref<T, Args, Ret, F>(&mut self, method: impl Into<String>, f: F)
    where
        T: Reflect + TypePath + 'static,
        Args: DeserializeOwned + 'static,
        Ret: Serialize,
        F: Fn(&T, Args) -> Ret + Send + Sync + 'static,
    {
        let key = MethodKey {
            type_path: T::type_path().to_string(),
            method: method.into(),
        };

        let handler = move |target: &dyn Reflect, args: Value| -> Result<Value> {
            let target = target
                .as_any()
                .downcast_ref::<T>()
                .ok_or_else(|| anyhow::anyhow!("Target does not match {}", T::type_path()))?;
            let args: Args = serde_json::from_value(args)?;
            let output = f(target, args);
            Ok(serde_json::to_value(output)?)
        };

        self.methods.insert(key, MethodCall::Read(Box::new(handler)));
    }

    pub fn invoke(
        &self,
        type_path: &str,
        method: &str,
        target: MethodTarget<'_>,
        args_json: &str,
    ) -> Result<String> {
        let key = MethodKey {
            type_path: type_path.to_string(),
            method: method.to_string(),
        };
        let Some(call) = self.methods.get(&key) else {
            bail!("Unknown method {type_path}::{method}");
        };

        let args = if args_json.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(args_json)?
        };

        let result = match (call, target) {
            (MethodCall::Read(f), MethodTarget::Read(target)) => f(target, args)?,
            (MethodCall::Read(f), MethodTarget::Write(target)) => f(target, args)?,
            (MethodCall::Write(f), MethodTarget::Write(target)) => f(target, args)?,
            (MethodCall::Write(_), MethodTarget::Read(_)) => {
                bail!("Method {type_path}::{method} requires mutable access")
            }
        };

        Ok(serde_json::to_string(&result)?)
    }

    pub fn access(&self, type_path: &str, method: &str) -> Option<MethodAccess> {
        self.methods.get(&MethodKey {
            type_path: type_path.to_string(),
            method: method.to_string(),
        })
        .map(|call| match call {
            MethodCall::Read(_) => MethodAccess::Read,
            MethodCall::Write(_) => MethodAccess::Write,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_reflect::{Reflect, TypePath};

    #[derive(Reflect, Default)]
    struct Health {
        current: f32,
        max: f32,
    }

    #[test]
    fn invoke_mut_method() {
        let mut registry = MethodRegistry::default();
        registry.register_method_mut("heal", |health: &mut Health, (amount,): (f32,)| {
            health.current = (health.current + amount).min(health.max);
        });

        let mut health = Health {
            current: 2.0,
            max: 10.0,
        };

        let out = registry
            .invoke(
                Health::type_path(),
                "heal",
                MethodTarget::Write(&mut health),
                "[5.0]",
            )
            .unwrap();

        assert_eq!(out, "null");
        assert_eq!(health.current, 7.0);
    }

    #[test]
    fn invoke_read_method() {
        let mut registry = MethodRegistry::default();
        registry.register_method_ref("pct", |health: &Health, (): ()| {
            health.current / health.max
        });

        let health = Health {
            current: 2.0,
            max: 8.0,
        };

        let out = registry
            .invoke(Health::type_path(), "pct", MethodTarget::Read(&health), "null")
            .unwrap();

        assert_eq!(out, "0.25");
    }

    #[test]
    fn read_target_rejects_mut_method() {
        let mut registry = MethodRegistry::default();
        registry.register_method_mut("tick", |_health: &mut Health, (): ()| {});

        let health = Health::default();
        let err = registry
            .invoke(
                Health::type_path(),
                "tick",
                MethodTarget::Read(&health),
                "null",
            )
            .unwrap_err();

        assert!(err.to_string().contains("requires mutable"));
    }
}
