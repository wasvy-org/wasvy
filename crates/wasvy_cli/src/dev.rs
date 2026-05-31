use std::{
    collections::BTreeSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode},
};
use wasvy::{
    modules::ModuleId,
    workspace::{WorkspaceManifest, parse_workspace_manifest},
};

const WASM_TARGET: &str = "wasm32-wasip2";
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);
const HOST_EXIT_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct DevSession {
    pub manifest_path: PathBuf,
    pub manifest: WorkspaceManifest,
    pub host_manifest_path: PathBuf,
    pub module_specs: Vec<ModuleBuildSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleBuildSpec {
    pub id: ModuleId,
    pub package_name: String,
    pub crate_path: PathBuf,
    pub artifact_stem: String,
    pub built_wasm: PathBuf,
    pub staged_wasm: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ChangeSet {
    changed_modules: BTreeSet<String>,
    rebuild_all_guest_modules: bool,
    restart_host: bool,
}

pub fn load_dev_session(manifest_path: impl AsRef<Path>) -> Result<DevSession> {
    let manifest_path = fs::canonicalize(manifest_path.as_ref()).with_context(|| {
        format!(
            "failed to resolve workspace manifest at {}",
            manifest_path.as_ref().display()
        )
    })?;
    let manifest = parse_workspace_manifest(&manifest_path)?;
    let host_dir = manifest
        .host
        .clone()
        .context("[workspace].host is required for `wasvy dev`")?;
    let host_manifest_path = host_dir.join("Cargo.toml");
    if !host_manifest_path.exists() {
        bail!(
            "expected host Cargo manifest at {}",
            host_manifest_path.display()
        );
    }

    let module_specs = load_module_build_specs(&manifest)?;

    Ok(DevSession {
        manifest_path,
        manifest,
        host_manifest_path,
        module_specs,
    })
}

pub fn render_dev_plan(session: &DevSession, native: bool) -> String {
    let mode = if native { "native" } else { "guest" };
    let modules = session
        .manifest
        .default_world
        .active_modules
        .iter()
        .map(|id| id.as_str().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "wasvy dev\nmode: {mode}\nmanifest: {}\nhost: {}\nmodules: {}",
        session.manifest_path.display(),
        session.host_manifest_path.display(),
        if modules.is_empty() {
            "<none>"
        } else {
            &modules
        }
    )
}

pub fn run_dev(manifest_path: impl AsRef<Path>, native: bool) -> Result<()> {
    let session = load_dev_session(manifest_path)?;
    println!("{}", render_dev_plan(&session, native));

    if !native {
        rebuild_and_stage_modules(&session, &session.module_specs)?;
    }

    let mut host = HostProcess::spawn(&session, native)?;
    let (_watcher, rx) = create_watcher(&session)?;

    println!("watching for changes...");

    loop {
        if let Some(status) = host.try_wait()? {
            if status.success() {
                return Ok(());
            }
            bail!("host process exited with status {status}");
        }

        let event = match rx.recv_timeout(HOST_EXIT_POLL) {
            Ok(event) => event?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("file watcher disconnected unexpectedly")
            }
        };

        let mut paths = relevant_paths(&event);
        let deadline = Instant::now() + DEBOUNCE_WINDOW;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(event) => paths.extend(relevant_paths(&event?)),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("file watcher disconnected unexpectedly")
                }
            }
        }

        let changes = classify_changes(&session, &paths, native);
        if changes.changed_modules.is_empty() && !changes.restart_host {
            continue;
        }

        if native {
            println!("change detected; restarting native host...");
            host.restart(&session, true)?;
            continue;
        }

        if changes.rebuild_all_guest_modules {
            println!("workspace API/host config changed; rebuilding guest modules...");
            if let Err(err) = rebuild_and_stage_modules(&session, &session.module_specs) {
                eprintln!("guest rebuild failed; keeping current host running: {err:#}");
                continue;
            }
        } else if !changes.changed_modules.is_empty() {
            let specs = session
                .module_specs
                .iter()
                .filter(|spec| changes.changed_modules.contains(spec.id.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            println!(
                "module source changed: {}",
                specs
                    .iter()
                    .map(|spec| spec.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if let Err(err) = rebuild_and_stage_modules(&session, &specs) {
                eprintln!("guest rebuild failed; old module generation stays active: {err:#}");
                continue;
            }
        }

        if changes.restart_host {
            println!("restarting host...");
            host.restart(&session, false)?;
        }
    }
}

fn load_module_build_specs(manifest: &WorkspaceManifest) -> Result<Vec<ModuleBuildSpec>> {
    manifest
        .default_world
        .active_modules
        .iter()
        .map(|module_id| {
            let entry = manifest
                .inventory
                .module(module_id)
                .ok_or_else(|| anyhow!("missing workspace inventory entry for `{module_id}`"))?;
            let cargo_toml_path = entry.path.join("Cargo.toml");
            let contents = fs::read_to_string(&cargo_toml_path).with_context(|| {
                format!(
                    "failed to read module manifest at {}",
                    cargo_toml_path.display()
                )
            })?;
            let value: toml::Value = toml::from_str(&contents).with_context(|| {
                format!(
                    "failed to parse module manifest at {}",
                    cargo_toml_path.display()
                )
            })?;

            let package_name = value
                .get("package")
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str)
                .ok_or_else(|| anyhow!("{} is missing [package].name", cargo_toml_path.display()))?
                .to_string();

            let artifact_stem = value
                .get("lib")
                .and_then(toml::Value::as_table)
                .and_then(|table| table.get("name"))
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| package_name.replace('-', "_"));

            Ok(ModuleBuildSpec {
                id: module_id.clone(),
                package_name,
                crate_path: entry.path.clone(),
                artifact_stem: artifact_stem.clone(),
                built_wasm: manifest
                    .root
                    .join("target")
                    .join(WASM_TARGET)
                    .join("debug")
                    .join(format!("{artifact_stem}.wasm")),
                staged_wasm: manifest
                    .root
                    .join("assets")
                    .join("modules")
                    .join(format!("{}.wasm", module_id.as_str())),
            })
        })
        .collect()
}

