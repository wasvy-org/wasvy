use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::*;

#[derive(Clone, Debug)]
pub struct WitComponentInfo {
    pub type_path: fn() -> &'static str,
    pub name: &'static str,
}

#[derive(Clone, Debug)]
pub struct WitMethodInfo {
    pub type_path: fn() -> &'static str,
    pub name: &'static str,
    pub arg_names: &'static [&'static str],
    pub arg_types: &'static [&'static str],
    pub ret: &'static str,
    pub mutable: bool,
}

inventory::collect!(WitComponentInfo);
inventory::collect!(WitMethodInfo);

#[doc(hidden)]
#[macro_export]
macro_rules! __wasvy_submit_component {
    ($info:expr) => {
        $crate::witgen::inventory::submit! { $info }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __wasvy_submit_method {
    ($info:expr) => {
        $crate::witgen::inventory::submit! { $info }
    };
}

pub use inventory;

#[derive(Resource, Clone, Debug)]
pub struct WitGeneratorSettings {
    pub package: String,
    pub interface: String,
    pub world: String,
    pub wasvy_package: String,
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

pub struct WitGeneratorPlugin {
    settings: WitGeneratorSettings,
}

impl Default for WitGeneratorPlugin {
    fn default() -> Self {
        Self {
            settings: WitGeneratorSettings::default(),
        }
    }
}

impl WitGeneratorPlugin {
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

fn write_wit(settings: Res<WitGeneratorSettings>) {
    let output = generate_wit(&settings);
    if let Some(parent) = settings.output_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            bevy_log::error!("Failed to create WIT output dir: {err}");
            return;
        }
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

pub fn generate_wit(settings: &WitGeneratorSettings) -> String {
    let mut components: BTreeMap<String, ComponentEntry> = BTreeMap::new();

    for info in inventory::iter::<WitComponentInfo> {
        let type_path = (info.type_path)();
        let entry = components
            .entry(type_path.to_string())
            .or_insert_with(ComponentEntry::default);
        entry.name = info.name.to_string();
        entry.type_path = type_path.to_string();
    }

    for info in inventory::iter::<WitMethodInfo> {
        let type_path = (info.type_path)();
        let entry = components
            .entry(type_path.to_string())
            .or_insert_with(ComponentEntry::default);
        if entry.type_path.is_empty() {
            entry.type_path = type_path.to_string();
        }
        entry.methods.push(MethodEntry {
            name: info.name.to_string(),
            arg_names: info.arg_names.iter().map(|s| s.to_string()).collect(),
            arg_types: info.arg_types.iter().map(|s| s.to_string()).collect(),
            ret: info.ret.to_string(),
        });
    }

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
        other => {
            if other.ends_with("String") {
                "string".to_string()
            } else {
                "string".to_string()
            }
        }
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

    inventory::submit! {
        WitComponentInfo {
            type_path: health_type_path,
            name: "Health",
        }
    }

    inventory::submit! {
        WitMethodInfo {
            type_path: health_type_path,
            name: "heal",
            arg_names: &["amount"],
            arg_types: &["f32"],
            ret: "()",
            mutable: true,
        }
    }

    inventory::submit! {
        WitMethodInfo {
            type_path: health_type_path,
            name: "pct",
            arg_names: &[],
            arg_types: &[],
            ret: "f32",
            mutable: false,
        }
    }

    fn health_type_path() -> &'static str {
        "game::Health"
    }

    #[test]
    fn generates_wit() {
        let settings = WitGeneratorSettings::default();
        let output = generate_wit(&settings);
        let wasvy_use = "use wasvy:ecs/app.{component}";

        assert!(output.contains(wasvy_use));
        assert!(output.contains("resource health"));
        assert!(output.contains("wasvy:type-path=game::Health"));
        assert!(output.contains("constructor(component: component)"));
        assert!(output.contains("heal: func(amount: f32)"));
        assert!(output.contains("pct: func() -> f32"));
        assert!(output.contains("world host"));
    }
}
