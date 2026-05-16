use std::{
    fs::{self, canonicalize},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use error_collection::Errors;

use crate::{
    command::{Command, CommandType, Logging},
    fs::WriteTo,
    language::{Language, SourceInfo},
    named::Named,
    source::Source,
};

pub struct Python;

impl Language for Python {
    fn identify(&self, path: &Path) -> Result<SourceInfo> {
        let path = path.join("pyproject.toml");
        if !path.is_file() {
            bail!("missing pyproject.toml");
        }

        Ok(SourceInfo {
            name: get_name(&path),
        })
    }

    fn create(&self, source: &Source, logging: Logging) -> Result<()> {
        let mut errors = Errors::new();

        let path = source.path();
        let namespace = source.runtime().namespace();
        let name = source.name();

        #[derive(askama::Template)]
        #[template(path = "./python/pyproject.toml")]
        pub struct PyProject<'a> {
            namespace: &'a str,
            name: &'a str,
        }
        errors.collect(PyProject { namespace, name }.write(path));

        #[derive(askama::Template)]
        #[template(path = "./python/src/__init__.py")]
        pub struct Init;
        errors.collect(Init.write(path));

        #[derive(askama::Template)]
        #[template(path = "./python/src/app.py")]
        pub struct App<'a> {
            name: &'a str,
        }
        errors.collect(App { name }.write(path));

        // Remove outdated codegen
        let src = path.join("src");
        let _ = fs::remove_dir_all(src.join("componentize_py_async_support"));
        let _ = fs::remove_dir_all(src.join("wit_world"));
        let _ = fs::remove_dir_all(src.join("componentize_py_async_support"));
        let _ = fs::remove_file(src.join("componentize_py_runtime.pyi"));
        let _ = fs::remove_file(src.join("componentize_py_types.py"));
        let _ = fs::remove_file(src.join("poll_loop.py"));

        // Componentize will fail if the wit is not there
        errors.collect(source.update_deps());

        // Bindings generation
        errors.collect(Command::run(Poetry::Bindings, source, logging));

        errors.into()
    }

    fn build(&self, source: &Source, logging: Logging) -> Result<Source> {
        let path = source.path();
        let name = source.name();

        let dest = &path.join("dest");
        let _ = fs::create_dir_all(dest);

        let output = &canonicalize(dest)
            .with_context(|| anyhow!("{dest:?}"))?
            .join(format!("{name}.wasm"));

        Command::run(Poetry::Componentize { output }, source, logging)?;

        Source::identify_file(output, Some(source.name()), source.runtime())
            .with_context(|| anyhow!("identifying build artifact {output:?}"))
    }
}

fn get_name(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let value = contents.parse::<toml::Table>().ok()?;
    value
        .get("project")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

enum Poetry<'a> {
    Bindings,
    Componentize { output: &'a Path },
}

impl<'a> CommandType for Poetry<'a> {
    const PROGRAM: &'static str = "poetry";

    fn setup(self, command: &mut Command, source: &Source) -> Result<()> {
        command.arg("run").arg("componentize-py");
        match self {
            Poetry::Bindings => {
                command
                    .arg("--wit-path")
                    .arg("wit/")
                    .arg("--world")
                    .arg(source.world_name())
                    .arg("bindings")
                    .arg("src");
            }
            Poetry::Componentize { output } => {
                command
                    .arg("--wit-path")
                    .arg("../wit/")
                    .arg("--world")
                    .arg(source.world_name())
                    .arg("componentize")
                    .arg("app")
                    .arg("-o")
                    .arg(output)
                    .current_dir(source.path().join("src"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify() {
        let path = Path::new("../../examples/mods/python");
        let info = Python.identify(path).expect("valid source");
        assert_eq!(
            info,
            SourceInfo {
                name: Some("python-example".into())
            }
        );
    }

    #[test]
    fn identify_invalid() {
        let path = Path::new("../../examples/mods/rust/basic");
        assert!(Python.identify(path).is_err());
    }
}
