//! Function registry indexing and dynamic invocation for reflected methods.
//!
//! This module builds an index over Bevy's `FunctionRegistry` so Wasvy can:
//! - generate WIT method signatures from registered functions
//! - invoke component methods dynamically via `component.invoke`
//!
//! Argument names are sourced from `#[wasvy::methods]` metadata when available,
//! and fall back to `argN` otherwise.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use bevy_ecs::prelude::Resource;
use bevy_ecs::reflect::{AppFunctionRegistry, AppTypeRegistry};
use bevy_platform::collections::HashMap;
use bevy_reflect::{
    Reflect,
    func::args::Ownership,
    func::{ArgList, DynamicFunction},
};

use crate::authoring::{WasvyExport, WasvyMethodMetadata, inventory};
use crate::serialize::CodecResource;

/// Required access for a registered function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionAccess {
    Read,
    Write,
}

/// One argument in a reflected function signature.
#[derive(Clone, Debug)]
pub struct FunctionArg {
    pub name: String,
    pub type_path: String,
    pub ownership: Ownership,
}

/// One reflected function associated with a component method.
#[derive(Clone, Debug)]
pub struct FunctionEntry {
    pub type_path: String,
    pub method: String,
    pub function_name: String,
    pub access: FunctionAccess,
    pub args: Vec<FunctionArg>,
    pub ret: String,
    pub function: DynamicFunction<'static>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FunctionKey {
    type_path: String,
    method: String,
}

/// Index of component methods derived from Bevy's `FunctionRegistry`.
///
/// This is used by WIT generation and by the host runtime to resolve
/// dynamic method calls.
#[derive(Default, Resource)]
pub struct FunctionIndex {
    entries: HashMap<FunctionKey, FunctionEntry>,
    methods_by_component: BTreeMap<String, Vec<FunctionKey>>,
    components: BTreeSet<String>,
}