fn rebuild_and_stage_modules(session: &DevSession, specs: &[ModuleBuildSpec]) -> Result<()> {
    if specs.is_empty() {
        return Ok(());
    }

    let workspace_cargo = session.manifest.root.join("Cargo.toml");
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&workspace_cargo)
        .arg("--target")
        .arg(WASM_TARGET)
        .current_dir(&session.manifest.root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    for spec in specs {
        command.arg("-p").arg(&spec.package_name);
    }

    let status = command.status().with_context(|| {
        format!(
            "failed to run cargo build for workspace {}",
            workspace_cargo.display()
        )
    })?;
    if !status.success() {
        bail!("cargo build failed with status {status}");
    }

    for spec in specs {
        stage_module_artifact(spec)?;
        println!(
            "staged module {} -> {}",
            spec.id,
            spec.staged_wasm.display()
        );
    }

    Ok(())
}

fn stage_module_artifact(spec: &ModuleBuildSpec) -> Result<()> {
    let parent = spec
        .staged_wasm
        .parent()
        .expect("module staged wasm always has a parent");
    fs::create_dir_all(parent)?;

    let temp_path = spec.staged_wasm.with_extension("wasm.tmp");
    fs::copy(&spec.built_wasm, &temp_path).with_context(|| {
        format!(
            "failed to copy built wasm from {} to {}",
            spec.built_wasm.display(),
            temp_path.display()
        )
    })?;

    match fs::rename(&temp_path, &spec.staged_wasm) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(parent)?;
            fs::copy(&spec.built_wasm, &spec.staged_wasm).with_context(|| {
                format!(
                    "failed to recover by copying staged wasm into place at {}",
                    spec.staged_wasm.display()
                )
            })?;
            let _ = fs::remove_file(&temp_path);
            Ok(())
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to move staged wasm into place at {}",
                spec.staged_wasm.display()
            )
        }),
    }
}

