use std::{
    collections::HashSet,
    fmt::{self},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    fs::WriteTo,
    id::Id,
    runtime::Runtime,
    wit::{Exports, SystemImport},
};

use anyhow::{Context, Result, anyhow, ensure};
use wit_parser::{Package, PackageId, Resolve, UnresolvedPackageGroup, World};

/// A source
pub struct Source {
    name: Option<String>,
    path: PathBuf,
    language: Option<Id>,
    resolve: Resolve,
    package: PackageId,
    runtime: Runtime,
}

impl Source {
    /// Creates a new source (project/build files) at the specified directory, using the language of choice
    pub(crate) fn new(
        name: impl AsRef<str>,
        path: impl AsRef<Path>,
        runtime: &Runtime,
        language: Id,
    ) -> Result<Self> {
        assert!(runtime.languages().contains_key(&language));

        let name = name.as_ref().to_string();
        let path = path.as_ref().to_owned();

        // Now create the source and generate it's contents
        let mut source = Self {
            name: Some(name),
            path,
            language: Some(language),
            resolve: Resolve::default(),
            package: invalid_package_id(),
            runtime: runtime.clone(),
        };
        source.generate()?;

        // Ensure package is no longer invalid
        debug_assert!(source.resolve.packages.get(source.package).is_some());

        Ok(source)
    }

    /// Identifies a path as a compatible [Source] (build files) for a Mod
    pub fn identify(path: impl AsRef<Path>, builder: &Runtime) -> Result<Self> {
        let path = path.as_ref();
        if path.is_file() && path.extension().unwrap_or_default() == "wasm" {
            Self::identify_file(path, builder)
        } else if path.is_dir() {
            Self::identify_dir(path, builder)
        } else {
            Err(anyhow!("path is neither a wasm file nor a directory"))
        }
        .with_context(|| format!("path = {path:?}"))
    }

    /// Identifies a wasm file as a compatible [Source] for a Mod
    pub fn identify_file(path: impl AsRef<Path>, builder: &Runtime) -> Result<Self> {
        let path = path.as_ref();

        let mut resolve = builder.resolve().clone();
        let package = resolve
            .push_file(path)
            .context("failed to resolve wasm file")?;

        Self::new_raw(None, path, builder, None, resolve, package)
    }

    /// Identifies a directory as a compatible [Source] (build files) for a Mod
    pub fn identify_dir(path: impl AsRef<Path>, builder: &Runtime) -> Result<Self> {
        let path = path.as_ref();

        let mut resolve = builder.resolve().clone();

        let wit_path = path.join("wit");
        let top_pkg = UnresolvedPackageGroup::parse_dir(&wit_path)
            .with_context(|| format!("failed to parse packages: {:?}", wit_path.join("*.wit")))?;

        let span_offset = resolve.push_source_map(top_pkg.source_map);
        let package = resolve
            .push(top_pkg.main, span_offset)
            .context("failed to resolve path")?;

        // Try validating different languages until one matches
        if let Some((id, _)) = builder
            .languages()
            .iter()
            .find(|(_, language)| language.identify(&path))
        {
            return Source::new_raw(None, &path, builder, Some(id.clone()), resolve, package);
        }
        Err(anyhow!("path was not identified as any language"))
    }

    // Returns the name
    pub fn name(&self) -> &str {
        self.name
            .as_ref()
            .unwrap_or_else(|| &self.resolve.packages[self.package].name.name)
    }

    // Returns the path of this source
    pub fn path(&self) -> &Path {
        &self.path
    }

    // Returns the resolved wit definition
    pub fn resolve(&self) -> &Resolve {
        &self.resolve
    }

    // Returns the runtime
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Returns the world at the root directory
    pub fn world(&self) -> &World {
        get_world(&self.resolve, self.package).expect("unreachable")
    }

    /// Returns the world at the root directory
    pub fn package(&self) -> &Package {
        &self.resolve.packages[self.package]
    }

    /// Returns the name of the main world
    pub fn world_name(&self) -> String {
        let world = self.world();
        self.resolve
            .canonicalized_id_of_name(self.package, &world.name)
    }

    /// Populates the wit deps, overwriting those already there
    pub fn populate_deps(&mut self) -> Result<()> {
        let wit_path = self.path.join("wit");
        let deps_path = wit_path.join("deps");

        fs::create_dir_all(&deps_path)?;
        for dependency in self.runtime.dependencies() {
            dependency.create(&deps_path)?;
        }

        // The resolve is no longer valid, so update it
        {
            self.resolve = self.runtime.resolve().clone(); // Already includes dependencies above

            let top_pkg = UnresolvedPackageGroup::parse_dir(&wit_path).with_context(|| {
                format!("failed to parse packages: {:?}", wit_path.join("*.wit"))
            })?;

            let span_offset = self.resolve.push_source_map(top_pkg.source_map);
            self.package = self
                .resolve
                .push(top_pkg.main, span_offset)
                .context("failed to resolve path")?;
        }

        Ok(())
    }

