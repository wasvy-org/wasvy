//! Workspace-level scaffolding and config parsing for **Wasvy Modules**.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use bevy_ecs::prelude::*;

use crate::modules::ModuleId;

/// Placeholder config path resource for a future `wasvy.toml` parser.
#[derive(Resource, Debug, Clone)]
pub struct WorkspaceConfigPath(pub PathBuf);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManifest {
    pub root: PathBuf,
    pub host: Option<PathBuf>,
    pub api: Option<PathBuf>,
    pub inventory: WorkspaceInventory,
    pub default_world: WorldComposition,
}

/// One available module in a workspace inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceModuleEntry {
    pub id: ModuleId,
    pub path: PathBuf,
}

/// The declared set of available Modules in a game workspace.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceInventory {
    pub modules: Vec<WorkspaceModuleEntry>,
}

impl WorkspaceInventory {
    pub fn module(&self, id: &ModuleId) -> Option<&WorkspaceModuleEntry> {
        self.modules.iter().find(|entry| &entry.id == id)
    }

    pub fn insert(&mut self, entry: WorkspaceModuleEntry) {
        self.modules.push(entry);
    }

    pub fn from_manifest(path: impl AsRef<Path>) -> Result<WorkspaceManifest> {
        parse_workspace_manifest(path)
    }
}

/// The host-side selection of which workspace modules are active in one world.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldComposition {
    pub active_modules: Vec<ModuleId>,
}

impl WorldComposition {
    pub fn new<I, S>(modules: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<ModuleId>,
    {
        Self {
            active_modules: modules.into_iter().map(Into::into).collect(),
        }
    }

    pub fn includes(&self, id: &ModuleId) -> bool {
        self.active_modules.iter().any(|candidate| candidate == id)
    }

    pub fn enable(&mut self, id: impl Into<ModuleId>) {
        let id = id.into();
        if !self.includes(&id) {
            self.active_modules.push(id);
        }
    }

    pub fn disable(&mut self, id: &ModuleId) {
        self.active_modules.retain(|candidate| candidate != id);
    }
}

pub fn parse_workspace_manifest(path: impl AsRef<Path>) -> Result<WorkspaceManifest> {
    let path = path.as_ref();
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read workspace manifest at {}", path.display()))?;
    let value: toml::Value = toml::from_str(&contents)
        .with_context(|| format!("failed to parse workspace manifest at {}", path.display()))?;

    let workspace = value
        .get("workspace")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();

    let host = workspace
        .get("host")
        .and_then(toml::Value::as_str)
        .map(|path| root.join(path));
    let api = workspace
        .get("api")
        .and_then(toml::Value::as_str)
        .map(|path| root.join(path));

    let mut inventory = WorkspaceInventory::default();
    for module in value
        .get("module")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let table = module
            .as_table()
            .ok_or_else(|| anyhow!("[[module]] entries must be tables"))?;
        let name = table
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| anyhow!("[[module]] is missing required `name`"))?;
        let path = table
            .get("path")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| anyhow!("[[module]] is missing required `path`"))?;

        let id = ModuleId::new(name);
        if inventory.module(&id).is_some() {
            bail!("duplicate module `{name}` in workspace inventory");
        }

        inventory.insert(WorkspaceModuleEntry {
            id,
            path: root.join(path),
        });
    }

    let default_world = value
        .get("world")
        .and_then(toml::Value::as_table)
        .and_then(|world| world.get("modules"))
        .and_then(toml::Value::as_array)
        .map(|modules| {
            let mut composition = WorldComposition::default();
            for module in modules {
                let module = module
                    .as_str()
                    .ok_or_else(|| anyhow!("[world].modules entries must be strings"))?;
                composition.enable(module);
            }
            Ok::<_, anyhow::Error>(composition)
        })
        .transpose()?
        .unwrap_or_else(|| {
            WorldComposition::new(inventory.modules.iter().map(|entry| entry.id.clone()))
        });

    for module in &default_world.active_modules {
        if inventory.module(module).is_none() {
            bail!("world composition references unknown module `{module}`");
        }
    }

    Ok(WorkspaceManifest {
        root,
        host,
        api,
        inventory,
        default_world,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest_builds_inventory_and_default_world() {
        let dir = std::env::temp_dir().join(format!("wasvy-workspace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wasvy.toml");
        std::fs::write(
            &path,
            r#"
[workspace]
host = "crates/game_host"
api = "crates/game_api"

[[module]]
name = "combat"
path = "crates/modules/combat"

[[module]]
name = "ai"
path = "crates/modules/ai"

[world]
modules = ["combat"]
"#,
        )
        .unwrap();

        let manifest = parse_workspace_manifest(&path).unwrap();
        assert_eq!(manifest.inventory.modules.len(), 2);
        assert!(manifest.default_world.includes(&ModuleId::new("combat")));
        assert!(!manifest.default_world.includes(&ModuleId::new("ai")));
        assert_eq!(manifest.host, Some(dir.join("crates/game_host")));
    }

    #[test]
    fn parse_manifest_defaults_world_to_all_modules() {
        let dir =
            std::env::temp_dir().join(format!("wasvy-workspace-default-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wasvy.toml");
        std::fs::write(
            &path,
            r#"
[[module]]
name = "combat"
path = "crates/modules/combat"

[[module]]
name = "ai"
path = "crates/modules/ai"
"#,
        )
        .unwrap();

        let manifest = parse_workspace_manifest(&path).unwrap();
        assert!(manifest.default_world.includes(&ModuleId::new("combat")));
        assert!(manifest.default_world.includes(&ModuleId::new("ai")));
    }
}
