use std::{
    any::TypeId,
    fmt::Debug,
    path::{Path, PathBuf},
};

use crate::builder::Builder;

use anyhow::{Context, Result, anyhow, ensure};
use wit_parser::{PackageId, Resolve, World};

/// A source for the build
pub struct Source {
    root: PathBuf,
    language: TypeId,
    dependencies: Resolve,
    package: PackageId,
    builder: Builder,
}

impl Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Find the language name from the builder's languages collection
        let language_name = self
            .builder
            .languages()
            .find(|lang| lang.as_any().type_id() == self.language)
            .map(|lang| lang.name())
            .unwrap_or("Unknown");

        f.debug_struct("Source")
            .field("root", &self.root)
            .field("language", &language_name)
            .finish()
    }
}

impl Source {
    /// Identifies a root directory as a compatible [Source] (build files) for a Mod
    pub fn identify(root: impl AsRef<Path>, builder: &Builder) -> Result<Self> {
        let root = root.as_ref();

        // Attempt to resolve
        let mut dependencies = Resolve::default().clone();
        let mut result = push_path(&mut dependencies, root);
        if let Err(ref error) = result
            // If the error suggests the wit is just missing some dependencies,
            // attempt to resolve with the full builder deps
            && format!("{error:?}").contains("not found. known packages:")
        {
            dependencies = builder.dependencies().clone();
            result = push_path(&mut dependencies, root);
        }
        let package = result.context("failed to resolve source wit")?;

        // Root package must have a single guest world
        let world = get_world(&dependencies, package).ok_or(anyhow!("package is missing world"))?;

        // We expect that any includes in this world are fully resolved
        ensure!(
            world.includes.iter().len() == 0,
            "world includes are fully resolved"
        );

        // Try validating different languages until one matches
        for language in builder
            .languages()
            .filter(|language| language.identify(root))
        {
            return Ok(Source {
                root: root.to_path_buf(),
                language: language.as_any().type_id(),
                dependencies,
                package,
                builder: builder.clone(),
            });
        }
        Err(anyhow!("root was not identified as any language"))
    }

    /// Returns the world at the root directory
    pub fn world(&self) -> &World {
        get_world(&self.dependencies, self.package).expect("unreachable")
    }

    /// Returns the
    pub fn world_name(&self) -> String {
        let world = self.world();
        self.dependencies
            .canonicalized_id_of_name(self.package, &world.name)
    }
}

fn push_path(dependencies: &mut Resolve, root: impl AsRef<Path>) -> Result<PackageId> {
    dependencies
        .push_path(root.as_ref().join("wit"))
        .map(|(package, _)| package)
}

fn get_world(dependencies: &Resolve, package: PackageId) -> Option<&World> {
    match dependencies.select_world(&[package], None) {
        Ok(id) => dependencies.worlds.get(id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path};

    use crate::{builder::Config, language::Language};

    use super::*;

    struct MockLang {
        identify: bool,
    }
    impl Language for MockLang {
        fn identify(&self, _root: &Path) -> bool {
            self.identify
        }
    }

    fn builder(lang: MockLang) -> Builder {
        let mut config = Config::default();
        config.add_language(lang);
        assert!(config.add_dependency("../../wit/wasvy-ecs.wit").is_ok());
        config.build()
    }

    fn artifact_path(path: impl AsRef<Path>) -> PathBuf {
        let target = env::var("CARGO_TARGET_DIR").unwrap_or("../../target".to_string());
        let mut target = PathBuf::from(target);
        target.push(env!("CARGO_CRATE_NAME"));
        target.push(path.as_ref());
        let _ = fs::remove_dir_all(&target);
        fs::create_dir_all(&target).expect("create artifact directory");
        target
    }

    #[test]
    fn identify_simple() {
        let builder = builder(MockLang { identify: true });
        let source = Source::identify("../../examples/simple", &builder)
            .expect("Should identify the simple example as a valid source");
        assert_eq!(&source.root, Path::new("../../examples/simple"));
        assert_eq!(source.language, TypeId::of::<MockLang>());
        assert_eq!(source.world_name(), "component:simple/example");
    }

    #[test]
    fn identify_simple_without_deps() {
        let builder = builder(MockLang { identify: true });

        // Make a test directory just containing guest.wit without deps
        let target = artifact_path("simple_without_deps");
        fs::create_dir(target.join("wit")).expect("create wit directory");
        fs::copy(
            "../../examples/simple/wit/guest.wit",
            target.join("wit/guest.wit"),
        )
        .expect("copy wit file");

        let source = Source::identify(target, &builder)
            .expect("Should identify simple_without_deps as a valid source");
        assert_eq!(source.world_name(), "component:simple/example");
    }

    #[test]
    fn identify_invalid_dir() {
        let builder = builder(MockLang { identify: true });
        Source::identify("../../examples/host_example", &builder).expect_err("no wit folder");
    }

    #[test]
    fn identify_lang_fail() {
        let builder = builder(MockLang { identify: false });
        Source::identify("../../examples/simple", &builder)
            .expect_err("root was not identified as any language");
    }
}
