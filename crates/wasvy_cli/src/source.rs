use std::{
    borrow::Cow,
    fmt::{self},
    fs,
    mem::replace,
    path::{Path, PathBuf},
};

use crate::{
    command::Logging,
    fs::WriteTo,
    id::Id,
    language::BoxedLanguage,
    languages::Rust,
    named::Named,
    runtime::Runtime,
    witgen::{ScaffoldWit, Wit, WitConfig},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use error_collection::Errors;
use wit_parser::{
    Package, PackageId, Resolve, Type, UnresolvedPackageGroup, World, WorldItem, WorldKey,
    decoding::{DecodedWasm, decode},
};

/// Any valid source of code or pre-compiled wasm binary that can be loaded in Bevy via Wasvy
#[derive(Clone)]
pub struct Source {
    name: String,
    path: PathBuf,
    variant: Variant,
    resolve: Resolve,
    package: PackageId,
    runtime: Runtime,
}

#[derive(Clone, PartialEq, Eq)]
enum Variant {
    /// A mod developed via a language of choice, must be compiled to Wasm in order to run
    External { language: Id },

    /// A crate that lives in the same workspace as the host app.
    ///
    /// Can be compiled to a mod, or directly into the the app.
    Native { crate_name: String },

    /// A Wasm Mod compiled via wasi. Built from another variant type.
    Wasm,
}
use Variant::*;

impl Source {
    /// Creates a new source from an existing path.
    ///
    /// Note: This will fail if the path is missing, if you want to create a new Source in a language of choice, call [Source::scaffold]
    pub fn new(path: impl AsRef<Path>, runtime: &Runtime) -> Result<Self> {
        let path = path.as_ref();
        if path.is_file() && path.extension().unwrap_or_default() == "wasm" {
            Self::new_wasm(path, None, runtime)
        } else if path.is_dir() {
            Self::new_dir(path, runtime)
        } else if path.exists() {
            Err(anyhow!("Neither a wasm file nor a directory"))
        } else {
            Err(anyhow!("Path does not exist"))
        }
        .with_context(|| format!("source at {path:?} is not valid"))
    }

    /// Returns the path of this source
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the resolved wit definition
    pub fn resolve(&self) -> &Resolve {
        &self.resolve
    }

    /// Returns the runtime
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Returns the boxed language implementation
    pub fn language(&self) -> Option<&BoxedLanguage> {
        let language = match &self.variant {
            External { language } => Some(language.clone()),
            Native { crate_name: _ } => Some(Rust::id()),
            Wasm => None,
        };
        language.and_then(|language| {
            let language = self.runtime().languages().get(&language).map(|a| &a.0);
            debug_assert!(
                language.is_some(),
                "Source::language must exist in the Runtime it was created with"
            );
            language
        })
    }

    pub fn is_language(&self, id: &Id) -> bool {
        match &self.variant {
            External { language } => language == id,
            Native { crate_name: _ } => &Rust::id() == id,
            Wasm => false,
        }
    }

    /// Returns true when this is a wasm file
    pub fn is_wasm(&self) -> bool {
        self.variant == Wasm
    }

    /// Returns true when this is an external mod
    pub fn is_external(&self) -> bool {
        matches!(self.variant, External { .. })
    }

    /// Returns true when this is a native mod
    pub fn is_native(&self) -> bool {
        matches!(self.variant, Native { .. })
    }

    /// Returns the world at the root directory
    pub fn world(&self) -> &World {
        get_world(&self.resolve, self.package).expect("valid resolve")
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
        // A native mod does not have a name or wit dependencies that can be refreshed
        if !self.is_native() {
            let path = self.path();
            let src = Self::new(path, self.runtime())
                .with_context(|| anyhow!("Could not refresh existing source directory {path:?}"))?;
            let _ = replace(self, src);
        }

        Ok(())
    }

    /// Updates the wit deps on disk, overwriting those already there
    ///
    /// Make sure to [Self::refresh] the source after calling this since it might be invalid
    pub fn update_deps(&self) -> Result<()> {
        if self.is_wasm() {
            return Ok(());
        }

        let wit_path = self.path.join("wit");
        let deps_path = wit_path.join("deps");

        let mut errors = Errors::new();

        for dependency in self.runtime.dependencies() {
            errors.collect(
                dependency
                    .write(&deps_path)
                    .with_context(|| anyhow!("Writing dependency {deps_path:?}")),
            );
        }

        errors.as_result()
    }

    /// Builds the source, producing a new Wasm source
    pub fn build(&self, logging: Logging) -> Result<Cow<'_, Source>> {
        if let Some(language) = self.language() {
            logging.println(format!("Building {self}"));
            let result = language
                .build(self, logging.clone())
                .with_context(|| format!("Building {self} with language {}", language.name()));

            match &result {
                Ok(source) => logging.println(format!("Successfully built {source}")),
                Err(err) => logging.eprintln(format!("Error: {err:?}")),
            }

            Ok(Cow::Owned(result?))
        } else {
            Ok(Cow::Borrowed(self))
        }
    }

    /// Create a new default starter project inside the specified directory, using the language of choice
    pub fn scaffold(
        name: impl AsRef<str>,
        path: impl AsRef<Path>,
        runtime: &Runtime,
        language: Id,
        logging: Logging,
    ) -> Result<Self> {
        let (boxed_language, _) = runtime
            .languages()
            .get(&language)
            .ok_or(anyhow!("language must belong to runtime"))?;

        let wit = Wit::new(ScaffoldWit::new(&name, runtime))?;
        wit.write(&path)?;

        let mut source =
            Self::new_dir_inner(path, Some(name.as_ref().to_string()), language, runtime)?;

        let mut errors = Errors::new();

        errors.collect(
            boxed_language
                .scaffold(&source, logging)
                .context("generating source"),
        );

        errors.collect(source.update_deps());
        errors.collect(source.refresh());

        errors.as_result().map(|_| source)
    }

    /// Returns file system paths that should be watched to react to changes
    pub fn watch_paths(&self) -> Vec<PathBuf> {
        let paths = if let Some(language) = self.language() {
            language.watch_paths(self)
        } else {
            vec![self.path().to_owned()]
        };
        paths
            .iter()
            .filter_map(|path| fs::canonicalize(path).ok())
            .collect()
    }

    /// Identifies a crate in the same workspace as the app as a compatible [Source] for a Mod
    pub fn new_native(
        path: impl AsRef<Path>,
        crate_name: String,
        runtime: &Runtime,
    ) -> Result<Self> {
        let mut resolve = runtime.resolve().clone();

        let mut config: WitConfig = runtime.into();
        config.name = crate_name.clone();
        let wit = Wit::new(config)?;
        let contents: String = wit.try_into()?;

        let package = resolve.push_str("default", &contents)?;

        let name = Some(crate_name.clone());
        Self::new_inner(Native { crate_name }, name, path, runtime, resolve, package)
    }

    /// Identifies a wasm file as a compatible [Source] for a Mod
    pub(crate) fn new_wasm(
        path: impl AsRef<Path>,
        name: Option<String>,
        runtime: &Runtime,
    ) -> Result<Self> {
        let path = path.as_ref();

        let bytes = fs::read(path).with_context(|| anyhow!("Reading {path:?}"))?;

        let decoded = decode(&bytes).with_context(|| anyhow!("Decoding wit from {path:?}"))?;
        let package = decoded.package();
        let DecodedWasm::Component(resolve, _) = decoded else {
            bail!("Invalid wasm. Wasm is not a precompiled binary but a wit package.")
        };

        Self::new_inner(Wasm, name, path, runtime, resolve, package)
    }

    /// Identifies a directory as a compatible [Source] (build files) for a Mod
    pub(crate) fn new_dir(path: impl AsRef<Path>, runtime: &Runtime) -> Result<Self> {
        let path = path.as_ref();

        // Try validating different languages until one matches
        let Some((language, info)) = runtime.languages().iter().find_map(|(id, (language, _))| {
            language.identify(path).ok().map(|info| (id.clone(), info))
        }) else {
            bail!("path was not identified as any language");
        };

        Source::new_dir_inner(path, info.name, language, runtime)
    }

    fn new_dir_inner(
        path: impl AsRef<Path>,
        name: Option<String>,
        language: Id,
        runtime: &Runtime,
    ) -> Result<Self> {
        let path = path.as_ref();

        let mut resolve = runtime.resolve().clone();

        let wit_path = path.join("wit");
        let top_pkg = UnresolvedPackageGroup::parse_dir(&wit_path)
            .with_context(|| format!("failed to parse packages: {:?}", wit_path.join("*.wit")))?;

        let span_offset = resolve.push_source_map(top_pkg.source_map);
        let package = resolve
            .push(top_pkg.main, span_offset)
            .context("failed to resolve path")?;

        Source::new_inner(External { language }, name, path, runtime, resolve, package)
    }

    fn new_inner(
        variant: Variant,
        name: Option<String>,
        path: impl AsRef<Path>,
        runtime: &Runtime,
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

        // Expect minimum required exports
        let setup = WorldKey::Name("setup".into());
        if let Some(WorldItem::Function(function)) = world.exports.get(&setup) {
            ensure!(
                function.result.is_none()
                    && function.params.len() == 1
                    && matches!(function.params[0].ty, Type::Id(_)), // TODO: This is an App
                "Setup has correct signature"
            )
        } else {
            bail!("Mod must export a setup function")
        }

        // TODO: Are there other checks that we should perform?

        let Some(name) = name.or(resolve
            .packages
            .get(package)
            .map(|package| &package.name.name)
            .cloned())
        else {
            bail!("source name could not be derived from wit resolution")
        };

        Ok(Self {
            name,
            path: path.to_path_buf(),
            variant,
            resolve,
            package,
            runtime: runtime.clone(),
        })
    }
}