fn create_watcher(
    session: &DevSession,
) -> Result<(RecommendedWatcher, mpsc::Receiver<notify::Result<Event>>)> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })?;

    if let Some(api) = &session.manifest.api {
        watch_crate_paths(&mut watcher, api)?;
    }
    if let Some(host) = &session.manifest.host {
        watch_crate_paths(&mut watcher, host)?;
    }
    for spec in &session.module_specs {
        watch_crate_paths(&mut watcher, &spec.crate_path)?;
    }

    Ok((watcher, rx))
}

fn watch_crate_paths(watcher: &mut RecommendedWatcher, crate_dir: &Path) -> Result<()> {
    let src = crate_dir.join("src");
    if src.exists() {
        watcher.watch(&src, RecursiveMode::Recursive)?;
    }

    Ok(())
}

fn relevant_paths(event: &Event) -> Vec<PathBuf> {
    match &event.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write))
        | EventKind::Create(_)
        | EventKind::Modify(_)
        | EventKind::Remove(_)
        | EventKind::Any => event
            .paths
            .iter()
            .filter(|path| is_rust_source_path(path))
            .cloned()
            .collect(),
        EventKind::Access(_) | EventKind::Other => Vec::new(),
    }
}

fn is_rust_source_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "rs")
}

fn classify_changes(session: &DevSession, paths: &[PathBuf], native: bool) -> ChangeSet {
    let mut changes = ChangeSet::default();

    for path in paths {
        if let Some(host) = &session.manifest.host
            && path.starts_with(host)
        {
            changes.restart_host = true;
        }

        if let Some(api) = &session.manifest.api
            && path.starts_with(api)
        {
            changes.restart_host = true;
            changes.rebuild_all_guest_modules = !native;
        }

        for spec in &session.module_specs {
            if path.starts_with(&spec.crate_path) {
                if native {
                    changes.restart_host = true;
                } else {
                    changes.changed_modules.insert(spec.id.as_str().to_string());
                }
            }
        }
    }

    changes
}

struct HostProcess {
    child: Child,
}

