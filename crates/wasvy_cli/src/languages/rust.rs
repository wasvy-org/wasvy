use std::{
    collections::HashSet,
    env, fs,
    iter::once,
    path::{Path, PathBuf},
    process, thread,
};

use anyhow::{Context, Result, anyhow, bail};
use error_collection::Errors;
use glob::Pattern;
use semver::Version;
use serde::Deserialize;
use toml_edit::{Array, DocumentMut, Item, RawString, Value};
use tracing::warn;

use crate::{
    command::{Command, CommandType, Logging},
    fs::WriteTo,
    id::Id,
    language::{Language, SourceInfo},
    named::Named,
    source::Source,
    witgen::write_guest_wit,
};

pub struct Rust {
    pub rust_version: Version,
}

impl Rust {
    /// Gets the rust version from cargo
    pub fn new() -> Result<Self> {
        let path = env::current_dir().context("could not get working directory")?;
        let output = process::Command::new("cargo")
            .arg("--version")
            .current_dir(path)
            .output()
            .context("could not get cargo version")?;
        let version = String::from_utf8(output.stdout)?;
        Self::parse(version)
    }

    pub fn id() -> Id {
        (&Self::default()).into()
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
    fn identify(&self, path: &Path) -> Result<SourceInfo> {
        let path = path.join("Cargo.toml");
        if !path.is_file() {
            bail!("missing Cargo.toml");
        }

        Ok(SourceInfo {
            name: get_name(&path),
        })
    }

    fn scaffold(&self, source: &Source, logging: Logging) -> Result<()> {
        let mut errors = Errors::new();

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
        let file = CargoToml { name, rust_version };
        errors.collect(file.write(path));

        #[derive(askama::Template)]
        #[template(path = "./rust/src/lib.rs")]
        struct Lib<'a> {
            name: &'a str,
        }
        let file = Lib { name };
        errors.collect(file.write(path));

        #[derive(askama::Template)]
        #[template(path = "./rust/src/bindings.rs")]
        struct Bindings<'a> {
            world_name: &'a str,
            wasvy_wit_version: &'a str,
        }
        let file = Bindings {
            world_name,
            wasvy_wit_version,
        };
        errors.collect(file.write(path));

        errors.collect(source.update_deps());

        if errors.is_empty() {
            errors.collect(add_to_workspace_if_needed(source.path()));
            errors.collect(Command::run(Cargo::Check, source, logging));
        }

        errors.as_result()
    }

    fn build(&self, source: &Source, logging: Logging) -> Result<Source> {
        if let Err(error) = Command::run(Cargo::Build, source, logging) {
            // Since rust is strongly typed, compilation might fail if wit is outdated.
            // Retry building setup only and generate wit from that if possible.
            // if error.to_string().contains("TODO") {
            let source = source.clone();
            thread::spawn(move || {
                if let Err(err) = retry_witgen(&source) {
                    warn!("Failed generating wit: {err:?}")
                }
            });

            return Err(error);
        }

        get_build_artifact(source)
    }

    fn watch_paths(&self, source: &Source) -> Vec<PathBuf> {
        vec![source.path().join("src"), source.path().join("Cargo.toml")]
    }
}

enum Cargo {
    Build,
    Check,
}

impl CommandType for Cargo {
    const PROGRAM: &str = "cargo";

    fn setup(self, command: &mut Command, source: &Source) -> Result<()> {
        match self {
            Self::Build => {
                command
                    .arg("build")
                    .arg("--release")
                    .arg("--target")
                    .arg("wasm32-wasip2");
            }
            Self::Check => {
                command.arg("check");
            }
        }
        command.arg("-p").arg(source.name());

        Ok(())
    }
}