impl Named for Source {
    fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("name", &self.name())
            .field("path", &self.path)
            .field(
                "language",
                &self.language().map(|l| l.name()).unwrap_or("Wasm"),
            )
            .finish()
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

fn get_world(resolve: &Resolve, package: PackageId) -> Option<&World> {
    resolve
        .select_world(&[package], None)
        .ok()
        .and_then(|id| resolve.worlds.get(id))
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

        fn scaffold(&self, _source: &Source, _logging: Logging) -> Result<()> {
            unreachable!()
        }

        fn build(&self, _source: &Source, _logging: Logging) -> Result<Source> {
            unreachable!()
        }

        fn watch_paths(&self, _source: &Source) -> Vec<PathBuf> {
            unreachable!()
        }
    }

    fn mock(lang: MockLang) -> Runtime {
        let mut config = Config::empty();
        config
            .add_dependency(include_str!("../../../wit/wasvy-ecs.wit"))
            .expect("valid dep");
        config.add_language(lang, &[]);
        Runtime::new(config).expect("valid config")
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
        let runtime = mock(MockLang { identify: true });
        let source = Source::new("../../examples/mods/rust/basic", &runtime)
            .expect("Should identify the basic example as a valid source");
        assert_eq!(&source.path, Path::new("../../examples/mods/rust/basic"));
        assert!(source.language().is_some());
        assert_eq!(source.world_name(), "component:basic/example");
    }

    #[test]
    fn identify_basic_without_deps() {
        let runtime = mock(MockLang { identify: true });

        // Make a test directory just containing guest.wit without deps
        let target = artifact_path("basic_without_deps");
        fs::create_dir(target.join("wit")).expect("create wit directory");
        fs::copy(
            "../../examples/mods/rust/basic/wit/guest.wit",
            target.join("wit/guest.wit"),
        )
        .expect("copy wit file");

        let source = Source::new(target, &runtime)
            .expect("Should identify basic_without_deps as a valid source");
        assert_eq!(source.world_name(), "component:basic/example");
    }

    #[test]
    fn identify_invalid_dir() {
        let runtime = mock(MockLang { identify: true });
        Source::new("../../examples/host_example", &runtime).expect_err("no wit folder");
    }

    #[test]
    fn identify_lang_fail() {
        let runtime = mock(MockLang { identify: false });
        Source::new("../../examples/mods/rust/basic", &runtime)
            .expect_err("root was not identified as any language");
    }
}
