use std::process::{Command, ExitStatus};

use anyhow::Result;

use crate::{editor::Editor, named::Named, source::Source};

pub struct Generic {
    pub program: String,
}

impl Generic {
    pub fn new(program: impl AsRef<str>) -> Self {
        Self {
            program: program.as_ref().to_string(),
        }
    }
}

impl Editor for Generic {
    /// Checks that an editor is available
    fn available(&self) -> bool {
        Command::new(&self.program)
            .arg("--version")
            .status()
            .is_ok()
    }

    /// Opens up a source in an editor installed on the user's machine
    fn launch(&self, source: &Source) -> Result<ExitStatus> {
        let path = source.path();
        let status = Command::new(&self.program)
            .arg(".")
            .current_dir(path)
            .status()?;
        Ok(status)
    }
}

impl Named for Generic {
    fn name(&self) -> &str {
        &self.program
    }
}
