use std::{
    fmt::{self},
    fs,
    path::{Path, PathBuf},
};

use crate::{builder::Builder, language::LanguageId};

use anyhow::{Context, Result, anyhow, ensure};
use wit_parser::{PackageId, Resolve, World};

/// A source for the build
pub struct Source {
    name: Option<String>,
    root: PathBuf,
    language: LanguageId,
    resolve: Resolve,
    pub(crate) package: PackageId,
    builder: Builder,
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Find the language name from the builder's languages collection
        let language_name = self
            .builder
            .languages()
            .get(&self.language)
            .map(|lang| lang.display())
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

        // Attempt to resolve using the builder's wit resolutions
        let mut resolve = builder.resolve().clone();
        let mut result = push_path(&mut resolve, root);
        if let Err(ref error) = result
            // If the error suggests the wit is just missing some dependencies,
            // attempt to resolve with the full builder deps
            && format!("{error:?}").contains("not found. known packages:")
        {
            resolve = builder.resolve().clone();
            result = push_path(&mut resolve, root);
        }
        let package = result.context("failed to resolve source wit")?;

        // Root package must have a single guest world
        let world = get_world(&resolve, package).ok_or(anyhow!("package is missing world"))?;

        // We expect that any includes in this world are fully resolved
        ensure!(
            world.includes.iter().len() == 0,
            "world includes are fully resolved"
        );

        // Try validating different languages until one matches
        if let Some(language) = builder
            .languages()
            .values()
            .find(|language| language.identify(&root))
        {
            return Source::new(None, &root, builder, language.id(), resolve, package);
        }
        Err(anyhow!("root was not identified as any language"))
    }

    // Returns the name
    pub fn name(&self) -> &str {
        self.name
            .as_ref()
            .unwrap_or_else(|| &self.resolve.packages[self.package].name.name)
    }

    // Returns the root path
    pub fn root(&self) -> &Path {
        &self.root
    }

    // Returns the dependencies
    pub fn dependencies(&self) -> &Resolve {
        &self.resolve
    }

    // Returns the builder
    pub fn builder(&self) -> &Builder {
        &self.builder
    }

    /// Returns the world at the root directory
    pub fn world(&self) -> &World {
        get_world(&self.resolve, self.package).expect("unreachable")
    }

    /// Returns the name of the main world
    pub fn world_name(&self) -> String {
        let world = self.world();
        self.resolve
            .canonicalized_id_of_name(self.package, &world.name)
    }

    /// Populates the wit deps, overwriting those already there
    pub fn populate_deps(&self, root: impl AsRef<Path>) -> Result<()> {
        let root = root.as_ref();
        let path = root.join("wit/deps");

        fs::create_dir_all(&path)?;
        for dependency in self.builder.dependencies() {
            dependency.create(&path)?;
        }

        Ok(())
    }

    pub fn generate(&self) -> Result<()> {
        todo!()
    }

    pub(crate) fn new(
        name: Option<String>,
        root: impl AsRef<Path>,
        builder: &Builder,
        language: LanguageId,
        resolve: Resolve,
        package: PackageId,
    ) -> Result<Self> {
        let root = root.as_ref();
        let mut source = Source {
            name,
            root: root.to_path_buf(),
            language,
            resolve,
            package,
            builder: builder.clone(),
        };
        if source.name.is_none() {
            source.name = builder
                .languages()
                .get(&source.language)
                .map(|a| a.name(&source))
                .ok_or(anyhow!("builder does not implement {:?}", source.language))?;
        }

        Ok(source)
    }
}

fn push_path(resolve: &mut Resolve, root: impl AsRef<Path>) -> Result<PackageId> {
    resolve
        .push_path(root.as_ref().join("wit"))
        .map(|(package, _)| package)
}

fn get_world(resolve: &Resolve, package: PackageId) -> Option<&World> {
    match resolve.select_world(&[package], None) {
        Ok(id) => resolve.worlds.get(id),
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
        config
            .add_dependency_str(
                "wasvy-ecs.wit",
                include_str!("../../../wit/wasvy-ecs.wit").to_string(),
            )
            .expect("valid dep");
        config.add_language(lang);
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
        let lang = MockLang { identify: true };
        let lang_id = LanguageId::new(&lang);
        let builder = builder(MockLang { identify: true });
        let source = Source::identify("../../examples/simple", &builder)
            .expect("Should identify the simple example as a valid source");
        assert_eq!(&source.root, Path::new("../../examples/simple"));
        assert_eq!(source.language, lang_id);
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
