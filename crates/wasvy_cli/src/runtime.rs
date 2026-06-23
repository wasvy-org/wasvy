use std::{collections::HashMap, iter, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use error_collection::Errors;
use wit_parser::Resolve;

use crate::{
    command::Logging,
    dependency::{Dependency, Interface},
    editor::BoxedEditor,
    id::Id,
    language::BoxedLanguage,
    named::Named,
    remote::Remote,
    search::SearchBuilder,
    source::Source,
};

/// The config for a [Runtime]
///
/// Create a new runtime by calling [Runtime::new]
pub struct Config {
    /// A namespace, usually the name of the game/software being modded
    pub namespace: String,
    pub resolve: Resolve,
    pub dependencies: Vec<Dependency>,
    pub languages: Languages,
    pub editors: Editors,
}

type Languages = HashMap<Id, (BoxedLanguage, Vec<String>)>;
type Editors = HashMap<Id, BoxedEditor>;

impl Default for Config {
    fn default() -> Self {
        let mut config = Self::empty();
        config.add_all_editors();
        config.add_all_languages();
        config
    }
}

impl Config {
    /// Empty config, different than default
    pub fn empty() -> Self {
        Self {
            namespace: "my-namespace".to_string(),
            resolve: Default::default(),
            dependencies: Default::default(),
            languages: Default::default(),
            editors: Default::default(),
        }
    }

    /// Adds a new required dependency to our builder
    pub fn add_dependency(&mut self, interface: impl Into<Interface>) -> Result<&mut Self> {
        let (dep, _) = Dependency::new_with_resolve(interface, &mut self.resolve)?;
        self.dependencies.push(dep);

        Ok(self)
    }

    /// Adds support for a new language
    pub fn add_language(
        &mut self,
        language: impl Into<BoxedLanguage>,
        synonyms: &[&str],
    ) -> &mut Self {
        let language = language.into();
        let id = Id::from(&language);
        let synonyms = synonyms
            .iter()
            .chain(iter::once(&language.name()))
            .map(|synonym| synonym.to_lowercase())
            .collect();
        self.languages.insert(id, (language, synonyms));
        self
    }

    /// Adds support for an external editor
    pub fn add_editor(&mut self, editor: impl Into<BoxedEditor>) -> &mut Self {
        let editor = editor.into();
        let id = Id::from(&editor);
        self.editors.insert(id, editor);
        self
    }
}

impl TryFrom<Remote> for Config {
    type Error = anyhow::Error;

    fn try_from(value: Remote) -> Result<Self> {
        (&value).try_into()
    }
}

impl TryFrom<&Remote> for Config {
    type Error = anyhow::Error;

    fn try_from(value: &Remote) -> Result<Self> {
        let Remote {
            current_exe: _,
            name,
            endpoint: _,
            asset_dir: _,
            dependencies,
        } = value;

        let mut config = Config {
            namespace: name
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect(),
            ..Default::default()
        };

        let mut errors = Errors::new();
        for dep in dependencies.iter() {
            errors.collect(config.add_dependency(dep));
        }

        errors
            .as_result()
            .with_context(|| format!("Loading remote config for \"{name}\""))
            .map(|_| config)
    }
}

/// A Wasvy Cli Runtime exposes an api for locating and building mods from source.
///
/// Start with a [Config]
#[derive(Clone)]
pub struct Runtime(Arc<Config>);

impl Runtime {
    /// Produces a runtime from a config
    pub fn new(config: impl TryInto<Config, Error = impl Into<anyhow::Error>>) -> Result<Self> {
        let config = config.try_into().map_err(Into::into)?;

        if config.languages.is_empty() {
            bail!("config requires 1 or more languages");
        }

        Ok(Self(Arc::new(config)))
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

    /// Returns this builder's supported editors
    pub fn editors(&self) -> &Editors {
        &self.0.editors
    }

    /// Searches its contents for compatible [Source]s (build files) for Mods
    ///
    /// This will locate:
    /// - Native mods in the host app workspace (Rust)
    /// - External mods located somewhere within the path (Rust, Python, Go, etc)
    /// - Pre-compiled binaries located somewhere within the path (Wasm)
    pub fn search(&self, remote: &Remote, path: impl AsRef<Path>) -> Result<Vec<Source>> {
        SearchBuilder::new(self)
            .native(&remote.current_exe)
            .dir(path.as_ref())
            .wasm(path.as_ref())
            .search()
    }

    /// Identifies a directory directory as a compatible [Source] (build files) for a Mod
    pub fn identify(&self, path: impl AsRef<Path>) -> Option<Source> {
        Source::new(path, self).ok()
    }

    /// Creates a new source (project/build files) at the specified directory, using the language of choice
    pub fn scaffold(
        &self,
        name: impl AsRef<str>,
        path: impl AsRef<Path>,
        language: Id,
        logging: Logging,
    ) -> Result<Source> {
        Source::scaffold(name, path, self, language, logging)
    }
}
