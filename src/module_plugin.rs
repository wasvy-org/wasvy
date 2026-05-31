//! Plugin scaffolding for the **Wasvy Modules** product surface.
//!
//! This is intentionally separate from [`crate::plugin::ModLoaderPlugin`].
//! Phase 1 installs the shared runtime substrate plus the module-specific
//! reload pipeline while leaving the public Mod workflow intact.

use crate::authoring::Plugin;

/// Marker trait for generated native adapter plugins in the Wasvy Modules surface.
pub trait NativeAdapterPlugin: Plugin {}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct WasvyWorkspacePlugin;

#[cfg(target_arch = "wasm32")]
impl WasvyWorkspacePlugin {
    pub fn new(_path: impl Into<std::path::PathBuf>) -> Self {
        Self
    }

    pub fn with_modules<I, S>(self, _modules: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self
    }

    pub fn without_auto_spawn(self) -> Self {
        self
    }
}

#[cfg(target_arch = "wasm32")]
impl Plugin for WasvyWorkspacePlugin {}

#[cfg(not(target_arch = "wasm32"))]
mod host_impl {
    use std::path::PathBuf;

    use bevy_app::prelude::*;
    use bevy_asset::prelude::*;
    use bevy_ecs::{
        prelude::*,
        reflect::{AppFunctionRegistry, AppTypeRegistry},
    };
    use bevy_log::warn;

    use crate::{
        asset::{ModAsset, ModAssetLoader},
        authoring::AutoRegistrationPlugin,
        component::WasmComponentRegistry,
        engine::{Engine, create_linker},
        methods::FunctionIndex,
        mods::ModDespawnBehaviour,
        module_reload::{
            DisableModuleSystemSet, ModuleGenerationCounter, ModuleReloadQueue,
            disable_module_system_sets, run_module_reload,
        },
        modules::ModuleId,
        resource::WasmResourceRegistry,
        sandbox::Sandboxed,
        schedule::{ModSchedules, ModStartup},
        serialize::CodecResource,
        workspace::{
            WorkspaceConfigPath, WorkspaceInventory, WorldComposition, parse_workspace_manifest,
        },
    };

    /// Workspace-oriented entry point for the Wasvy Modules product surface.
    #[derive(Default)]
    pub struct WasvyWorkspacePlugin {
        config_path: Option<PathBuf>,
        requested_modules: Vec<ModuleId>,
        auto_spawn: bool,
    }

    impl WasvyWorkspacePlugin {
        pub fn new(path: impl Into<PathBuf>) -> Self {
            Self {
                config_path: Some(path.into()),
                requested_modules: Vec::new(),
                auto_spawn: true,
            }
        }

        pub fn with_modules<I, S>(mut self, modules: I) -> Self
        where
            I: IntoIterator<Item = S>,
            S: Into<ModuleId>,
        {
            self.requested_modules = modules.into_iter().map(Into::into).collect();
            self
        }

        pub fn without_auto_spawn(mut self) -> Self {
            self.auto_spawn = false;
            self
        }
    }

    impl Plugin for WasvyWorkspacePlugin {
        fn build(&self, app: &mut App) {
            let engine = Engine::new();
            let linker = create_linker(&engine);

            app.init_asset::<ModAsset>()
                .register_asset_loader(ModAssetLoader { linker })
                .insert_resource(engine)
                .insert_resource(ModDespawnBehaviour::None)
                .insert_resource(CodecResource::default())
                .init_resource::<WasmComponentRegistry>()
                .init_resource::<WasmResourceRegistry>()
                .init_resource::<AppTypeRegistry>()
                .insert_resource(ModSchedules::default())
                .add_schedule(ModStartup::new_schedule())
                .init_resource::<ModuleGenerationCounter>()
                .init_resource::<ModuleReloadQueue>()
                .init_resource::<WorkspaceInventory>()
                .add_message::<DisableModuleSystemSet>()
                .add_systems(First, (run_module_reload, disable_module_system_sets))
                .add_plugins(AutoRegistrationPlugin);

            let mut parsed_inventory = None;
            let mut parsed_world = None;

            if let Some(path) = &self.config_path {
                app.insert_resource(WorkspaceConfigPath(path.clone()));
                if let Ok(manifest) = parse_workspace_manifest(path) {
                    parsed_inventory = Some(manifest.inventory.clone());
                    parsed_world = Some(manifest.default_world.clone());
                }
            }

            if let Some(inventory) = parsed_inventory {
                app.insert_resource(inventory);
            }

            if self.requested_modules.is_empty() {
                if let Some(world_composition) = parsed_world {
                    app.insert_resource(world_composition);
                } else {
                    app.init_resource::<WorldComposition>();
                }
            } else {
                app.insert_resource(WorldComposition::new(self.requested_modules.clone()));
            }

            if self.auto_spawn {
                app.add_systems(Startup, spawn_configured_world_modules);
            }

            app.world_mut().register_component::<Sandboxed>();

            app.insert_resource(FunctionIndex::build(
                app.world()
                    .get_resource::<AppTypeRegistry>()
                    .expect("AppTypeRegistry to be initialized"),
                app.world()
                    .get_resource::<AppFunctionRegistry>()
                    .expect("AppFunctionRegistry to be initialized"),
            ));
        }
    }

    fn spawn_configured_world_modules(
        inventory: Res<WorkspaceInventory>,
        composition: Res<WorldComposition>,
        mut modules: crate::modules::Modules,
    ) {
        for id in &composition.active_modules {
            if inventory.module(id).is_none() {
                warn!("world composition requested missing module `{id}`");
                continue;
            }

            modules.spawn(id.clone(), format!("modules/{}.wasm", id.as_str()));
        }
    }

    pub use WasvyWorkspacePlugin as ExportedWasvyWorkspacePlugin;
}

#[cfg(not(target_arch = "wasm32"))]
pub use host_impl::ExportedWasvyWorkspacePlugin as WasvyWorkspacePlugin;
