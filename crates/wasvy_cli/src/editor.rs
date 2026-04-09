use std::process::ExitStatus;

use anyhow::Result;

use crate::{editors::Generic, named::Named, source::Source};

pub trait Editor: Named + Send + Sync {
    /// Checks that an editor is available
    fn available(&self) -> bool;

    /// Opens up a source in an editor installed on the user's machine
    fn launch(&self, source: &Source) -> Result<ExitStatus>;
}

pub struct BoxedEditor(Box<dyn Editor>);

impl BoxedEditor {
    pub fn available(&self) -> bool {
        self.0.available()
    }

    pub fn launch(&self, source: &Source) -> Result<ExitStatus> {
        self.0.launch(source)
    }
}

impl Named for BoxedEditor {
    fn name(&self) -> &str {
        self.0.name()
    }
}

impl<T> From<T> for BoxedEditor
where
    T: Editor + Named + 'static,
{
    fn from(value: T) -> Self {
        Self(Box::new(value))
    }
}

impl From<&'static str> for BoxedEditor {
    fn from(value: &'static str) -> Self {
        Generic::new(value).into()
    }
}
