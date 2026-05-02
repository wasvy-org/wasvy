use std::{
    borrow::Cow,
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use semver::Version;
use wit_parser::{PackageId, PackageName, Resolve};

use crate::fs::{WriteTo, write};

/// Represents a wit interface supported by the game, thus a possible dependency for our mod.
///
/// These include:
/// - bevy-ecs.wit
/// - wasvy-ecs.wit
///
/// And any custom wit we choose to define.
#[derive(Clone)]
pub struct Dependency {
    /// The name of the package
    pub name: String,

    /// The namespace of the package
    pub namespace: String,

    /// The version of the package
    pub version: Version,

    /// The contents of the wit file: the interface
    pub interface: Interface,
}

pub type Interface = Cow<'static, str>;

impl From<&Dependency> for Interface {
    fn from(value: &Dependency) -> Self {
        value.interface.clone()
    }
}

impl Dependency {
    pub fn new(interface: impl Into<Interface>) -> Result<Self> {
        let mut resolve = Resolve::default();
        Self::new_with_resolve(interface, &mut resolve).map(|(dep, _)| dep)
    }

    pub fn new_with_resolve(
        interface: impl Into<Interface>,
        resolve: &mut Resolve,
    ) -> Result<(Self, PackageId)> {
        let interface = interface.into();
        let package_id = resolve.push_str("unknown", &interface)?;
        let package = &resolve.packages[package_id];
        let PackageName {
            name,
            namespace,
            version,
        } = package.name.clone();
        let version = version.ok_or(anyhow!(
            "dependency \"{namespace}:{name}.wit\" must have a version"
        ))?;
        Ok((
            Self {
                name,
                namespace,
                version,
                interface,
            },
            package_id,
        ))
    }

    pub fn resolve(&self, path: impl AsRef<Path>, resolve: &mut Resolve) -> Result<PackageId> {
        let path = path.as_ref().join(&self.file_name());
        resolve.push_str(path, &self.interface)
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

    /// Returns the file name for this dependency in the format "namespace:name.wit"
    pub fn file_name(&self) -> PathBuf {
        let Self {
            namespace, name, ..
        } = self;
        format!("{namespace}:{name}.wit").into()
    }
}

impl WriteTo for Dependency {
    fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref().join(&self.file_name());
        write(&path, self.interface.as_bytes())
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            namespace,
            name,
            version,
            ..
        } = self;
        write!(f, "{namespace}:{name}@{version}")
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
