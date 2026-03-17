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

    /// TODO: document each
    pub name: String,
    pub namespace: String,
    pub version: Version,
    pub file_name: PathBuf,
    pub file_contents: String,
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

impl Dependency {
    pub fn new(
        resolve: &mut Resolve,
        file_name: impl AsRef<Path>,
        file_contents: String,
    ) -> Result<Self> {
        let file_name = file_name.as_ref().to_path_buf();
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

    /// Writes the contents of the dependencyy to the path in the disk
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

/// The result of a comparison of the package () against a [Dependency]
pub enum Comparison {
    /// The package is ahead. The package is for a newer version of the app than the cli is connected to.
    Ahead(Version),

    /// The package is outdated. That means we can update it!
    Outdated(Version),

    /// The package is up-to-date. Nothing to do.
    UpToDate,
}
