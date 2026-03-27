use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use semver::Version;
use wit_parser::{PackageId, PackageName, Resolve};

#[derive(Clone)]
pub struct Dependency {
    /// Package id in [Builder::resolve]
    pub package_id: PackageId,

    /// The name of the package
    pub name: String,

    /// The namespace of the package
    pub namespace: String,

    /// The version of the package
    pub version: Version,

    /// The file name of the package
    pub file_name: PathBuf,

    /// The file contents of the package
    pub file_contents: String,
}

impl Dependency {
    pub fn new(file_name: impl AsRef<Path>, file_contents: impl AsRef<str>) -> Result<Self> {
        let mut resolve = Resolve::default();
        Self::new_with_resolve(&mut resolve, file_name, file_contents)
    }

    pub fn new_with_resolve(
        resolve: &mut Resolve,
        file_name: impl AsRef<Path>,
        file_contents: impl AsRef<str>,
    ) -> Result<Self> {
        let file_name = file_name.as_ref().to_path_buf();
        let file_contents = file_contents.as_ref().to_string();
        let package_id = resolve.push_str(&file_name, &file_contents)?;
        let package = &resolve.packages[package_id];
        let PackageName {
            name,
            namespace,
            version,
        } = package.name.clone();
        let version = version.ok_or(anyhow!("dependency {file_name:?} must have a version"))?;
        Ok(Self {
            package_id,
            name,
            namespace,
            version,
            file_name,
            file_contents,
        })
    }

    pub fn resolve(&self, path: impl AsRef<Path>, resolve: &mut Resolve) -> Result<PackageId> {
        let path = path.as_ref().join(&self.file_name);
        resolve.push_str(path, &self.file_contents)
    }

    /// Writes the contents of the dependency to the path in the disk
    pub fn create(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref().join(&self.file_name);
        fs::write(path, &self.file_contents)?;

        Ok(())
    }

    pub fn compare(&self, resolve: &Resolve) -> Option<Comparison> {
        let highest = resolve
            .packages
            .iter()
            .map(|(_, package)| &package.name)
            .filter(|package| package.namespace == self.namespace && package.name == self.name)
            .filter_map(|package| package.version.as_ref())
            .max()
            .cloned();

        highest.map(|highest| {
            if highest > self.version {
                Comparison::Ahead(highest)
            } else if highest == self.version {
                Comparison::UpToDate
            } else {
                Comparison::Outdated(highest)
            }
        })
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            namespace,
            name,
            version,
            file_name,
            ..
        } = self;
        write!(f, "{namespace}:{name}@{version} ({file_name:?})")
    }
}

/// The result of a comparison of the package () against a [Dependency]
pub enum Comparison {
    /// The package is ahead. The package is for a newer version of the app than the cli is connected to.
    Ahead(Version),

    /// The package is outdated. That means we can update it!
    Outdated(Version),

    /// The package is up-to-date. Nothing to do.
    UpToDate,
}
