use anyhow::{Result, anyhow};
use bevy_asset::{AssetLoader, LoadContext, io::Reader};
use bevy_reflect::TypePath;
use wasmtime::{
    component::{Component, InstancePre, Val},
    error::Context,
};
use wasvy_runtime::asset::{ModAsset, ModDiscovery};

use crate::{
    engine::Linker,
    host::{WasmApp, WasmHost},
    runner::{Config, ConfigSetup, Runner},
    system::AddSystems,
};

fn discover(instance_pre: &InstancePre<WasmHost>) -> ModDiscovery {
    let mut runner = Runner::new(instance_pre.engine());

    let mut systems = AddSystems::default();
    let config = Config::Setup(ConfigSetup {
        add_systems: &mut systems,
    });

    let mut errors = error_collection::Errors::new();
    let Some(app) = errors.collect(runner.new_resource(WasmApp)) else {
        return ModDiscovery {
            systems: Vec::new(),
            errors,
        };
    };
    errors.append(call(
        &mut runner,
        instance_pre,
        config,
        "setup",
        &[Val::Resource(app)],
        &mut [],
    ));

    let mut discovery = systems.definitions(runner.table(), instance_pre);
    discovery.errors.append(errors);
    discovery
}

pub(crate) fn call(
    runner: &mut Runner,
    instance_pre: &InstancePre<WasmHost>,
    config: Config,
    name: &str,
    params: &[Val],
    results: &mut [Val],
) -> Result<()> {
    runner.use_store(config, move |mut store| {
        let instance = Context::context(
            instance_pre.instantiate(&mut store),
            "Failed to instantiate component",
        )?;

        let func = instance
            .get_func(&mut store, name)
            .ok_or(anyhow!("Missing {name} function"))?;

        Context::context(
            func.call(&mut store, params, results),
            "Failed to run the desired wasm function",
        )?;

        Ok(())
    })
}

/// A Bevy [`AssetLoader`] for wasm files resolving to a [`ModAsset`]s values.
#[derive(TypePath)]
pub struct WasmAssetLoader {
    pub(crate) linker: Linker,
}

impl WasmAssetLoader {
    pub fn new(linker: Linker) -> Self {
        Self { linker }
    }
}

