use std::process::ExitStatus;

use anyhow::Result;

use crate::source::Source;

pub trait Editor: Send + Sync {
    /// Checks that an editor is available
    fn available(&self) -> bool;

    /// Opens up a source in an editor installed on the user's machine
    fn launch(&self, source: &Source) -> Result<ExitStatus>;
}