fn get_name(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let value = contents.parse::<toml::Table>().ok()?;
    value
        .get("package")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

fn get_build_artifact(source: &Source) -> Result<Source> {
    let name = source.name().replace("-", "_");
    let path = source.path();
    let file = build_directory(path)
        .with_context(|| format!("build_directory for path = {path:?}"))?
        .join("wasm32-wasip2")
        .join("release")
        .join(format!("{name}.wasm"));
    Source::new_wasm(&file, Some(name), source.runtime())
        .with_context(|| anyhow!("identifying build artifact {file:?}"))
}

fn retry_witgen(source: &Source) -> Result<()> {
    let mut command = Command::new(Cargo::Build, source, Logging::Ignore)?;
    command.arg("--features").arg("setup_only");
    command.execute()?;

    let wit_source = get_build_artifact(source)?;
    write_guest_wit(&wit_source)
}

#[derive(Deserialize, Default)]
pub(crate) struct Metadata {
    pub(crate) packages: Vec<MetadataPackage>,
    pub(crate) workspace_root: PathBuf,
    pub(crate) target_directory: PathBuf,
}

#[derive(Deserialize, Default)]
pub(crate) struct MetadataPackage {
    pub(crate) name: String,
    pub(crate) targets: Vec<MetadataTarget>,
}

#[derive(Deserialize, Default)]
pub(crate) struct MetadataTarget {
    pub(crate) crate_types: HashSet<String>,
}

pub(crate) fn cargo_metadata(path: impl AsRef<Path>) -> Result<Metadata> {
    let path = path.as_ref();
    let output = process::Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .current_dir(path)
        .output()
        .context("could not run `cargo metadata`")?;

    serde_json::from_slice(&output.stdout).context("cargo metadata output was not valid UTF-8")
}

fn build_directory(path: impl AsRef<Path>) -> Result<PathBuf> {
    Ok(cargo_metadata(path)?.target_directory)
}

fn add_to_workspace_if_needed(crate_path: &Path) -> Result<()> {
    let Some(parent) = crate_path.parent() else {
        return Ok(());
    };
    let Ok(Metadata { workspace_root, .. }) = cargo_metadata(parent) else {
        return Ok(());
    };

    let workspace_manifest = workspace_root.join("Cargo.toml");
    if !workspace_manifest.is_file() {
        return Ok(());
    }

    let crate_path = fs::canonicalize(crate_path)
        .with_context(|| format!("canonicalizing scaffolded crate path {crate_path:?}"))?;
    let relative_path = crate_path
        .strip_prefix(&workspace_root)
        .with_context(|| format!("{crate_path:?} is not inside workspace {workspace_root:?}"))?;
    let relative_path = cargo_path(relative_path);
    let contents = fs::read_to_string(&workspace_manifest)
        .with_context(|| format!("reading workspace manifest {workspace_manifest:?}"))?;
    if !has_workspace_table(&contents) {
        return Ok(());
    }

    if workspace_members(&contents)
        .iter()
        .any(|member| member_covers_path(member, &relative_path))
    {
        return Ok(());
    }

    let contents = append_workspace_member(&contents, &relative_path)?;
    fs::write(&workspace_manifest, contents)
        .with_context(|| format!("writing workspace manifest {workspace_manifest:?}"))
}

fn has_workspace_table(contents: &str) -> bool {
    let Ok(document) = contents.parse::<DocumentMut>() else {
        return false;
    };
    document.get("workspace").and_then(Item::as_table).is_some()
}

fn cargo_path(path: &Path) -> String {
    path.iter()
        .map(|component| component.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn workspace_members(contents: &str) -> Vec<String> {
    let Ok(document) = contents.parse::<DocumentMut>() else {
        return Vec::new();
    };
    document
        .get("workspace")
        .and_then(Item::as_table)
        .and_then(|workspace| workspace.get("members"))
        .and_then(Item::as_array)
        .into_iter()
        .flatten()
        .filter_map(|member| member.as_str().map(str::to_string))
        .collect()
}

fn member_covers_path(member: &str, path: &str) -> bool {
    member == path || Pattern::new(member).is_ok_and(|pattern| pattern.matches(path))
}

fn append_workspace_member(contents: &str, member: &str) -> Result<String> {
    let mut document = contents
        .parse::<DocumentMut>()
        .context("parsing workspace manifest")?;
    let Some(workspace) = document.get_mut("workspace").and_then(Item::as_table_mut) else {
        bail!("workspace manifest is missing [workspace]");
    };

    if !workspace.contains_key("members") {
        workspace.insert("members", Item::Value(Value::Array(Default::default())));
    }

    let Some(members) = workspace.get_mut("members").and_then(Item::as_array_mut) else {
        bail!("workspace.members must be an array");
    };
    if is_multiline_array(members) {
        let indent = array_member_indent(contents, members).unwrap_or_else(|| "    ".to_string());
        members.push_formatted(Value::from(member).decorated(format!("\n{indent}"), ""));
        members.set_trailing_comma(true);
        members.set_trailing("\n");
    } else {
        members.push(member);
    }
    Ok(document.to_string())
}

fn is_multiline_array(array: &Array) -> bool {
    array.to_string().contains('\n')
}

fn array_member_indent(contents: &str, array: &Array) -> Option<String> {
    array.iter().find_map(|value| {
        let prefix = raw_string(contents, value.decor().prefix()?)?;
        let (_, indent) = prefix.rsplit_once('\n')?;
        Some(indent.to_string())
    })
}

fn raw_string<'a>(contents: &'a str, raw: &'a RawString) -> Option<&'a str> {
    raw.as_str()
        .or_else(|| raw.span().and_then(|span| contents.get(span)))
}

impl Default for Rust {
    fn default() -> Self {
        Self::parse(env!("CARGO_PKG_RUST_VERSION")).expect("valid rust version")
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::languages::rust::build_directory;

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
        let path = Path::new("../../examples/mods/rust/basic");
        let info = Rust::default().identify(path).expect("valid source");
        assert_eq!(
            info,
            SourceInfo {
                name: Some("basic_example_mod".into())
            }
        );
    }

    #[test]
    fn identify_invalid() {
        let path = Path::new("../../examples/mods/python");
        assert!(Rust::default().identify(path).is_err());
    }

    #[test]
    fn build_directory_current() {
        let dir = build_directory(".").unwrap();
        assert_eq!(dir.file_name(), Some("target".as_ref()));
        assert!(dir.try_exists().unwrap_or(false));
    }

    #[test]
    fn workspace_member_wildcard_covers_path() {
        assert!(member_covers_path("crates/*", "crates/example"));
        assert!(!member_covers_path("crates/*", "examples/example"));
    }

    #[test]
    fn append_workspace_member_to_inline_array() {
        let manifest = "[workspace]\nmembers = [\"crates/*\"]\n";
        let manifest = append_workspace_member(manifest, "mods/new-mod").unwrap();
        assert_eq!(
            manifest,
            "[workspace]\nmembers = [\"crates/*\", \"mods/new-mod\"]\n"
        );
    }

    #[test]
    fn append_workspace_member_to_multiline_array() {
        let manifest = "[workspace]\nmembers = [\n    \"crates/*\",\n]\n";
        let manifest = append_workspace_member(manifest, "mods/new-mod").unwrap();
        assert_eq!(
            manifest,
            "[workspace]\nmembers = [\n    \"crates/*\",\n    \"mods/new-mod\",\n]\n"
        );
    }

    #[test]
    fn append_workspace_member_when_members_key_is_missing() {
        let manifest = "[workspace]\nresolver = \"3\"\n";
        let manifest = append_workspace_member(manifest, "mods/new-mod").unwrap();
        assert_eq!(
            workspace_members(&manifest),
            vec!["mods/new-mod".to_string()]
        );
        assert!(manifest.contains("resolver = \"3\""));
    }

    #[test]
    fn append_workspace_member_preserves_comments() {
        let manifest = "# top\n[workspace]\n# members comment\nmembers = [\"crates/*\"] # inline\n";
        let manifest = append_workspace_member(manifest, "mods/new-mod").unwrap();
        assert!(manifest.contains("# top"));
        assert!(manifest.contains("# members comment"));
        assert!(manifest.contains("# inline"));
        assert_eq!(
            workspace_members(&manifest),
            vec!["crates/*".to_string(), "mods/new-mod".to_string()]
        );
    }

    #[test]
    fn package_manifest_is_not_a_workspace() {
        let manifest = "[package]\nname = \"app\"\n";
        assert!(!has_workspace_table(manifest));
    }

    #[test]
    fn workspace_members_are_parsed() {
        let manifest = "[workspace]\nmembers = [\"crates/*\", \"mods/new-mod\"]\n";
        assert_eq!(
            workspace_members(manifest),
            vec!["crates/*".to_string(), "mods/new-mod".to_string()]
        );
    }

    #[test]
    fn add_to_workspace_adds_uncovered_crate() {
        let target = test_artifact_path("add_to_workspace_adds_uncovered_crate");
        fs::write(
            target.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"3\"\n",
        )
        .unwrap();

        let crate_path = target.join("mods").join("new-mod");
        fs::create_dir_all(crate_path.join("src")).unwrap();
        fs::write(
            crate_path.join("Cargo.toml"),
            "[package]\nname = \"new-mod\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(crate_path.join("src/lib.rs"), "").unwrap();

        add_to_workspace_if_needed(&crate_path).unwrap();

        let manifest = fs::read_to_string(target.join("Cargo.toml")).unwrap();
        assert!(workspace_members(&manifest).contains(&"mods/new-mod".to_string()));
    }

    #[test]
    fn add_to_workspace_skips_wildcard_covered_crate() {
        let target = test_artifact_path("add_to_workspace_skips_wildcard_covered_crate");
        fs::write(
            target.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
        )
        .unwrap();

        let crate_path = target.join("crates").join("new-mod");
        fs::create_dir_all(crate_path.join("src")).unwrap();
        fs::write(
            crate_path.join("Cargo.toml"),
            "[package]\nname = \"new-mod\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(crate_path.join("src/lib.rs"), "").unwrap();

        add_to_workspace_if_needed(&crate_path).unwrap();

        let manifest = fs::read_to_string(target.join("Cargo.toml")).unwrap();
        assert_eq!(workspace_members(&manifest), vec!["crates/*".to_string()]);
    }

    fn test_artifact_path(path: impl AsRef<Path>) -> PathBuf {
        let target = env::var("CARGO_TARGET_DIR").unwrap_or("../../target".to_string());
        let target = PathBuf::from(target)
            .join(env!("CARGO_CRATE_NAME"))
            .join(path);
        let _ = fs::remove_dir_all(&target);
        fs::create_dir_all(&target).expect("create artifact directory");
        target
    }
}
