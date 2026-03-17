use std::{env, fs, path::Path, process::Command};

use anyhow::{Context, Result, anyhow};
use askama::Template;
use semver::Version;

use crate::{language::Language, source::Source};

pub struct Rust {
    rust_version: Version,
}

impl Language for Rust {
    fn generate(&self, source: &Source) -> Result<()> {
        #[derive(Template)]
        #[template(path = "./rust/Cargo.toml")]
        struct CargoToml<'a> {
            name: &'a str,
            rust_version: &'a str,
            bevy_version: &'a str,
        }

        let bevy_version = source
            .builder()
            .resolve()
            .package_names
            .iter()
            .filter(|(name, _)| name.namespace == "bevyengine" && name.name == "bevy")
            .find_map(|(name, _)| name.version.clone())
            .context("Missing bevy version")?;

        fs::write(
            source.root().join("Cargo.toml"),
            CargoToml {
                name: source.name(),
                rust_version: &self.rust_version.to_string(),
                bevy_version: &bevy_version.to_string(),
            }
            .render()?,
        )?;

        Ok(())
    }
}

impl Default for Rust {
    fn default() -> Self {
        let path = env::current_dir().expect("current_dir");
        Self::new(path).unwrap_or(Rust {
            rust_version: Version::new(1, 89, 0),
        })
    }
}

impl Rust {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let output = Command::new("cargo")
            .arg("--version")
            .current_dir(path)
            .output()?; // ex: cargo 1.89.0 (c24e10642 2025-06-23)
        let version = String::from_utf8(output.stdout)?;
        Self::new_raw(version)
    }

    fn new_raw(version: String) -> Result<Self> {
        let rust_version = version
            .split(" ")
            .nth(1)
            .and_then(|ver| Version::parse(&ver).ok())
            .ok_or(anyhow!("could not parse rust version \"{version}\""))?;
        Ok(Self { rust_version })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_raw() {
        let version = "cargo 1.89.0 (c24e10642 2025-06-23)".to_string();
        let rust = Rust::new_raw(version).expect("parses");
        assert_eq!(rust.rust_version, Version::new(1, 89, 0))
    }
}
