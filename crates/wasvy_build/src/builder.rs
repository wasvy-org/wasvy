use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    slice::Iter,
    sync::Arc,
};

use anyhow::Result;
use glob::glob;
use wit_parser::Resolve;

use crate::{language::Language, source::Source};

pub struct Config {
    resolve: Resolve,
    dependencies: Vec<(PathBuf, String)>,
    languages: Vec<Box<dyn Language>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            resolve: Resolve::default(),
            dependencies: Vec::new(),
            languages: Vec::new(),
        }
    }
}

impl Config {
    /// Adds a new required dependency to our builder via a path
    pub fn add_dependency(&mut self, path: impl AsRef<Path>) -> Result<&mut Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)?;
        self.add_dependency_str(path, contents)
    }

    /// Adds a new required dependency to our builder
    pub fn add_dependency_str(
        &mut self,
        path: impl AsRef<Path>,
        contents: impl AsRef<str>,
    ) -> Result<&mut Self> {
        let path = path.as_ref();
        let contents = contents.as_ref();
        self.resolve.push_str(path, contents)?;
        self.dependencies
            .push((path.to_path_buf(), contents.to_string()));
        Ok(self)
    }

    /// Adds support for a new language
    pub fn add_language<L: Language + 'static>(&mut self, language: L) -> &mut Self {
        self.languages.push(Box::new(language));
        self
    }

    /// Produces a builder from this config
    pub fn build(self) -> Builder {
        Builder(Arc::new(self))
    }
}

/// A Wasvy builder is responsible for locating and building mods from source.
#[derive(Clone)]
pub struct Builder(Arc<Config>);

impl Builder {
    /// Returns access to the resolved wit dependencies for this builder
    pub fn dependencies(&self) -> &Resolve {
        &self.0.resolve
    }

    /// Returns an iterator over this builder's supported languages
    pub fn languages(&self) -> Iter<'_, Box<dyn Language>> {
        self.0.languages.iter()
    }

    /// Given a directory, searches its contents for compatible [Source]s (build files) for Mods
    pub fn search(&self, directory: impl AsRef<Path>) -> Result<Vec<Source>> {
        assert!(
            self.0.resolve.packages.len() > 0,
            "Builder requires one or more packages"
        );

        // Locate root directories that contain wit dependencies
        let directory = directory.as_ref();
        let wit_directory = directory.join("**/wit/*.wit");
        let pattern = wit_directory.to_str().expect("unicode path");
        let roots: HashSet<_> = glob(pattern)?
            .into_iter()
            .filter_map(Result::ok)
            .filter_map(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf))
            .collect();

        // Convert roots to sources
        let sources = roots
            .into_iter()
            .filter_map(|root| self.identify(root))
            .collect();

        Ok(sources)
    }

    /// Identifies a root directory as a compatible [Source] (build files) for a Mod
    pub fn identify(&self, root: impl AsRef<Path>) -> Option<Source> {
        Source::identify(root, self).ok()
    }
}
