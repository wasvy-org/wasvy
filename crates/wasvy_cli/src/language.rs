use std::{path::Path, process::Stdio};

use anyhow::Result;

use crate::{named::Named, source::Source};

pub trait Language: Send + Sync {
    /// Given a path on the filesystem, determines whether it is a mod source.
    fn identify(&self, path: &Path) -> bool;

    /// Gets the source/project name from language-specific files
    ///
    /// For example, a Rust source usually has a name in the Cargo.toml
    fn name(&self, source: &Source) -> Option<String> {
        let _ = source;
        None
    }

    /// Creates necessary files for a new source of this language type
    fn create(&self, source: &Source) -> Result<()>;

    /// Compiles this language to a source.
    ///
    /// See [Source::identify_file] to return a Source from a wasm file.
    fn build(&self, source: &Source, stdio: Stdio) -> Result<Source>;
}

impl<T> Named for T where T: Language {}