/// Target used when invoking a method.
///
/// `Read` is for `&self` methods and `Write` is for `&mut self` methods.
pub enum MethodTarget<'a> {
    Read(&'a dyn Reflect),
    Write(&'a mut dyn Reflect),
}

impl FunctionIndex {
    /// Build a fresh index from the app's type and function registries.
    ///
    /// # Example
    /// ```ignore
    /// let index = FunctionIndex::build(&type_registry, &function_registry);
    /// ```
    pub fn build(type_registry: &AppTypeRegistry, function_registry: &AppFunctionRegistry) -> Self {
        let mut arg_name_overrides: HashMap<(String, String), Vec<String>> = HashMap::default();
        for entry in inventory::iter::<WasvyMethodMetadata> {
            let key = (
                normalize_type_path(entry.type_path),
                entry.method.to_string(),
            );
            let names = entry
                .arg_names
                .iter()
                .map(|name| name.to_string())
                .collect();
            arg_name_overrides.insert(key, names);
        }

        let registry = type_registry.read();
        let mut components = BTreeSet::new();
        for (registration, _) in registry.iter_with_data::<WasvyExport>() {
            components.insert(normalize_type_path(registration.type_info().type_path()));
        }

        let functions = function_registry.read();
        let mut index = Self {
            entries: HashMap::new(),
            methods_by_component: BTreeMap::new(),
            components,
        };

        for function in functions.iter() {
            let info = function.info();
            if info.is_overloaded() {
                bevy_log::warn!(
                    "Skipping overloaded function {:?}; Wasvy only supports single-signature methods",
                    info.name()
                );
                continue;
            }

            let signature = info.base();
            let args = signature.args();
            if args.is_empty() {
                continue;
            }

            let receiver = &args[0];
            let access = match receiver.ownership() {
                Ownership::Ref => FunctionAccess::Read,
                Ownership::Mut => FunctionAccess::Write,
                Ownership::Owned => {
                    bevy_log::warn!(
                        "Skipping function {:?}; first argument must be &self or &mut self",
                        info.name()
                    );
                    continue;
                }
            };

            let receiver_type_path = normalize_type_path(receiver.ty().path());
            if !index.components.contains(&receiver_type_path) {
                continue;
            }

            let name = info
                .name()
                .map(|n| n.as_ref())
                .or_else(|| signature.name().map(|n| n.as_ref()));
            let Some(name) = name else {
                bevy_log::warn!("Skipping unnamed function; register with a name");
                continue;
            };

            let method = method_from_name(name);
            if method.is_empty() {
                bevy_log::warn!("Skipping function {name:?}; unable to infer method name");
                continue;
            }

            let key = FunctionKey {
                type_path: receiver_type_path.clone(),
                method: method.to_string(),
            };

            if index.entries.contains_key(&key) {
                bevy_log::warn!(
                    "Skipping duplicate function for {}::{}",
                    receiver_type_path,
                    method
                );
                continue;
            }

            let override_key = (receiver_type_path.clone(), method.to_string());
            let override_names = arg_name_overrides.get(&override_key);
            let mut arg_specs = Vec::with_capacity(args.len().saturating_sub(1));
            for (idx, arg) in args.iter().enumerate().skip(1) {
                let name = override_names
                    .and_then(|names| names.get(idx - 1))
                    .cloned()
                    .or_else(|| arg.name().map(|n| n.to_string()))
                    .unwrap_or_else(|| format!("arg{}", idx - 1));
                let type_path = normalize_type_path(arg.ty().path());
                arg_specs.push(FunctionArg {
                    name,
                    type_path,
                    ownership: arg.ownership(),
                });
            }

            let ret = normalize_type_path(signature.return_info().ty().path());
            let entry = FunctionEntry {
                type_path: receiver_type_path.clone(),
                method: method.to_string(),
                function_name: name.to_string(),
                access,
                args: arg_specs,
                ret,
                function: function.clone(),
            };

            index.entries.insert(key.clone(), entry);
            index
                .methods_by_component
                .entry(receiver_type_path)
                .or_default()
                .push(key);
        }

        index
    }

    /// Iterate over all exported component type paths.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.components.iter().map(|s| s.as_str())
    }

    /// Iterate over all methods for a component type path.
    pub fn methods_for<'a>(&'a self, type_path: &str) -> impl Iterator<Item = &'a FunctionEntry> {
        self.methods_by_component
            .get(type_path)
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.entries.get(key))
    }

    /// Lookup a specific method entry.
    pub fn get(&self, type_path: &str, method: &str) -> Option<&FunctionEntry> {
        self.entries.get(&FunctionKey {
            type_path: type_path.to_string(),
            method: method.to_string(),
        })
    }

    /// Invoke a reflected method using JSON-encoded arguments.
    ///
    /// `params_json` must be a JSON array string. The return value is JSON.
    ///
    /// # Example
    /// ```ignore
    /// let out = index.invoke(
    ///     Health::type_path(),
    ///     "heal",
    ///     MethodTarget::Write(&mut health),
    ///     "[5.0]",
    ///     &type_registry,
    /// )?;
    /// ```
    pub fn invoke(
        &self,
        type_path: &str,
        method: &str,
        target: MethodTarget<'_>,
        params: &[u8],
        type_registry: &AppTypeRegistry,
        codec: &CodecResource,
    ) -> Result<Vec<u8>> {
        let entry = self
            .get(type_path, method)
            .ok_or_else(|| anyhow::anyhow!("Unknown method {type_path}::{method}"))?;

        if let (FunctionAccess::Write, MethodTarget::Read(_)) = (entry.access, &target) {
            bail!("Method {type_path}::{method} requires mutable access")
        }

        let type_paths = entry
            .args
            .iter()
            .map(|arg| arg.type_path.as_str())
            .collect::<Vec<_>>();

        let registry = type_registry.read();

        let mut owned_args = codec.decode_reflect_args(params, &type_paths, &registry)?;

        if owned_args.len() != entry.args.len() {
            bail!(
                "Method {type_path}::{method} expects {} args but received {}",
                entry.args.len(),
                owned_args.len()
            );
        }

        let mut arg_list = ArgList::new();
        match target {
            MethodTarget::Read(target) => arg_list.push_ref(target),
            MethodTarget::Write(target) => arg_list.push_mut(target),
        }
        for (spec, slot) in entry.args.iter().zip(owned_args.iter_mut()) {
            match spec.ownership {
                Ownership::Owned => {
                    let boxed = slot.take().expect("owned arg to exist");
                    arg_list.push_boxed(boxed);
                }
                Ownership::Ref => {
                    let boxed = slot.as_ref().expect("arg to exist");
                    arg_list.push_ref(boxed.as_ref());
                }
                Ownership::Mut => {
                    let boxed = slot.as_mut().expect("arg to exist");
                    arg_list.push_mut(boxed.as_mut());
                }
            }
        }

        let result = entry.function.call(arg_list)?;
        let output = serialize_return(result, &registry, codec)?;
        Ok(output)
    }
}

