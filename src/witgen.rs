//! WIT generation for exported Wasvy components and methods.
//!
//! This module inspects the Bevy `TypeRegistry` + `FunctionRegistry` at runtime
//! and produces a `components.wit` description for guest bindings.
//! Argument names are sourced from `#[wasvy::methods]` metadata when available.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppFunctionRegistry;

use crate::methods::FunctionIndex;

#[derive(Resource, Clone, Debug)]
/// Settings controlling how `components.wit` is generated.
///
/// # Example
/// ```ignore
/// use wasvy::witgen::WitGeneratorSettings;
///
/// let settings = WitGeneratorSettings {
///     package: "game:components".to_string(),
///     output_path: "target/wasvy/components.wit".into(),
///     ..Default::default()
/// };
/// ```
pub struct WitGeneratorSettings {
    /// WIT package name (e.g. `game:components`).
    pub package: String,
    /// Interface name that contains the component resources.
    pub interface: String,
    /// World name that imports the interface.
    pub world: String,
    /// Package containing the `wasvy:ecs` types.
    pub wasvy_package: String,
    /// File path where the generated WIT should be written.
    pub output_path: PathBuf,
}

impl Default for WitGeneratorSettings {
    fn default() -> Self {
        Self {
            package: "game:components".to_string(),
            interface: "components".to_string(),
            world: "host".to_string(),
            wasvy_package: "wasvy:ecs".to_string(),
            output_path: PathBuf::from("target/wasvy/components.wit"),
        }
    }
}

/// Plugin that writes the generated WIT to disk at startup.
///
/// # Example
/// ```ignore
/// use bevy_app::App;
/// use wasvy::witgen::WitGeneratorPlugin;
///
/// let mut app = App::new();
/// app.add_plugins(WitGeneratorPlugin::default());
/// ```
#[derive(Default)]
pub struct WitGeneratorPlugin {
    settings: WitGeneratorSettings,
}

impl WitGeneratorPlugin {
    /// Create a plugin with the provided settings.
    pub fn new(settings: WitGeneratorSettings) -> Self {
        Self { settings }
    }
}

impl Plugin for WitGeneratorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .add_systems(Startup, write_wit);
    }
}

fn write_wit(
    settings: Res<WitGeneratorSettings>,
    type_registry: Res<AppTypeRegistry>,
    function_registry: Res<AppFunctionRegistry>,
) {
    let output = generate_wit(&settings, &type_registry, &function_registry);
    if let Some(parent) = settings.output_path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        bevy_log::error!("Failed to create WIT output dir: {err}");
        return;
    }

    if let Err(err) = fs::write(&settings.output_path, output) {
        bevy_log::error!("Failed to write WIT file: {err}");
    }
}

#[derive(Default)]
struct ComponentEntry {
    name: String,
    type_path: String,
    methods: Vec<MethodEntry>,
}

#[derive(Clone)]
struct MethodEntry {
    name: String,
    arg_names: Vec<String>,
    arg_types: Vec<String>,
    ret: String,
}

/// Build a WIT document for all exported components and methods.
///
/// Argument names are taken from `#[wasvy::methods]` metadata when available
/// and otherwise default to `argN`.
pub fn generate_wit(
    settings: &WitGeneratorSettings,
    type_registry: &AppTypeRegistry,
    function_registry: &AppFunctionRegistry,
) -> String {
    let index = FunctionIndex::build(type_registry, function_registry);
    let mut components: BTreeMap<String, ComponentEntry> = BTreeMap::new();

    for type_path in index.components() {
        let entry = components.entry(type_path.to_string()).or_default();
        entry.type_path = type_path.to_string();
        if entry.name.is_empty() {
            entry.name = type_path_to_name(type_path);
        }
    }

    for type_path in index.components() {
        for method in index.methods_for(type_path) {
            let entry = components.entry(type_path.to_string()).or_default();
            entry.methods.push(MethodEntry {
                name: method.method.clone(),
                arg_names: method.args.iter().map(|arg| arg.name.clone()).collect(),
                arg_types: method
                    .args
                    .iter()
                    .map(|arg| arg.type_path.clone())
                    .collect(),
                ret: method.ret.clone(),
            });
        }
    }

    render_wit(settings, components)
}

