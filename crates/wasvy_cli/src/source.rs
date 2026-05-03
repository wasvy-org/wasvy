use std::{
    borrow::Cow,
    fmt::{self},
    fs,
    mem::replace,
    path::{Path, PathBuf},
    process::Stdio,
};

use crate::{fs::WriteTo, id::Id, language::BoxedLanguage, named::Named, runtime::Runtime};

use anyhow::{Context, Result, anyhow, bail, ensure};
use wit_parser::{Package, PackageId, Resolve, UnresolvedPackageGroup, World};

/// A source
#[derive(Clone)]
pub struct Source {
    name: Option<String>,
    path: PathBuf,
    language: Option<Id>,
    resolve: Resolve,
    package: PackageId,
    runtime: Runtime,
}

impl Source {
    /// Identifies a path as a compatible [Source] (build files) for a Mod
    pub fn identify(path: impl AsRef<Path>, runtime: &Runtime) -> Result<Self> {
        let path = path.as_ref();
        if path.is_file() && path.extension().unwrap_or_default() == "wasm" {
            Self::identify_file(path, runtime)
        } else if path.is_dir() {
            Self::identify_dir(path, runtime)
        } else {
            Err(anyhow!("path is neither a file nor a directory"))
        }
        .with_context(|| format!("path = {path:?}"))
    }

    /// Identifies a wasm file as a compatible [Source] for a Mod
    pub fn identify_file(path: impl AsRef<Path>, runtime: &Runtime) -> Result<Self> {
        let path = path.as_ref();

        let mut resolve = runtime.resolve().clone();
        let package = resolve
            .push_file(path)
            .context("failed to resolve wasm file")?;

        Self::new_raw(None, path, runtime, None, resolve, package)
    }