fn serialize_return(
    result: bevy_reflect::func::Return<'_>,
    registry: &bevy_reflect::TypeRegistry,
    codec: &CodecResource,
) -> Result<Vec<u8>> {
    if result.is_unit() {
        return Ok(b"null".to_vec());
    }
    match result {
        bevy_reflect::func::Return::Owned(value) => {
            Ok(codec.encode_reflect(value.as_ref(), registry)?)
        }
        bevy_reflect::func::Return::Ref(value) => Ok(codec.encode_reflect(value, registry)?),
        bevy_reflect::func::Return::Mut(value) => Ok(codec.encode_reflect(value, registry)?),
    }
}

fn method_from_name(name: &str) -> &str {
    let segment = name.rsplit("::").next().unwrap_or(name);
    segment.rsplit('.').next().unwrap_or(segment)
}

fn normalize_type_path(path: &str) -> String {
    let trimmed = path.trim();
    let stripped = if let Some(rest) = trimmed.strip_prefix("&mut ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix('&') {
        rest
    } else {
        trimmed
    };

    if let Some(rest) = stripped.strip_prefix("build_script_build::")
        && let Ok(pkg) = std::env::var("CARGO_PKG_NAME")
    {
        return format!("{pkg}::{rest}");
    }

    stripped.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasvyComponent;
    use crate::authoring::{WasvyExport, WasvyMethodMetadata, inventory};
    use crate::prelude::WasvyAutoRegistrationPlugin;
    use crate::serialize::CodecResource;
    use bevy_app::App;
    use bevy_ecs::component::Component;
    use bevy_ecs::prelude::ReflectComponent;
    use bevy_ecs::reflect::AppFunctionRegistry;
    use bevy_reflect::{Reflect, TypePath};

    #[derive(Component, Reflect, Default, WasvyComponent)]
    #[reflect(Component)]
    struct Health {
        current: f32,
        max: f32,
    }

    #[wasvy::methods]
    impl Health {
        fn heal(&mut self, amount: f32) {
            self.current = (self.current + amount).min(self.max);
        }

        fn pct(&self) -> f32 {
            self.current / self.max
        }
    }

    #[derive(Component, Reflect, Default)]
    #[reflect(Component)]
    struct BuildScriptHealth {
        current: f32,
        max: f32,
    }

    impl BuildScriptHealth {
        fn heal(&mut self, amount: f32) {
            self.current = (self.current + amount).min(self.max);
        }
    }

    #[derive(Component, Reflect, Default, WasvyComponent)]
    #[reflect(Component)]
    struct FallbackHealth {
        current: f32,
        max: f32,
    }

    impl FallbackHealth {
        fn heal(&mut self, amount: f32) {
            self.current = (self.current + amount).min(self.max);
        }
    }

    #[derive(Component, Reflect, Default, WasvyComponent)]
    #[reflect(Component)]
    struct OverloadedHealth {
        current: f32,
        max: f32,
    }

    impl OverloadedHealth {
        fn heal_i32(&mut self, amount: i32) {
            self.current = (self.current + amount as f32).min(self.max);
        }

        fn heal_f32(&mut self, amount: f32) {
            self.current = (self.current + amount).min(self.max);
        }
    }

    inventory::submit! {
        WasvyMethodMetadata {
            type_path: "build_script_build::methods::tests::BuildScriptHealth",
            method: "heal",
            arg_names: &["amount"],
        }
    }

    fn new_app() -> App {
        let mut app = App::new();
        app.add_plugins(WasvyAutoRegistrationPlugin);
        app
    }

    #[test]
    fn index_builds_and_invokes() {
        let app = new_app();

        let type_registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry");
        let function_registry = app
            .world()
            .get_resource::<AppFunctionRegistry>()
            .expect("AppFunctionRegistry");

        let codec = CodecResource::default();
        let index = FunctionIndex::build(type_registry, function_registry);
        let mut health = Health {
            current: 2.0,
            max: 10.0,
        };

        let out = index
            .invoke(
                Health::type_path(),
                "heal",
                MethodTarget::Write(&mut health),
                b"[5.0]",
                type_registry,
                &codec,
            )
            .unwrap();
        assert_eq!(out, b"null");
        assert_eq!(health.current, 7.0);

        let pct = index
            .invoke(
                Health::type_path(),
                "pct",
                MethodTarget::Read(&health),
                b"null",
                type_registry,
                &codec,
            )
            .unwrap();
        let pct_val: f32 = crate::serialize::wasvy_decode(&pct).unwrap();
        assert!((pct_val - 0.7).abs() < 1e-6);
    }

    #[test]
    fn metadata_build_script_path_normalizes() {
        let mut app = App::new();
        app.init_resource::<AppFunctionRegistry>();
        app.register_type::<BuildScriptHealth>();
        app.register_type_data::<BuildScriptHealth, WasvyExport>();
        app.register_function(BuildScriptHealth::heal);

        let type_registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry");
        let function_registry = app
            .world()
            .get_resource::<AppFunctionRegistry>()
            .expect("AppFunctionRegistry");
        let index = FunctionIndex::build(type_registry, function_registry);
        let entry = index
            .get(BuildScriptHealth::type_path(), "heal")
            .expect("heal entry");

        assert_eq!(entry.args[0].name, "amount");
    }

    #[test]
    fn build_skips_non_exported_components() {
        let mut app = App::new();
        app.init_resource::<AppFunctionRegistry>();
        app.register_type::<BuildScriptHealth>();
        app.register_function(BuildScriptHealth::heal);

        let type_registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry");
        let function_registry = app
            .world()
            .get_resource::<AppFunctionRegistry>()
            .expect("AppFunctionRegistry");
        let index = FunctionIndex::build(type_registry, function_registry);

        assert!(index.get(BuildScriptHealth::type_path(), "heal").is_none());
    }

    #[test]
    fn arg_names_fallback_to_arg_index() {
        let mut app = new_app();
        app.register_function(FallbackHealth::heal);

        let type_registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry");
        let function_registry = app
            .world()
            .get_resource::<AppFunctionRegistry>()
            .expect("AppFunctionRegistry");
        let index = FunctionIndex::build(type_registry, function_registry);

        let entry = index
            .get(FallbackHealth::type_path(), "heal")
            .expect("heal entry");

        assert_eq!(entry.args[0].name, "arg0");
    }

    #[test]
    fn build_skips_overloaded_functions() {
        use bevy_reflect::func::IntoFunction;

        let app = new_app();

        let function_registry = app
            .world()
            .get_resource::<AppFunctionRegistry>()
            .expect("AppFunctionRegistry");

        let mut func = OverloadedHealth::heal_i32
            .into_function()
            .with_name("OverloadedHealth::heal");
        func = func.with_overload(OverloadedHealth::heal_f32);

        function_registry
            .write()
            .register(func)
            .expect("register overload");

        let type_registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry");
        let index = FunctionIndex::build(type_registry, function_registry);

        assert!(index.get(OverloadedHealth::type_path(), "heal").is_none());
    }
}
