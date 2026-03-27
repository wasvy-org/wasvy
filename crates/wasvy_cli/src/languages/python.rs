use std::{fs, path::Path};

use anyhow::{Context, Result};

use crate::{
    fs::WriteTo,
    language::Language,
    source::Source,
    wit::{Exports, Method, MethodArg, SystemImport},
};

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

    fn generate(&self, source: &Source) -> Result<()> {
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

        // Avoid exiting before all files are written
        file1?;
        file2?;
        file3?;

        Ok(())
    }

    fn exports(&self, source: &Source) -> Result<Exports> {
        let path = source.path().join("src/app.py");
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("{source:?} is missing src/app.py"))?;

        let mut systems = Vec::new();

        // A very naive initial approach for parsing out exports
        let lines: Vec<&str> = contents.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];

            // Look for class definitions
            if line.trim_start().starts_with("class ") {
                // Check if this class has a setup method
                let mut has_setup = false;
                let mut j = i + 1;

                // Skip class documentation/comment lines
                while j < lines.len()
                    && (lines[j].trim_start().starts_with("#") || lines[j].trim().is_empty())
                {
                    j += 1;
                }

                // Look for setup method in the class
                while j < lines.len() && !lines[j].starts_with("class") {
                    if lines[j].trim_start().starts_with("def setup(") {
                        has_setup = true;
                        break;
                    }
                    j += 1;
                }

                // If class has setup method, extract all its methods
                if has_setup {
                    let mut k = i + 1;
                    while k < lines.len() && !lines[k].starts_with("class") {
                        let method_line = lines[k];
                        if method_line.trim_start().starts_with("def ")
                            && !method_line.trim_start().starts_with("def setup(")
                            && method_line.contains("(")
                            && method_line.contains("):")
                        {
                            // Extract method name
                            let method_def = method_line.trim_start();
                            let method_name = method_def
                                .strip_prefix("def ")
                                .and_then(|s| s.split('(').next())
                                .unwrap_or("")
                                .trim();

                            // Extract arguments
                            let args_str = method_def
                                .split('(')
                                .nth(1)
                                .unwrap_or("")
                                .split(')')
                                .next()
                                .unwrap_or("");

                            let mut args = Vec::new();
                            for arg in args_str.split(',') {
                                let arg = arg.trim();
                                if !arg.is_empty() && arg != "self" {
                                    let parts: Vec<&str> = arg.split(':').collect();
                                    if parts.len() >= 2 {
                                        let arg_name = parts[0].trim();
                                        let arg_type = parts[1].trim();

                                        let import = match arg_type {
                                            "Commands" => SystemImport::Commands,
                                            "Query" => SystemImport::Query,
                                            _ => continue, // Skip unrecognized argument types
                                        };

                                        args.push(MethodArg {
                                            name: arg_name.to_string(),
                                            import,
                                        });
                                    }
                                }
                            }

                            if !method_name.is_empty() {
                                systems.push(Method {
                                    name: method_name.to_string(),
                                    args,
                                });
                            }
                        }
                        k += 1;
                    }
                }

                i = j;
            } else {
                i += 1;
            }
        }

        Ok(Exports { methods: systems })
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

    #[test]
    fn exports() {
        let source = source();
        let Exports { methods } = Python.exports(&source).expect("exports are found");
        assert_eq!(methods.len(), 2);
        assert_eq!(
            methods[0],
            Method {
                name: "spin_cube".to_string(),
                args: vec![MethodArg {
                    name: "query".to_string(),
                    import: SystemImport::Query,
                }],
            }
        );
        assert_eq!(
            methods[1],
            Method {
                name: "my_system".to_string(),
                args: vec![
                    MethodArg {
                        name: "commands".to_string(),
                        import: SystemImport::Commands,
                    },
                    MethodArg {
                        name: "query".to_string(),
                        import: SystemImport::Query,
                    }
                ],
            }
        );
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
