use std::{path::Path, process::Stdio};

use anyhow::Result;
use derive_more::Deref;

use crate::{named::Named, source::Source};

pub trait Language: Named + Send + Sync {
    /// Given a path on the filesystem, determines whether it is a mod source and returns metadata.
    fn identify(&self, path: &Path) -> Result<SourceInfo>;

    /// Creates necessary files for a new source of this language type
    fn create(&self, source: &Source) -> Result<()>;

    /// Compiles this language to a source.
    ///
    /// See [Source::identify_file] to return a Source from a wasm file.
    fn build(&self, source: &Source, stdio: Stdio) -> Result<Source>;
}

/// Information identified from a mod directory by a [Language] implementation.
///
/// See [Language::identify]
#[derive(Debug, Default, Eq, PartialEq, Clone)]
pub struct SourceInfo {
    /// Expected to be the source/project title from language-specific files
    ///
    /// For example, a Rust source usually has a name in the Cargo.toml
    pub name: Option<String>,
}

impl<T> Named for T where T: Language {}

#[derive(Deref)]
pub struct BoxedLanguage(Box<dyn Language>);

impl Named for BoxedLanguage {
    fn name(&self) -> &str {
        self.0.name()
    }
}

impl<T> From<T> for BoxedLanguage
where
    T: Language + 'static,
{
    fn from(value: T) -> Self {
        Self(Box::new(value))
    }
}
