use std::{
    env, fs,
    iter::once,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use anyhow::{Context, Result, anyhow};
use semver::Version;
use tracing::warn;

use crate::{fs::WriteTo, language::Language, source::Source, witgen::write_guest_wit};

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
        once(Version::parse(version))
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

    fn build(&self, source: &Source, stdio: Stdio) -> Result<Source> {
        let result = build(Full, source, stdio);

        // Since rust is strongly typed, compilation might fail if wit is outdated.
        // Retry building setup only and generate wit from that if possible.
        if let Err(_error) = result.as_ref()
        // && error.to_string().contains("TODO")
        {
            let source = source.clone();
            thread::spawn(move || {
                retry_witgen(source)
                    .err()
                    .map(|err| warn!("Failed generating wit: {err:?}"))
            });
        }

        result
    }
}

fn retry_witgen(source: Source) -> Result<()> {
    let wit_source = build(SetupOnly, &source, Stdio::null())?;
    write_guest_wit(&wit_source)
}

enum BuildMode {
    /// Full build for the game
    Full,

    /// Instruct the wasvy_setup macro to only export the setup method
    SetupOnly,
}
use BuildMode::*;

fn build(mode: BuildMode, source: &Source, stdio: Stdio) -> Result<Source> {
    let name = source.name();
    let path = source.path();

    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip2")
        .arg("-p")
        .arg(name)
        .current_dir(path)
        .stderr(stdio);

    if matches!(mode, Full) {
        command.arg("--feature").arg("setup_only");
    }

    let file = target_dir(path)
        .with_context(|| format!("path = {path:?}"))?
        .join("wasm32-wasip2")
        .join("release")
        .join(format!("{name}.wasm"));
    Source::identify_file(file, source.runtime()).context("identifying build artifcat")
}

fn target_dir(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .current_dir(path)
        .output()
        .context("could not run cargo metadata")?;

    let stdout =
        String::from_utf8(output.stdout).context("cargo metadata output was not valid UTF-8")?;

    let metadata: serde_json::Value =
        serde_json::from_str(&stdout).context("failed to parse cargo metadata as JSON")?;

    let target_directory = metadata["target_directory"]
        .as_str()
        .context("target_directory not found in cargo metadata")?;

    Ok(PathBuf::from(target_directory))
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
        languages::rust::target_dir,
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
    fn target_dir_simple() {
        let dir = target_dir(".").unwrap();
        assert_eq!(dir.file_name(), Some("target".as_ref()));
        assert!(dir.try_exists().unwrap_or(false));
    }

    #[test]
    fn build_simple() {
        let dir = target_dir(".").unwrap();
        assert_eq!(dir.file_name(), Some("target".as_ref()));
        assert!(dir.try_exists().unwrap());
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
