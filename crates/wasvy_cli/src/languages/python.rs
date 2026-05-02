use std::{
    fs::{self, canonicalize},
    path::Path,
    process::Stdio,
};

use anyhow::{Context, Result, anyhow, bail};
use derive_more::{Deref, DerefMut};
use error_collection::Errors;

use crate::{
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

    fn create(&self, source: &Source) -> Result<()> {
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

        let mut poetry = Poetry::new(source);
        poetry
            .arg("bindings")
            .arg("src")
            .arg("--wit-path")
            .arg("wit/");
        errors.collect(poetry.run());

        errors.into()
    }

    fn build(&self, source: &Source, stdio: Stdio) -> Result<Source> {
        let path = source.path();
        let name = source.name();
        let dest = path.join("dest");
        let file = canonicalize(dest.join(format!("{name}.wasm")))?;

        let _ = fs::create_dir_all(&dest);

        let mut poetry = Poetry::new(source);
        poetry
            .arg("componentize")
            .arg("app")
            .arg("--wit-path")
            .arg("../wit/")
            .arg("-o")
            .arg(&file)
            .current_dir(path.join("src"))
            .stdout(stdio);
        poetry.run()?;

        Source::identify_file(&file, source.runtime())
            .with_context(|| anyhow!("identifying build artifact {file:?}"))
    }
}

fn get_name(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(&path).ok()?;
    let value = contents.parse::<toml::Table>().ok()?;
    value
        .get("project")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

#[derive(Deref, DerefMut)]
struct Poetry(std::process::Command);

impl Poetry {
    fn new(source: &Source) -> Self {
        let mut value = Self(std::process::Command::new("poetry"));
        value
            .arg("run")
            .arg("componentize-py")
            .arg("--world")
            .arg(source.name())
            .current_dir(source.path());
        value
    }

    fn run(&mut self) -> anyhow::Result<()> {
        match self.output() {
            Err(err) => Err(err.into()),
            Ok(output) if !output.status.success() => Err(anyhow::anyhow!(
                "componentize-py failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify() {
        let path = Path::new("../../examples/python_example");
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
        let path = Path::new("../../examples/simple");
        assert!(Python.identify(path).is_err());
    }
}
