use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use wit_parser::Resolve;

use crate::{
    dependency::Dependency, editor::Editor, id::Id, language::Language, named::Named,
    source::Source,
};

/// The config for a [Runtime]
///
/// Create a runtime by calling [Config::build()]
pub struct Config {
    /// A namespace, usually the name of the game/software being modded
    pub namespace: String,
    resolve: Resolve,
    dependencies: Vec<Dependency>,
    languages: Languages,
    editors: Editors,
}

type Languages = HashMap<Id, Box<dyn Language>>;
type Editors = HashMap<Id, Box<dyn Editor>>;

impl Default for Config {
    fn default() -> Self {
        Self {
            namespace: "namespace".to_string(),
            resolve: Default::default(),
            dependencies: Default::default(),
            languages: Default::default(),
            editors: Default::default(),
        }
    }
}

impl Config {
    /// Adds a new required dependency to our builder
    pub fn add_dependency(&mut self, dependency: Dependency) -> Result<&mut Self> {
        dependency.resolve(&PathBuf::new(), &mut self.resolve)?;
        self.dependencies.push(dependency);

        Ok(self)
    }

    /// Adds a new required dependency to our builder via a path
    pub fn add_dependency_path(&mut self, path: impl AsRef<Path>) -> Result<&mut Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)?;
        self.add_dependency_str(path, contents)
    }

    /// Adds a new required dependency to our builder
    pub fn add_dependency_str(
        &mut self,
        file_name: impl AsRef<Path>,
        file_contents: String,
    ) -> Result<&mut Self> {
        let file_name = file_name
            .as_ref()
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or(anyhow!("dependency path should be a file name"))?;

        let dependency =
            Dependency::new_with_resolve(&mut self.resolve, &file_name, file_contents)?;
        self.dependencies.push(dependency);

        Ok(self)
    }

    /// Adds support for a new language
    pub fn add_language<L: Language + Named + 'static>(&mut self, language: L) -> &mut Self {
        let id = Id::from(&language);
        self.languages.insert(id, Box::new(language));
        self
    }

    /// Adds support for an external editor
    pub fn add_editor<E: Editor + Named + 'static>(&mut self, editor: E) -> &mut Self {
        let id = Id::from(&editor);
        self.editors.insert(id, Box::new(editor));
        self
    }
}

/// A Wasvy Cli Runtime exposes an api for locating and building mods from source.
///
/// Start with a [BuildConfig]
#[derive(Clone)]
pub struct Runtime(Arc<Config>);

impl Runtime {
    /// Produces a runtime from a config
    pub fn new(config: Config) -> Self {
        assert!(config.languages.len() > 0, "must add languages to builder");
        Self(Arc::new(config))
    }

    /// Returns the wit namespace
    pub fn namespace(&self) -> &str {
        &self.0.namespace
    }

    /// Returns access to the resolved wit dependencies for this builder
    pub fn resolve(&self) -> &Resolve {
        &self.0.resolve
    }

    /// Returns this builder's dependencies
    pub fn dependencies(&self) -> &Vec<Dependency> {
        &self.0.dependencies
    }

    /// Finds a dependency given a namespace:name.
    ///
    /// If there are multiple matches, returns the dependency with the latest version.
    pub fn find_dependency(
        &self,
        namespace: impl AsRef<str>,
        name: impl AsRef<str>,
    ) -> Option<&Dependency> {
        self.dependencies()
            .iter()
            .filter(|dep| dep.namespace == namespace.as_ref() && dep.name == name.as_ref())
            .max_by_key(|dep| &dep.version)
    }

    /// Returns this builder's supported languages
    pub fn languages(&self) -> &Languages {
        &self.0.languages
    }

    /// Given a directory, searches its contents for compatible [Source]s (build files) for Mods
    pub fn search(&self, path: impl AsRef<Path>) -> Result<Vec<Source>> {
        let wasm_matches = search_glob(path.as_ref().join("**/*.wasm"));
        let source_matches = search_glob(path.as_ref().join("**/wit/*.wit"))
            .filter_map(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf));

        // remove duplicate paths from source_matches
        let paths: HashSet<_> = wasm_matches.chain(source_matches).collect();

        let sources = paths
            .into_iter()
            .filter_map(|path| self.identify(path))
            .collect();

        Ok(sources)
    }

    /// Identifies a directory directory as a compatible [Source] (build files) for a Mod
    pub fn identify(&self, path: impl AsRef<Path>) -> Option<Source> {
        Source::identify(path, self).ok()
    }

    /// Creates a new source (project/build files) at the specified directory, using the language of choice
    pub fn generate(
        &self,
        name: impl AsRef<str>,
        path: impl AsRef<Path>,
        language: Id,
    ) -> Result<Source> {
        Source::new(name, path, &self, language)
    }

    /// Populates the wit deps, overwriting those already there
    pub fn populate_deps(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref().join("wit/deps");

        fs::create_dir_all(&path)?;
        for dependency in self.dependencies() {
            dependency.create(&path)?;
        }

        Ok(())
    }
}

fn search_glob(pattern: PathBuf) -> impl Iterator<Item = PathBuf> {
    let pattern = pattern.to_str().expect("unicode path");
    glob::glob(pattern)
        .expect("valid glob")
        .into_iter()
        .filter_map(Result::ok)
}
