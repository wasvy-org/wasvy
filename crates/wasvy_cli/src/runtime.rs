use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use error_collection::Errors;
use wit_parser::Resolve;

use crate::{
    command::Logging,
    dependency::{Dependency, Interface},
    editor::BoxedEditor,
    id::Id,
    language::BoxedLanguage,
    remote::Remote,
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

type Languages = HashMap<Id, BoxedLanguage>;
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
    pub fn add_language(&mut self, language: impl Into<BoxedLanguage>) -> &mut Self {
        let language = language.into();
        let id = Id::from(&language);
        self.languages.insert(id, language);
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
            name,
            endpoint: _,
            asset_dir: _,
            dependencies,
        } = value;

        let mut config = Config {
            namespace: name.to_string(),
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

    /// Given a directory, searches its contents for compatible [Source]s (build files) for Mods
    pub fn search(&self, path: impl AsRef<Path>) -> Result<Vec<Source>> {
        // Resolve directories
        let mut sources: Vec<Source> = search_glob(path.as_ref().join("**/wit/*.wit"))
            .filter_map(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf))
            .collect::<HashSet<_>>() // Dedupe
            .into_iter()
            .filter_map(|path| Source::identify_dir(&path, None, self).ok())
            .collect();

        // Resolve wasm files
        let mut wasm = search_glob(path.as_ref().join("**/*.wasm"))
            // Ignore wasm build artifacts located in source directories (such as dest directory for python)
            .filter(|path| !sources.iter().any(|source| path.starts_with(source.path())))
            // Ignore wasm files in rust build directory (**/target/wasm32-*/*/*.wasm )
            .filter(|path| {
                let mut components = path.components().rev().skip(2);
                let target = components
                    .next()
                    .and_then(|part| part.as_os_str().to_str())
                    .map(|part| part.starts_with("wasm32-"))
                    .unwrap_or_default();
                let dir = components
                    .next()
                    .map(|part| part.as_os_str() == "target")
                    .unwrap_or_default();
                !target || !dir
            })
            .filter_map(|path| Source::identify_file(path, None, self).ok())
            .collect();

        sources.append(&mut wasm);

        Ok(sources)
    }

    /// Identifies a directory directory as a compatible [Source] (build files) for a Mod
    pub fn identify(&self, path: impl AsRef<Path>) -> Option<Source> {
        Source::identify(path, self).ok()
    }

    /// Creates a new source (project/build files) at the specified directory, using the language of choice
    pub fn create(
        &self,
        name: impl AsRef<str>,
        path: impl AsRef<Path>,
        language: Id,
        logging: Logging,
    ) -> Result<Source> {
        Source::create(name, path, self, language, logging)
    }
}

fn search_glob(pattern: PathBuf) -> impl Iterator<Item = PathBuf> {
    let pattern = pattern.to_str().expect("unicode path");
    glob::glob(pattern)
        .expect("valid glob")
        .filter_map(Result::ok)
        .filter(|path| {
            // Omit hidden directories
            !path
                .components()
                .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
        })
}
