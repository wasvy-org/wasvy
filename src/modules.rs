//! Runtime types for the **Wasvy Modules** product surface.
//!
//! This module is intentionally separate from [`crate::mods`].
//! - [`crate::mods`] remains the public modding workflow for externally authored add-ons.
//! - [`crate::modules`] is the internal modular-game workflow for Rust-first runtime modules.
//!
//! Phase 0 only adds scaffolding and stable names. Runtime reload behavior is added later.

use std::fmt;

use bevy_asset::{AssetPath, AssetServer, Handle};
use bevy_ecs::{prelude::*, reflect::AppTypeRegistry, system::SystemParam};
use bevy_platform::collections::HashSet;
use bevy_reflect::{Reflect, TypeInfo};

use crate::{
    access::ModAccess, asset::ModAsset, module_reload::DisableModuleSystemSet,
    schedule::ModSchedules,
};

/// Durable runtime identity for a Wasvy Module.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Reflect)]
pub struct ModuleId(String);

impl ModuleId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for ModuleId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ModuleId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Monotonic runtime generation number for a Module artifact activation.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Reflect)]
pub struct ModuleGeneration(pub u64);

/// Why a pending reload was blocked.
#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub enum ReloadBlockedReason {
    RegistrationFailed,
    CompatibilityFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Reflect, Default)]
pub struct ModuleSchemaSnapshot {
    pub types: Vec<ModuleTypeSchema>,
}

