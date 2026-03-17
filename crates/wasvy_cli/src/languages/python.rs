use std::fs;

use anyhow::Result;
use askama::Template;

use crate::{language::Language, source::Source};

pub struct Python;

impl Language for Python {
    fn generate(&self, source: &Source) -> Result<()> {
        let name = source.name();

        #[derive(Template)]
        #[template(path = "./python/pyproject.toml")]
        pub struct PyProject<'a> {
            name: &'a str,
        }

        fs::write(
            source.root().join("pyproject.toml"),
            PyProject { name }.render()?,
        )?;

        Ok(())
    }
}
