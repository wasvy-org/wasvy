use std::path::Path;

use anyhow::Result;

use crate::{named::Named, source::Source, wit::Exports};

pub trait Language {
    /// Given a path on the filesystem, determines whether it is a mod source.
    fn identify(&self, path: &Path) -> bool;

    /// Gets the source/project name from language-specific files
    ///
    /// For example, a Rust source usually has a name in the Cargo.toml
    fn name(&self, source: &Source) -> Option<String> {
        let _ = source;
        None
    }

    /// Parses required wit exports from language-specific files
    fn exports(&self, source: &Source) -> Result<Exports>;

    /// Generates necessary files for a source of this language type
    ///
    /// This shouldn't overwrite existing files.
    fn generate(&self, source: &Source) -> Result<()>;
}

impl<T> Named for T where T: Language {}