#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub struct ModuleTypeSchema {
    pub type_path: String,
    pub fields: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub struct ModuleCompatibilityIssue {
    pub type_path: String,
    pub details: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub struct ModuleCompatibilityFailure {
    pub module_id: ModuleId,
    pub issues: Vec<ModuleCompatibilityIssue>,
}

/// High-level reload state for a Module inside one world.
#[derive(Clone, Debug, PartialEq, Eq, Reflect)]
pub enum ModuleReloadStatus {
    Active,
    Pending,
    Blocked(ReloadBlockedReason),
}

/// Internal gameplay unit for the Wasvy Modules product surface.
///
/// This is separate from [`crate::mods::Mod`]. It intentionally represents a
/// pure gameplay module with a stable [`ModuleId`] and reload generations.
#[derive(Component, Reflect)]
#[component(on_despawn = Self::on_despawn)]
pub struct Module {
    id: ModuleId,
    asset: Handle<ModAsset>,
    active_generation: Option<ModuleGeneration>,
    pending_generation: Option<ModuleGeneration>,
    reload_status: ModuleReloadStatus,
    access: HashSet<ModAccess>,
    active_schema: Option<ModuleSchemaSnapshot>,
}

impl ModuleSchemaSnapshot {
    pub fn from_type_paths(world: &World, type_paths: impl IntoIterator<Item = String>) -> Self {
        let registry = world
            .get_resource::<AppTypeRegistry>()
            .expect("AppTypeRegistry initialized")
            .read();
        let mut types = Vec::new();

        for type_path in type_paths {
            let fields = registry
                .get_with_type_path(&type_path)
                .and_then(|registration| match registration.type_info() {
                    TypeInfo::Struct(info) => Some(
                        info.field_names()
                            .iter()
                            .map(|field| (*field).to_string())
                            .collect(),
                    ),
                    TypeInfo::TupleStruct(info) => Some(
                        (0..info.field_len())
                            .map(|index| format!("#{index}"))
                            .collect(),
                    ),
                    _ => None,
                });
            types.push(ModuleTypeSchema { type_path, fields });
        }

        Self::from_type_schemas(types)
    }

    pub fn from_type_schemas(mut types: Vec<ModuleTypeSchema>) -> Self {
        types.sort_by(|a, b| a.type_path.cmp(&b.type_path));
        types.dedup_by(|a, b| a.type_path == b.type_path);
        Self { types }
    }

    pub fn diff(&self, next: &Self) -> Vec<ModuleCompatibilityIssue> {
        let mut issues = Vec::new();
        for current in &self.types {
            let Some(candidate) = next
                .types
                .iter()
                .find(|next| next.type_path == current.type_path)
            else {
                issues.push(ModuleCompatibilityIssue {
                    type_path: current.type_path.clone(),
                    details: vec!["type no longer referenced by the module".to_string()],
                });
                continue;
            };

            match (&current.fields, &candidate.fields) {
                (Some(current_fields), Some(next_fields)) => {
                    let mut details = Vec::new();
                    for field in next_fields {
                        if !current_fields.contains(field) {
                            details.push(format!("added field `{field}`"));
                        }
                    }
                    for field in current_fields {
                        if !next_fields.contains(field) {
                            details.push(format!("removed field `{field}`"));
                        }
                    }
                    if !details.is_empty() {
                        issues.push(ModuleCompatibilityIssue {
                            type_path: current.type_path.clone(),
                            details,
                        });
                    }
                }
                (Some(_), None) | (None, Some(_)) => issues.push(ModuleCompatibilityIssue {
                    type_path: current.type_path.clone(),
                    details: vec![
                        "schema visibility changed between opaque and reflected".to_string(),
                    ],
                }),
                (None, None) => {}
            }
        }

        issues
    }
}

impl std::fmt::Display for ModuleCompatibilityFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Reload blocked for module `{}`. Relaunch required to run latest code",
            self.module_id
        )?;
        for issue in &self.issues {
            write!(f, "\n- {}", issue.type_path)?;
            for detail in &issue.details {
                write!(f, "\n  - {detail}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ModuleCompatibilityFailure {}

impl Module {
    pub fn new(id: ModuleId, asset: Handle<ModAsset>) -> Self {
        let mut access = HashSet::default();
        access.insert(ModAccess::World);

        Self {
            id,
            asset,
            active_generation: None,
            pending_generation: None,
            reload_status: ModuleReloadStatus::Pending,
            access,
            active_schema: None,
        }
    }

    pub fn id(&self) -> &ModuleId {
        &self.id
    }

    pub fn asset(&self) -> Handle<ModAsset> {
        self.asset.clone()
    }

    pub fn active_generation(&self) -> Option<ModuleGeneration> {
        self.active_generation
    }

    pub fn pending_generation(&self) -> Option<ModuleGeneration> {
        self.pending_generation
    }

    pub fn reload_status(&self) -> &ModuleReloadStatus {
        &self.reload_status
    }

    pub fn active_schema(&self) -> Option<&ModuleSchemaSnapshot> {
        self.active_schema.as_ref()
    }

    pub fn set_pending_generation(&mut self, generation: ModuleGeneration) {
        self.pending_generation = Some(generation);
        self.reload_status = ModuleReloadStatus::Pending;
    }

    pub fn activate_generation(
        &mut self,
        generation: ModuleGeneration,
        schema: ModuleSchemaSnapshot,
    ) {
        self.active_generation = Some(generation);
        self.pending_generation = None;
        self.reload_status = ModuleReloadStatus::Active;
        self.active_schema = Some(schema);
    }

    pub fn block_reload(&mut self, reason: ReloadBlockedReason) {
        self.pending_generation = None;
        self.reload_status = ModuleReloadStatus::Blocked(reason);
    }

    pub fn enable_access(&mut self, access: ModAccess) -> bool {
        self.access.insert(access)
    }

    pub fn disable_access(&mut self, access: &ModAccess) -> bool {
        self.access.remove(access)
    }

    pub fn accesses(&self) -> impl Iterator<Item = &ModAccess> {
        self.access.iter()
    }

    fn on_despawn(
        mut world: bevy_ecs::world::DeferredWorld,
        ctx: bevy_ecs::lifecycle::HookContext,
    ) {
        let Some(module) = world.entity(ctx.entity).get::<Self>() else {
            return;
        };

        let module_id = module.id.clone();
        let generation = module.active_generation;
        let accesses: Vec<ModAccess> = module.access.iter().copied().collect();
        let schedules = schedules_for_accesses(&accesses, &world);

        world.commands().queue(DisableModuleSystemSet {
            set: ModuleSystemSet::Module(module_id.clone()),
            schedules: schedules.clone(),
        });

        if let Some(generation) = generation {
            world.commands().queue(DisableModuleSystemSet {
                set: ModuleSystemSet::Generation {
                    id: module_id,
                    generation,
                },
                schedules,
            });
        }
    }
}

fn schedules_for_accesses(accesses: &[ModAccess], world: &World) -> ModSchedules {
    let mut out = Vec::new();
    for access in accesses {
        for schedule in access.schedules(world).0 {
            if !out.contains(&schedule) {
                out.push(schedule);
            }
        }
    }

    ModSchedules(out)
}

/// SystemSets for systems belonging to Wasvy Modules.
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq, Default)]
pub enum ModuleSystemSet {
    #[default]
    All,
    Module(ModuleId),
    Generation {
        id: ModuleId,
        generation: ModuleGeneration,
    },
    Access(ModAccess),
}

impl ModuleSystemSet {
    pub fn module(id: impl Into<ModuleId>) -> Self {
        Self::Module(id.into())
    }

    pub fn generation(id: impl Into<ModuleId>, generation: ModuleGeneration) -> Self {
        Self::Generation {
            id: id.into(),
            generation,
        }
    }
}

/// SystemParam for Module lifecycle operations.
///
/// This mirrors [`crate::mods::Mods`] conceptually but targets Wasvy Modules.
#[derive(SystemParam)]
pub struct Modules<'w, 's> {
    commands: Commands<'w, 's>,
    asset_server: Res<'w, AssetServer>,
    modules: Query<'w, 's, Entity, With<Module>>,
}

impl Modules<'_, '_> {
    /// Spawns a new Module entity for the provided runtime identity and wasm asset path.
    pub fn spawn<'a>(&mut self, id: impl Into<ModuleId>, path: impl Into<AssetPath<'a>>) -> Entity {
        let asset = self.asset_server.load(path);
        self.commands.spawn(Module::new(id.into(), asset)).id()
    }

    /// Despawns a Module entity.
    pub fn despawn(&mut self, entity: Entity) {
        debug_assert!(self.modules.contains(entity));
        self.commands.entity(entity).despawn();
    }

    /// Despawn all currently spawned Modules.
    pub fn despawn_all(&mut self) {
        for entity in self.modules.iter() {
            self.commands.entity(entity).despawn();
        }
    }
}