    /// Generates files for the source
    /// - Performs wit codegen
    /// - Generates language-specific files (will not overwite existing files)
    pub fn generate(&mut self) -> Result<()> {
        if let Some(language) = &self.language {
            let path = &self.path;
            let runtime = self.runtime.clone();
            let namespace = runtime.namespace();
            let name = self.name();
            let language = &runtime.languages()[language];
            let exports = &language.exports(self)?;
            let imports = exports
                .methods
                .iter()
                .flat_map(|a| a.args.iter())
                .map(|a| &a.import)
                .collect();
            let wasvy_wit_version = &self
                .runtime
                .find_dependency("wasvy", "ecs")
                .expect("wasvy:ecs is a dependecy of the runtime")
                .version
                .to_string();

            #[derive(askama::Template)]
            #[template(path = "./wit/guest.wit")]
            struct GuestWit<'a> {
                namespace: &'a str,
                name: &'a str,
                wasvy_wit_version: &'a str,
                imports: HashSet<&'a SystemImport>,
                exports: &'a Exports,
            }
            GuestWit {
                namespace,
                name,
                wasvy_wit_version,
                imports,
                exports,
            }
            .write(path)?;

            self.populate_deps()?;

            // Since this is an existing source, we expect errors for existing files
            let _ = language.generate(&self);
        } else {
            // Nothing to do for pre-built sources
        }
        Ok(())
    }

    pub(crate) fn new_raw(
        name: Option<String>,
        path: impl AsRef<Path>,
        runtime: &Runtime,
        language: Option<Id>,
        resolve: Resolve,
        package: PackageId,
    ) -> Result<Self> {
        // Root package must have a single guest world
        let world = get_world(&resolve, package).ok_or(anyhow!("package is missing world"))?;

        // We expect that any includes in this world are fully resolved
        // Otherwise we know the wit for this source would not work with the game
        ensure!(
            world.includes.iter().len() == 0,
            "world includes are fully resolved"
        );

        // TODO: Are there other checks that we should perform?

        let path = path.as_ref();
        let mut source = Source {
            name,
            path: path.to_path_buf(),
            language,
            resolve,
            package,
            runtime: runtime.clone(),
        };

        // Use the language implementation to try to find the name
        if source.name.is_none() {
            source.name = if let Some(language) = &source.language {
                runtime
                    .languages()
                    .get(&language)
                    .map(|a| a.name(&source))
                    .ok_or(anyhow!("builder does not implement {language:?}"))?
            } else {
                Some(source.package().name.name.clone())
            };
        }

        Ok(source)
    }

    /// A mock source for tests
    #[cfg(test)]
    pub(crate) fn mock(path: impl AsRef<Path>, runtime: Runtime, language: Id) -> Self {
        Self {
            name: None,
            path: path.as_ref().to_path_buf(),
            language: Some(language),
            resolve: Resolve::new(),
            package: invalid_package_id(),
            runtime,
        }
    }
}

/// Create a mock package id
///
/// This id will panic when used as an index
fn invalid_package_id() -> PackageId {
    // SAFETY: there's nothing unsafe, this is workaround since Id has no constructor
    unsafe { std::mem::transmute((usize::MAX, u32::MAX)) }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Source");
        debug.field("path", &self.path);
        debug.field("name", &self.name);
        if let Some(language) = &self.language {
            debug.field("language", &language);
        }
        debug.finish()
    }
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

    use crate::{language::Language, runtime::Config, wit::Exports};

    use super::*;

    struct MockLang {
        identify: bool,
    }
    impl Language for MockLang {
        fn identify(&self, _path: &Path) -> bool {
            self.identify
        }

        fn exports(&self, _source: &Source) -> Result<Exports> {
            unreachable!()
        }

        fn generate(&self, _source: &Source) -> Result<()> {
            unreachable!()
        }

        fn name(&self, _source: &Source) -> Option<String> {
            unreachable!()
        }
    }

    fn runtime(lang: MockLang) -> Runtime {
        let mut config = Config::default();
        config
            .add_dependency_str(
                "wasvy-ecs.wit",
                include_str!("../../../wit/wasvy-ecs.wit").to_string(),
            )
            .expect("valid dep");
        config.add_language(lang);
        Runtime::new(config)
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
        let lang_id = Id::from(&lang);
        let builder = runtime(MockLang { identify: true });
        let source = Source::identify("../../examples/simple", &builder)
            .expect("Should identify the simple example as a valid source");
        assert_eq!(&source.path, Path::new("../../examples/simple"));
        assert_eq!(source.language, Some(lang_id));
        assert_eq!(source.world_name(), "component:simple/example");
    }

    #[test]
    fn identify_simple_without_deps() {
        let builder = runtime(MockLang { identify: true });

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
        let builder = runtime(MockLang { identify: true });
        Source::identify("../../examples/host_example", &builder).expect_err("no wit folder");
    }

    #[test]
    fn identify_lang_fail() {
        let builder = runtime(MockLang { identify: false });
        Source::identify("../../examples/simple", &builder)
            .expect_err("root was not identified as any language");
    }
}