    /// Identifies a directory as a compatible [Source] (build files) for a Mod
    pub fn identify_dir(path: impl AsRef<Path>, runtime: &Runtime) -> Result<Self> {
        let path = path.as_ref();

        let mut resolve = runtime.resolve().clone();

        let wit_path = path.join("wit");
        let top_pkg = UnresolvedPackageGroup::parse_dir(&wit_path)
            .with_context(|| format!("failed to parse packages: {:?}", wit_path.join("*.wit")))?;

        let span_offset = resolve.push_source_map(top_pkg.source_map);
        let package = resolve
            .push(top_pkg.main, span_offset)
            .context("failed to resolve path")?;

        // Try validating different languages until one matches
        let Some((id, info)) = runtime
            .languages()
            .iter()
            .find_map(|(id, language)| language.identify(path).ok().map(|info| (id, info)))
        else {
            bail!("path was not identified as any language");
        };

        Source::new_raw(info.name, path, runtime, Some(id.clone()), resolve, package)
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

    // Returns the boxed language implementation
    pub fn language(&self) -> Option<&BoxedLanguage> {
        self.language.as_ref().map(|language| {
            self.runtime()
                .languages()
                .get(language)
                .expect("language exists in source")
        })
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

    /// Refresh the wit deps from the filesystem
    pub fn refresh(&mut self) -> Result<()> {
        if self.language.is_some() {
            let src = Self::identify_dir(self.path(), self.runtime())
                .context("identifying exisitng source directory")?;
            let _ = replace(self, src);
        }

        Ok(())
    }

    /// Updates the wit deps, overwriting those already there
    ///
    /// Make sure to [Self::refresh] the source after calling this since it might be invalid
    pub fn update_deps(&self) -> Result<()> {
        if self.language.is_some() {
            let wit_path = self.path.join("wit");
            let deps_path = wit_path.join("deps");

            fs::create_dir_all(&deps_path)?;
            for dependency in self.runtime.dependencies() {
                dependency.write(&deps_path)?;
            }
        }

        Ok(())
    }

    /// Builds the source, producing a new Wasm source
    pub fn build(&self, stdio: Stdio) -> Result<Cow<'_, Source>> {
        if let Some(language) = self.language() {
            let source = language
                .build(self, stdio)
                .with_context(|| format!("building with language {}", language.name()))?;

            Ok(Cow::Owned(source))
        } else {
            Ok(Cow::Borrowed(self))
        }
    }

    /// Creates a new source (project/build files) at the specified directory, using the language of choice
    pub(crate) fn create(
        name: impl AsRef<str>,
        path: impl AsRef<Path>,
        runtime: &Runtime,
        language: Id,
    ) -> Result<Self> {
        let boxed_language = runtime
            .languages()
            .get(&language)
            .expect("language belongs to runtime");
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

        boxed_language
            .create(&source)
            .context("generating source")?;
        source.update_deps()?;
        source.refresh()?;

        // Ensure package is no longer invalid
        debug_assert!(source.resolve.packages.get(source.package).is_some());

        Ok(source)
    }

    pub(crate) fn new_raw(
        name: Option<String>,
        path: impl AsRef<Path>,
        runtime: &Runtime,
        language: Option<Id>,
        resolve: Resolve,
        package: PackageId,
    ) -> Result<Self> {
        let path = path.as_ref();

        // Root package must have a single guest world
        let world = get_world(&resolve, package).ok_or(anyhow!("package is missing world"))?;

        // We expect that any includes in this world are fully resolved
        // Otherwise we know the wit for this source would not work with the game
        ensure!(
            world.includes.iter().len() == 0,
            "world includes are fully resolved"
        );

        // TODO: Are there other checks that we should perform?

        Ok(Self {
            name,
            path: path.to_path_buf(),
            language,
            resolve,
            package,
            runtime: runtime.clone(),
        })
    }
}

impl Named for Source {
    fn name(&self) -> &str {
        self.name
            .as_ref()
            .unwrap_or_else(|| &self.resolve.packages[self.package].name.name)
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

    use crate::{
        language::{Language, SourceInfo},
        runtime::Config,
    };

    use super::*;

    struct MockLang {
        identify: bool,
    }
    impl Language for MockLang {
        fn identify(&self, _path: &Path) -> Result<SourceInfo> {
            if self.identify {
                Ok(SourceInfo::default())
            } else {
                Err(anyhow!("nothing"))
            }
        }

        fn create(&self, _source: &Source) -> Result<()> {
            unreachable!()
        }

        fn build(&self, _source: &Source, _stdio: Stdio) -> Result<Source> {
            unreachable!()
        }
    }

    fn runtime(lang: MockLang) -> Runtime {
        let mut config = Config::empty();
        config
            .add_dependency(include_str!("../../../wit/wasvy-ecs.wit"))
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
    fn identify_basic() {
        let builder = runtime(MockLang { identify: true });
        let source = Source::identify("../../examples/mods/rust/basic", &builder)
            .expect("Should identify the basic example as a valid source");
        assert_eq!(&source.path, Path::new("../../examples/mods/rust/basic"));
        assert!(source.language.is_some());
        assert_eq!(source.world_name(), "component:basic/example");
    }

    #[test]
    fn identify_basic_without_deps() {
        let builder = runtime(MockLang { identify: true });

        // Make a test directory just containing guest.wit without deps
        let target = artifact_path("basic_without_deps");
        fs::create_dir(target.join("wit")).expect("create wit directory");
        fs::copy(
            "../../examples/mods/rust/basic/wit/guest.wit",
            target.join("wit/guest.wit"),
        )
        .expect("copy wit file");

        let source = Source::identify(target, &builder)
            .expect("Should identify basic_without_deps as a valid source");
        assert_eq!(source.world_name(), "component:basic/example");
    }

    #[test]
    fn identify_invalid_dir() {
        let builder = runtime(MockLang { identify: true });
        Source::identify("../../examples/host_example", &builder).expect_err("no wit folder");
    }

    #[test]
    fn identify_lang_fail() {
        let builder = runtime(MockLang { identify: false });
        Source::identify("../../examples/mods/rust/basic", &builder)
            .expect_err("root was not identified as any language");
    }
}