impl AssetLoader for WasmAssetLoader {
    type Asset = ModAsset;
    type Settings = ();
    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset> {
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).await?;
        let component = Component::from_binary(self.linker.engine(), &bytes)?;
        let instance_pre = self.linker.instantiate_pre(&component)?;
        Ok(ModAsset::new(discover(&instance_pre)))
    }

    fn extensions(&self) -> &[&str] {
        &["wasm"]
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command, sync::OnceLock};

    use bevy_app::App;
    use bevy_asset::{AssetPlugin, Assets};
    use bevy_ecs::{prelude::*, query::FilteredAccess, system::BoxedSystem};
    use wasvy_runtime::{
        access::ModAccess,
        mods::Mod,
        plugin::ModRuntimePlugin,
        schedule::ModScheduleName,
        system::{ModSystemBuild, ModSystemContext},
    };

    use super::{ModAsset, discover};
    use crate::{
        WasmLoaderPlugin,
        engine::{Engine, create_linker},
    };

    fn build_guest(package: &str, feature: Option<&str>) -> Vec<u8> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = root.join("target");
        let mut command = Command::new(env!("CARGO"));
        command
            .current_dir(&root)
            .args([
                "build",
                "--offline",
                "--target",
                "wasm32-wasip2",
                "-p",
                package,
                "--target-dir",
            ])
            .arg(&target);
        if let Some(feature) = feature {
            command.args(["--features", feature]);
        }
        let output = command.output().expect("cargo builds guest fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .join("wasm32-wasip2/debug")
                .join(format!("{package}.wasm")),
        )
        .unwrap()
    }

    fn fixture(fail: bool) -> &'static [u8] {
        static BYTES: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();
        let bytes = BYTES.get_or_init(|| {
            (
                build_guest("wasvy_loader_fixture", None),
                build_guest("wasvy_loader_fixture", Some("fail-setup")),
            )
        });
        if fail { &bytes.1 } else { &bytes.0 }
    }

    fn instance(
        bytes: &[u8],
        engine: &Engine,
    ) -> wasmtime::component::InstancePre<crate::host::WasmHost> {
        let linker = create_linker(engine);
        let component = wasmtime::component::Component::from_binary(engine.inner(), bytes).unwrap();
        linker.instantiate_pre(&component).unwrap()
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            bevy_app::TaskPoolPlugin::default(),
            AssetPlugin::default(),
            ModRuntimePlugin::default(),
            WasmLoaderPlugin::default(),
        ));
        app.finish();
        app.cleanup();
        app
    }

    #[test]
    fn discovery_metadata() {
        let app = app();
        let instance = instance(fixture(false), app.world().resource::<Engine>());
        let discovery = discover(&instance);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        let definitions = discovery.systems;
        drop(instance);
        assert_eq!(definitions.len(), 2);
        assert_eq!(definitions[0].name.as_str(), "first");
        assert_eq!(definitions[1].name.as_str(), "second");
        assert_eq!(definitions[1].schedules, [ModScheduleName::Update].into());
        assert_eq!(definitions[0].before, ["second".into()].into());
        assert!(definitions[0].after.is_empty());
        assert_eq!(definitions[1].after, ["first".into()].into());
        assert!(definitions[1].before.is_empty());
        assert_eq!(
            definitions[0].schedules,
            [
                ModScheduleName::Update,
                ModScheduleName::ModStartup,
                ModScheduleName::Custom("custom".into())
            ]
            .into()
        );
    }

    #[test]
    fn reusable_definitions() {
        let mut app = app();
        let definitions = {
            let instance = instance(fixture(false), app.world().resource::<Engine>());
            discover(&instance).systems
        };
        for definition in definitions {
            for _ in 0..2 {
                let mut system: BoxedSystem = definition
                    .build(ModSystemContext {
                        world: app.world_mut(),
                        mod_name: "renamed",
                        access: FilteredAccess::matches_everything(),
                    })
                    .unwrap();
                system.initialize(app.world_mut());
                let count = app.world().entities().count_spawned();
                system.run((), app.world_mut()).unwrap();
                system.apply_deferred(app.world_mut());
                assert_eq!(app.world().entities().count_spawned(), count + 1);
            }
        }
    }

    #[test]
    fn partial_discovery() {
        let engine = Engine::default();
        let instance = instance(fixture(true), &engine);
        let discovery = discover(&instance);

        assert_eq!(discovery.systems.len(), 1);
        assert_eq!(discovery.systems[0].name.as_str(), "first");
        assert_eq!(discovery.errors.len(), 1);
    }

    #[test]
    fn partial_setup_runs() {
        #[derive(Resource, Default)]
        struct BeforeUpdate(Vec<usize>);

        let mut app = app();
        app.init_resource::<BeforeUpdate>();
        app.add_systems(
            bevy_app::PreUpdate,
            |owned: Query<&wasvy_runtime::cleanup::DespawnModEntities>,
             mut counts: ResMut<BeforeUpdate>| {
                counts.0.push(owned.single().unwrap().len());
            },
        );
        let instance = instance(fixture(true), app.world().resource::<Engine>());
        let asset = app
            .world_mut()
            .resource_mut::<Assets<ModAsset>>()
            .add(ModAsset::new(discover(&instance)));
        let mut mod_component = Mod::new(asset);
        mod_component.enable_access(ModAccess::World);
        let mod_id = app.world_mut().spawn(mod_component).id();

        app.update();

        let spawned = app
            .world()
            .get::<wasvy_runtime::cleanup::DespawnModEntities>(mod_id)
            .unwrap();
        assert_eq!(
            spawned.iter().count(),
            2,
            "one startup copy and one update copy ran despite the setup trap"
        );
        assert_eq!(app.world().resource::<BeforeUpdate>().0, [1]);
        app.update();
        assert_eq!(app.world().resource::<BeforeUpdate>().0, [1, 2]);
        assert_eq!(
            app.world()
                .get::<wasvy_runtime::cleanup::DespawnModEntities>(mod_id)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn wasm_spawn_cleanup() {
        use bevy_ecs::relationship::Relationship;
        let mut app = app();
        let instance = instance(fixture(false), app.world().resource::<Engine>());
        let asset = app
            .world_mut()
            .resource_mut::<Assets<ModAsset>>()
            .add(ModAsset::new(discover(&instance)));
        let sandbox = wasvy_runtime::sandbox::Sandbox::new(app.world_mut(), Default::default());
        let sandbox = app.world_mut().spawn(sandbox).id();
        let mut module = Mod::new(asset);
        module.enable_access(ModAccess::Sandbox(sandbox));
        let owner = app.world_mut().spawn(module).id();
        app.update();

        let spawned = app
            .world()
            .get::<wasvy_runtime::cleanup::DespawnModEntities>(owner)
            .unwrap();
        let entities: Vec<_> = spawned.iter().collect();
        assert_eq!(entities.len(), 3);
        for entity in &entities {
            assert_eq!(
                app.world().get::<ChildOf>(*entity).unwrap().parent(),
                sandbox
            );
            assert_eq!(
                app.world()
                    .get::<wasvy_runtime::sandbox::Sandboxed>(*entity)
                    .unwrap()
                    .get(),
                sandbox
            );
        }
        app.world_mut().despawn(owner);
        for entity in entities {
            assert!(app.world().get_entity(entity).is_err());
        }
    }

    #[test]
    fn basic_example() {
        #[derive(Component, bevy_reflect::Reflect)]
        #[reflect(Component)]
        #[type_path = "basic_example_app"]
        struct MyMarker;

        let bytes = build_guest("basic_example_mod", None);
        let mut app = app();
        app.register_type::<bevy_transform::components::Transform>();
        app.register_type::<MyMarker>();
        let transform = bevy_transform::components::Transform::default();
        let initial_rotation = transform.rotation;
        let entity = app.world_mut().spawn((transform, MyMarker)).id();
        let instance = instance(&bytes, app.world().resource::<Engine>());
        let asset = app
            .world_mut()
            .resource_mut::<Assets<ModAsset>>()
            .add(ModAsset::new(discover(&instance)));
        let mut mod_component = Mod::new(asset);
        mod_component.enable_access(ModAccess::World);
        app.world_mut().spawn(mod_component);
        app.update();
        assert_ne!(
            app.world()
                .get::<bevy_transform::components::Transform>(entity)
                .unwrap()
                .rotation,
            initial_rotation
        );
        let mut query = app
            .world_mut()
            .query::<&wasvy_runtime::cleanup::DespawnModEntities>();
        assert_eq!(query.single(app.world()).unwrap().iter().count(), 1);
    }
}