fn render_wit(
    settings: &WitGeneratorSettings,
    components: BTreeMap<String, ComponentEntry>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("package {};\n\n", settings.package));
    out.push_str(&format!("interface {} {{\n", settings.interface));
    out.push_str(&format!(
        "  use {}/app.{{component}};\n\n",
        settings.wasvy_package
    ));

    let mut used_names = BTreeSet::new();

    for (type_path, mut entry) in components {
        if entry.name.is_empty() {
            entry.name = type_path_to_name(&type_path);
        }
        if entry.type_path.is_empty() {
            entry.type_path = type_path.clone();
        }

        let resource_name = to_wit_ident(&entry.name, &mut used_names);
        out.push_str(&format!("  /// wasvy:type-path={}\n", entry.type_path));
        out.push_str(&format!("  resource {} {{\n", resource_name));
        out.push_str("    constructor(component: component);\n");

        for method in entry.methods {
            let signature = render_method(&method);
            out.push_str(&format!("    {};\n", signature));
        }

        out.push_str("  }\n");
    }

    out.push_str("}\n\n");
    out.push_str(&format!("world {} {{\n", settings.world));
    out.push_str(&format!("  import {};\n", settings.interface));
    out.push_str("}\n");
    out
}

fn render_method(method: &MethodEntry) -> String {
    let mut args = Vec::new();
    for (name, ty) in method.arg_names.iter().zip(method.arg_types.iter()) {
        let mapped = map_type(ty);
        args.push(format!("{}: {}", name, mapped));
    }

    let args = args.join(", ");
    let ret = map_type(&method.ret);
    if ret == "()" {
        format!("{}: func({})", method.name, args)
    } else {
        format!("{}: func({}) -> {}", method.name, args, ret)
    }
}

fn type_path_to_name(type_path: &str) -> String {
    type_path
        .rsplit("::")
        .next()
        .unwrap_or(type_path)
        .to_string()
}

fn to_wit_ident(name: &str, used: &mut BTreeSet<String>) -> String {
    let mut out = String::new();
    let mut prev_lower = false;

    for ch in name.chars() {
        if ch == '_' {
            out.push('-');
            prev_lower = false;
            continue;
        }

        if ch.is_ascii_uppercase() {
            if prev_lower {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
            prev_lower = false;
        } else {
            out.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_ascii_lowercase();
        }
    }

    if out.is_empty() {
        out.push_str("component");
    }

    let mut candidate = out.clone();
    let mut index = 1;
    while used.contains(&candidate) {
        candidate = format!("{out}-{index}");
        index += 1;
    }

    used.insert(candidate.clone());
    candidate
}

fn map_type(ty: &str) -> String {
    let ty = ty.trim();
    if ty == "()" {
        return "()".to_string();
    }

    let ty = ty.replace(' ', "");

    if let Some(inner) = strip_generic(&ty, "Option") {
        return format!("option<{}>", map_type(inner));
    }
    if let Some(inner) = strip_generic(&ty, "Vec") {
        return format!("list<{}>", map_type(inner));
    }

    match strip_path(&ty) {
        "bool" => "bool".to_string(),
        "u8" => "u8".to_string(),
        "u16" => "u16".to_string(),
        "u32" => "u32".to_string(),
        "u64" => "u64".to_string(),
        "i8" => "s8".to_string(),
        "i16" => "s16".to_string(),
        "i32" => "s32".to_string(),
        "i64" => "s64".to_string(),
        "f32" => "f32".to_string(),
        "f64" => "f64".to_string(),
        "String" | "str" => "string".to_string(),
        other => unimplemented!("Type '{other}' has no known representation in wit"),
    }
}

fn strip_path(ty: &str) -> &str {
    ty.rsplit("::").next().unwrap_or(ty)
}

fn strip_generic<'a>(ty: &'a str, name: &str) -> Option<&'a str> {
    let simple = strip_path(ty);
    if !simple.starts_with(name) {
        return None;
    }
    let start = simple.find('<')?;
    let end = simple.rfind('>')?;
    if end <= start + 1 {
        return None;
    }
    Some(&simple[start + 1..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_app::App;
    use bevy_ecs::component::Component;
    use bevy_reflect::Reflect;

    #[derive(Component, Reflect, Default)]
    struct Health {
        current: f32,
        max: f32,
    }

    impl Health {
        fn heal(&mut self, amount: f32) {
            self.current = (self.current + amount).min(self.max);
        }

        fn pct(&self) -> f32 {
            self.current / self.max
        }
    }

    #[test]
    fn generates_wit() {
        let mut app = App::new();
        app.register_type::<Health>();
        app.register_type_data::<Health, crate::authoring::WasvyExport>();
        app.register_function(Health::heal);
        app.register_function(Health::pct);

        let settings = WitGeneratorSettings::default();
        let type_registry = app
            .world()
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry");
        let function_registry = app
            .world()
            .get_resource::<AppFunctionRegistry>()
            .expect("AppFunctionRegistry");

        let output = generate_wit(&settings, type_registry, function_registry);
        let wasvy_use = "use wasvy:ecs/app.{component}";

        assert!(output.contains(wasvy_use));
        assert!(output.contains("resource health"));
        assert!(output.contains("wasvy:type-path="));
        assert!(output.contains("constructor(component: component)"));
        assert!(output.contains("heal: func(arg0: f32)"));
        assert!(output.contains("pct: func() -> f32"));
        assert!(output.contains("world host"));
    }
}