impl HostProcess {
    fn spawn(session: &DevSession, native: bool) -> Result<Self> {
        let mut command = Command::new("cargo");
        command
            .arg("run")
            .arg("--manifest-path")
            .arg(&session.host_manifest_path)
            .current_dir(&session.manifest.root)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let rust_log = match std::env::var("RUST_LOG") {
            Ok(value) if value.contains("bevy_asset::server") => value,
            Ok(value) => format!("{value},bevy_asset::server=warn"),
            Err(_) => "info,bevy_asset::server=warn".to_string(),
        };
        command.env("RUST_LOG", rust_log);

        if !native {
            command.arg("--features").arg("bevy/file_watcher");
        }
        if native {
            command.arg("--").arg("--native");
        }

        let child = command.spawn().with_context(|| {
            format!(
                "failed to launch host via {}",
                session.host_manifest_path.display()
            )
        })?;
        Ok(Self { child })
    }

    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().context("failed to poll host process")
    }

    fn restart(&mut self, session: &DevSession, native: bool) -> Result<()> {
        self.stop()?;
        thread::sleep(Duration::from_millis(100));
        *self = Self::spawn(session, native)?;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill().context("failed to stop host process")?;
            let _ = self.child.wait();
        }
        Ok(())
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_module_build_specs_uses_package_and_lib_names() {
        let dir = std::env::temp_dir().join(format!("wasvy-dev-specs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("crates/modules/combat/src")).unwrap();
        fs::create_dir_all(dir.join("crates/game_host")).unwrap();
        fs::write(
            dir.join("wasvy.toml"),
            r#"
[workspace]
host = "crates/game_host"

[[module]]
name = "combat"
path = "crates/modules/combat"
"#,
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers=[]\nresolver=\"2\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/game_host/Cargo.toml"),
            "[package]\nname=\"host\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/modules/combat/Cargo.toml"),
            "[package]\nname=\"combat-package\"\nversion=\"0.1.0\"\nedition=\"2024\"\n\n[lib]\nname=\"combat_guest_name\"\ncrate-type=[\"cdylib\"]\n",
        )
        .unwrap();

        let session = load_dev_session(dir.join("wasvy.toml")).unwrap();
        assert_eq!(session.module_specs.len(), 1);
        let spec = &session.module_specs[0];
        assert_eq!(spec.package_name, "combat-package");
        assert_eq!(spec.artifact_stem, "combat_guest_name");
        assert!(spec.staged_wasm.ends_with("assets/modules/combat.wasm"));
        assert!(
            spec.built_wasm
                .ends_with("target/wasm32-wasip2/debug/combat_guest_name.wasm")
        );
    }

    #[test]
    fn classify_changes_rebuilds_changed_guest_module_only() {
        let dir = std::env::temp_dir().join(format!("wasvy-dev-changes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("crates/modules/combat/src")).unwrap();
        fs::create_dir_all(dir.join("crates/game_host")).unwrap();
        fs::write(
            dir.join("wasvy.toml"),
            r#"
[workspace]
host = "crates/game_host"

[[module]]
name = "combat"
path = "crates/modules/combat"
"#,
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers=[]\nresolver=\"2\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/game_host/Cargo.toml"),
            "[package]\nname=\"host\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/modules/combat/Cargo.toml"),
            "[package]\nname=\"combat\"\nversion=\"0.1.0\"\nedition=\"2024\"\n\n[lib]\ncrate-type=[\"cdylib\"]\n",
        )
        .unwrap();
        fs::write(dir.join("crates/modules/combat/src/lib.rs"), "// changed").unwrap();

        let session = load_dev_session(dir.join("wasvy.toml")).unwrap();
        let changes = classify_changes(
            &session,
            &[dir.join("crates/modules/combat/src/lib.rs")],
            false,
        );

        assert!(changes.changed_modules.contains("combat"));
        assert!(!changes.restart_host);
        assert!(!changes.rebuild_all_guest_modules);
    }

    #[test]
    fn classify_changes_restarts_host_for_api_changes() {
        let dir = std::env::temp_dir().join(format!("wasvy-dev-api-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("crates/modules/combat/src")).unwrap();
        fs::create_dir_all(dir.join("crates/game_host")).unwrap();
        fs::create_dir_all(dir.join("crates/game_api/src")).unwrap();
        fs::write(
            dir.join("wasvy.toml"),
            r#"
[workspace]
host = "crates/game_host"
api = "crates/game_api"

[[module]]
name = "combat"
path = "crates/modules/combat"
"#,
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers=[]\nresolver=\"2\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/game_host/Cargo.toml"),
            "[package]\nname=\"host\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/modules/combat/Cargo.toml"),
            "[package]\nname=\"combat\"\nversion=\"0.1.0\"\nedition=\"2024\"\n\n[lib]\ncrate-type=[\"cdylib\"]\n",
        )
        .unwrap();
        fs::write(dir.join("crates/game_api/src/lib.rs"), "// changed").unwrap();

        let session = load_dev_session(dir.join("wasvy.toml")).unwrap();
        let changes = classify_changes(&session, &[dir.join("crates/game_api/src/lib.rs")], false);

        assert!(changes.restart_host);
        assert!(changes.rebuild_all_guest_modules);
    }

    #[test]
    fn relevant_paths_only_returns_rust_sources() {
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![
                PathBuf::from("src/lib.rs"),
                PathBuf::from("src/lib.rs.swp"),
                PathBuf::from("Cargo.toml"),
            ],
            attrs: Default::default(),
        };

        assert_eq!(relevant_paths(&event), vec![PathBuf::from("src/lib.rs")]);
    }

    #[test]
    fn stage_module_artifact_creates_destination_file() {
        let dir = std::env::temp_dir().join(format!("wasvy-dev-stage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("target/wasm32-wasip2/debug")).unwrap();
        let built = dir.join("target/wasm32-wasip2/debug/counter.wasm");
        fs::write(&built, b"wasm-bytes").unwrap();

        let spec = ModuleBuildSpec {
            id: ModuleId::new("counter"),
            package_name: "counter".to_string(),
            crate_path: dir.join("crates/modules/counter"),
            artifact_stem: "counter".to_string(),
            built_wasm: built,
            staged_wasm: dir.join("assets/modules/counter.wasm"),
        };

        stage_module_artifact(&spec).unwrap();
        assert_eq!(fs::read(&spec.staged_wasm).unwrap(), b"wasm-bytes");
    }
}
