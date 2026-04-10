use std::{fs, path::Path, process::Stdio};

use anyhow::Result;

use crate::{fs::WriteTo, language::Language, source::Source};

pub struct Python;

impl Language for Python {
    fn identify(&self, path: &Path) -> bool {
        path.join("pyproject.toml").is_file()
    }

    fn name(&self, source: &Source) -> Option<String> {
        let path = source.path().join("pyproject.toml");
        let contents = fs::read_to_string(&path).ok()?;
        let value = contents.parse::<toml::Table>().ok()?;
        value
            .get("project")?
            .get("name")?
            .as_str()
            .map(|s| s.to_string())
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

#[cfg(test)]
mod tests {
    use crate::{
        id::Id,
        runtime::{Config, Runtime},
    };

    use super::*;

    #[test]
    fn identify() {
        let path = Path::new("../../examples/python_example");
        assert!(Python.identify(path));
    }

    #[test]
    fn identify_invalid() {
        let path = Path::new("../../examples/simple");
        assert!(!Python.identify(path));
    }

    #[test]
    fn name() {
        let source = source();
        let name = Python.name(&source).expect("name is found");
        assert_eq!(&name, "python-example");
    }

    fn source() -> Source {
        let mut config = Config::default();
        config.add_language(Python);
        let runtime = Runtime::new(config);

        let path = Path::new("../../examples/python_example");
        let language = Id::from(&Python);
        Source::mock(path, runtime, language)
    }
}
