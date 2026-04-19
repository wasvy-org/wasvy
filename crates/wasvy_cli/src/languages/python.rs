use std::{fs, path::Path, process::Stdio};

use anyhow::{Result, bail};

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
        let path = source.path();
        let namespace = source.runtime().namespace();
        let name = source.name();

        #[derive(askama::Template)]
        #[template(path = "./python/pyproject.toml")]
        pub struct PyProject<'a> {
            namespace: &'a str,
            name: &'a str,
        }
        let file1 = PyProject { namespace, name }.write(path);

        #[derive(askama::Template)]
        #[template(path = "./python/src/__init__.py")]
        pub struct Init;
        let file2 = Init.write(path);

        #[derive(askama::Template)]
        #[template(path = "./python/src/app.py")]
        pub struct App<'a> {
            name: &'a str,
        }
        let file3 = App { name }.write(path);

        // Remove outdated codegen
        let src = path.join("src");
        let _ = fs::remove_dir_all(src.join("componentize_py_async_support"));
        let _ = fs::remove_dir_all(src.join("wit_world"));
        let _ = fs::remove_dir_all(src.join("componentize_py_async_support"));
        let _ = fs::remove_file(src.join("componentize_py_runtime.pyi"));
        let _ = fs::remove_file(src.join("componentize_py_types.py"));
        let _ = fs::remove_file(src.join("poll_loop.py"));

        // Componentize will fail if the wit is not there
        source.update_deps()?;

        let output = std::process::Command::new("poetry")
            .arg("run")
            .arg("componentize-py")
            .arg("--wit-path")
            .arg("wit/")
            .arg("--world")
            .arg(name)
            .arg("bindings")
            .arg("src")
            .current_dir(path)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "componentize-py failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Avoid exiting before all files are written
        file1?;
        file2?;
        file3?;

        Ok(())
    }

    fn build(&self, source: &Source, stdio: Stdio) -> Result<Source> {
        let path = source.path();
        let name = source.name();
        let dest = path.join("dest");
        let _ = fs::create_dir_all(&dest);

        let output = std::process::Command::new("poetry")
            .arg("run")
            .arg("componentize-py")
            .arg("--wit-path")
            .arg("../wit/")
            .arg("--world")
            .arg(&name)
            .arg("componentize")
            .arg("app")
            .arg("-o")
            .arg(format!("../dest/{}.wasm", name))
            .current_dir(path.join("src"))
            .stdout(stdio)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "componentize-py build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(source.clone())
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
