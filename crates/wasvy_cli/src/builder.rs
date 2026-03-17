use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs,
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow};
use askama::Template;
use glob::glob;
use wit_parser::Resolve;

use crate::{
    dependency::Dependency,
    language::{Language, LanguageId},
    source::Source,
};

type Languages = HashMap<LanguageId, Box<dyn Language>>;

/// A config, whic
pub struct Config {
    pub namespace: String,
    resolve: Resolve,
    dependencies: Vec<Dependency>,
    languages: Languages,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            namespace: "wasvy".to_string(),
            resolve: Resolve::default(),
            dependencies: Vec::new(),
            languages: HashMap::new(),
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
        file_name: impl AsRef<Path>,
        file_contents: String,
    ) -> Result<&mut Self> {
        let file_name = file_name
            .as_ref()
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or(anyhow!("dependency path should be a file name"))?;

        let dependency = Dependency::new(&mut self.resolve, &file_name, file_contents)?;
        self.dependencies.push(dependency);

        Ok(self)
    }

    /// Adds support for a new language
    pub fn add_language<L: Language + 'static>(&mut self, language: L) -> &mut Self {
        let id = LanguageId::new(&language);
        self.languages.insert(id, Box::new(language));
        self
    }

    /// Produces a builder from this config
    pub fn build(self) -> Builder {
        assert!(
            self.resolve.packages.len() > 0,
            "must add dependencies to builder"
        );
        assert!(self.languages.len() > 0, "must add languages to builder");
        Builder(Arc::new(self))
    }
}

/// A ModBuilder exposes an api for locating and building mods from source.
///
/// Start with a [BuildConfig]
#[derive(Clone)]
pub struct Builder(Arc<Config>);

impl Builder {
    /// Returns access to the resolved wit dependencies for this builder
    pub fn resolve(&self) -> &Resolve {
        &self.0.resolve
    }

    /// Returns this builder's dependencies
    pub fn dependencies(&self) -> &Vec<Dependency> {
        &self.0.dependencies
    }

    /// Returns this builder's supported languages
    pub fn languages(&self) -> &Languages {
        &self.0.languages
    }

    /// Given a directory, searches its contents for compatible [Source]s (build files) for Mods
    pub fn search(&self, directory: impl AsRef<Path>) -> Result<Vec<Source>> {
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

    /// Creates a new project at the specified directory
    pub fn generate(
        &self,
        name: impl AsRef<str>,
        root: impl AsRef<Path>,
        language: LanguageId,
    ) -> Result<Source> {
        let name = name.as_ref().to_string();
        let root = root.as_ref();

        // Start building and resolving deps
        let mut resolve = self.resolve().clone();

        // First populate the deps
        self.populate_deps(root)
            .context("populate -> generate deps {root:?}")?;
        for deps in self.dependencies() {
            resolve.push_str(
                root.join("wit/deps").join(&deps.file_name),
                &deps.file_contents,
            )?;
        }

        #[derive(Template)]
        #[template(path = "./wit/guest.wit")]
        struct GuestWit<'a> {
            name: &'a str,
            namespace: &'a str,
        }

        // Next the root guest.wit file
        let file_name = format!("{}.wit", &name);
        let contents = &GuestWit {
            name: &name,
            namespace: &self.0.namespace,
        }
        .render()?;
        fs::write(root.join("wit").join(&file_name), contents)?;
        let package = resolve.push_str(&file_name, &contents)?;

        // Now create the source and generate it's contents
        let source = Source::new(Some(name), root, self, language, resolve, package)?;
        source.generate()?;

        Ok(source)
    }

    /// Populates the wit deps, overwriting those already there
    pub fn populate_deps(&self, root: impl AsRef<Path>) -> Result<()> {
        let root = root.as_ref();
        let path = root.join("wit/deps");

        fs::create_dir_all(&path)?;
        for dependency in self.dependencies() {
            dependency.create(&path)?;
        }

        Ok(())
    }
}
