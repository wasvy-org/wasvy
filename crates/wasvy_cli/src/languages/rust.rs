use std::{env, fs, iter::once, path::Path, process::Command};

use anyhow::{Context, Result, anyhow, bail};
use semver::Version;

use crate::{
    fs::WriteTo,
    language::Language,
    source::Source,
    wit::{Exports, Method, MethodArg, SystemImport},
};

pub struct Rust {
    pub rust_version: Version,
}

impl Rust {
    /// Gets the rust version from cargo
    pub fn new() -> Result<Self> {
        let path = env::current_dir().context("could not get working directory")?;
        let output = Command::new("cargo")
            .arg("--version")
            .current_dir(path)
            .output()
            .context("could not get cargo version")?;
        let version = String::from_utf8(output.stdout)?;
        Self::parse(version)
    }

    fn parse(version: impl AsRef<str>) -> Result<Self> {
        let version = version.as_ref().trim();
        once(Version::parse(&version))
            .chain(version.split(" ").map(Version::parse))
            .find_map(|parsed| parsed.ok())
            .map(|rust_version| Self { rust_version })
            .ok_or(anyhow!("could not parse rust version \"{version}\""))
    }
}

impl Language for Rust {
    fn identify(&self, path: &Path) -> bool {
        path.join("Cargo.toml").is_file()
    }

    fn name(&self, source: &Source) -> Option<String> {
        let path = source.path().join("Cargo.toml");
        let contents = fs::read_to_string(&path).ok()?;
        let value = contents.parse::<toml::Table>().ok()?;
        value
            .get("package")?
            .get("name")?
            .as_str()
            .map(|s| s.to_string())
    }

    fn generate(&self, source: &Source) -> Result<()> {
        let path = source.path();
        let name = source.name();
        let rust_version = &self.rust_version.to_string();
        let world_name = &source.world_name();
        let wasvy_wit_version = &source
            .runtime()
            .find_dependency("wasvy", "ecs")
            .expect("wasvy:ecs is a dependecy of the runtime")
            .version
            .to_string();

        #[derive(askama::Template)]
        #[template(path = "./rust/Cargo.toml")]
        struct CargoToml<'a> {
            name: &'a str,
            rust_version: &'a str,
        }
        let file1 = CargoToml { name, rust_version }.write(path);

        #[derive(askama::Template)]
        #[template(path = "./rust/src/lib.rs")]
        struct Lib<'a> {
            name: &'a str,
        }
        let file2 = Lib { name }.write(path);

        #[derive(askama::Template)]
        #[template(path = "./rust/src/bindings.rs")]
        struct Bindings<'a> {
            world_name: &'a str,
            wasvy_wit_version: &'a str,
        }
        let file3 = Bindings {
            world_name,
            wasvy_wit_version,
        }
        .write(path);

        // Avoid exiting before all files are written
        file1?;
        file2?;
        file3?;

        Ok(())
    }

    fn exports(&self, source: &Source) -> Result<Exports> {
        let path = source.path().join("src/lib.rs");
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("{source:?} is missing src/lib.rs"))?;

        let mut systems = Vec::new();

        // A very naive initial approach for parsing out exports
        let Some(impl_start) = contents.find("impl Guest for") else {
            bail!("missing Guest impl")
        };
        let impl_block = &contents[impl_start..];

        // Find the end of the impl block by counting braces
        let mut brace_count = 0;
        let mut impl_end = None;
        for (i, ch) in impl_block.chars().enumerate() {
            match ch {
                '{' => brace_count += 1,
                '}' => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        impl_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let Some(impl_end) = impl_end else {
            bail!("could not find end of Guest impl")
        };
        let impl_content = &impl_block[..=impl_end];

        // Find all function definitions within the impl block
        let fn_matches: Vec<_> = impl_content.match_indices("fn ").map(|(i, _)| i).collect();

        for &fn_start in &fn_matches {
            let fn_content = &impl_content[fn_start + 3..]; // Skip "fn "
            if let Some(fn_name_end) = fn_content.find('(') {
                let fn_name = fn_content[..fn_name_end].trim();

                // Skip the setup function
                if fn_name == "setup" {
                    continue;
                }

                // Extract parameters
                let params_content = &fn_content[fn_name_end + 1..];
                if let Some(params_end) = params_content.find(')') {
                    let params_str = &params_content[..params_end];

                    // Parse parameter names and types
                    let mut args = Vec::new();
                    for param in params_str.split(',') {
                        let param = param.trim();
                        if !param.is_empty() {
                            let parts: Vec<&str> = param.split(':').collect();
                            if parts.len() == 2 {
                                let param_name = parts[0].trim();
                                let param_type = parts[1].trim();

                                // Map parameter types to SystemImport variants
                                let import = match param_type {
                                    "Commands" => SystemImport::Commands,
                                    "Query" => SystemImport::Query,
                                    _ => continue, // Skip unknown parameter types
                                };

                                args.push(MethodArg {
                                    name: param_name.to_string(),
                                    import,
                                });
                            }
                        }
                    }

                    systems.push(Method {
                        name: fn_name.to_string(),
                        args,
                    });
                }
            }
        }

        Ok(Exports { methods: systems })
    }
}

impl Default for Rust {
    fn default() -> Self {
        Self::parse(env!("CARGO_PKG_RUST_VERSION")).expect("valid rust version")
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
    fn new() {
        assert!(Rust::new().is_ok())
    }

    #[test]
    fn parse() {
        let rust = Rust::parse("cargo 1.89.0 (c24e10642 2025-06-23)").expect("parses");
        assert_eq!(rust.rust_version, Version::new(1, 89, 0))
    }

    #[test]
    fn identify() {
        let path = Path::new("../../examples/simple");
        assert!(Rust::default().identify(path));
    }

    #[test]
    fn identify_invalid() {
        let path = Path::new("../../examples/python_example");
        assert!(!Rust::default().identify(path));
    }

    #[test]
    fn name() {
        let source = source();
        let name = Rust::default().name(&source).expect("name is found");
        assert_eq!(&name, "simple");
    }

    #[test]
    fn exports() {
        let source = source();
        let Exports { methods } = Rust::default().exports(&source).expect("exports are found");
        assert_eq!(methods.len(), 2);
        assert_eq!(
            methods[0],
            Method {
                name: "spawn_entities".to_string(),
                args: vec![MethodArg {
                    name: "commands".to_string(),
                    import: SystemImport::Commands,
                }],
            }
        );
        assert_eq!(
            methods[1],
            Method {
                name: "spin_cube".to_string(),
                args: vec![MethodArg {
                    name: "query".to_string(),
                    import: SystemImport::Query,
                }],
            }
        );
    }

    fn source() -> Source {
        let mut config = Config::default();
        config.add_language(Rust::default());
        let runtime = Runtime::new(config);

        let path = Path::new("../../examples/simple");
        let language = Id::from(&Rust::default());
        Source::mock(path, runtime, language)
    }
}
