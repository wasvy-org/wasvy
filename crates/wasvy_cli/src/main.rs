use std::path::PathBuf;

use wasvy_cli::{
    dev::{load_dev_session, render_dev_plan, run_dev},
    remote::Remote,
    runtime::{Config, Runtime},
    source::Source,
};

fn main() {
    match parse_args(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Command::Dev { native, manifest } => {
            if std::env::var_os("WASVY_DEV_PLAN_ONLY").is_some() {
                match load_dev_session(&manifest) {
                    Ok(session) => println!("{}", render_dev_plan(&session, native)),
                    Err(err) => {
                        eprintln!("{err:#}");
                        std::process::exit(1);
                    }
                }
            } else if let Err(err) = run_dev(&manifest, native) {
                eprintln!("{err:#}");
                std::process::exit(1);
            }
        }
        Command::Remote => run_remote_workflow(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Dev { native: bool, manifest: PathBuf },
    Remote,
}

fn parse_args(args: &[String]) -> Command {
    let mut native = false;
    let mut manifest = PathBuf::from("wasvy.toml");

    if args.first().is_none_or(|arg| arg == "remote") {
        return Command::Remote;
    }

    if args.first().is_some_and(|arg| arg == "dev") {
        for arg in &args[1..] {
            match arg.as_str() {
                "--native" => native = true,
                value => manifest = PathBuf::from(value),
            }
        }
        Command::Dev { native, manifest }
    } else {
        Command::Remote
    }
}

fn run_remote_workflow() {
    let Ok(remote) = Remote::new() else {
        println!("There's no bevy apps running");
        return;
    };

    let runtime = {
        let mut config = Config::default();
        for dep in remote.dependencies.iter() {
            let name = format!("{dep}");
            if let Err(err) = config.add_dependency(dep) {
                println!("Could not resolve remote dependency {name} because: {err:?}");
                return;
            }
        }

        config.add_all_editors();
        config.add_all_languages();

        Runtime::new(config)
    };

    if let Some(source) = runtime.identify(".") {
        handle_sources(vec![source])
    } else {
        match runtime.search(".") {
            Err(err) => println!("No compatible sources found. Error reading file system: {err:?}"),
            Ok(sources) if sources.is_empty() => println!("No sources found."),
            Ok(sources) => handle_sources(sources),
        }
    }
}

fn handle_sources(_sources: Vec<Source>) {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_dev_args_defaults_manifest() {
        assert_eq!(
            parse_args(&["dev".to_string()]),
            Command::Dev {
                native: false,
                manifest: PathBuf::from("wasvy.toml"),
            }
        );
    }

    #[test]
    fn parse_dev_args_supports_native_and_custom_manifest() {
        assert_eq!(
            parse_args(&[
                "dev".to_string(),
                "--native".to_string(),
                "game.wasvy.toml".to_string(),
            ]),
            Command::Dev {
                native: true,
                manifest: PathBuf::from("game.wasvy.toml"),
            }
        );
    }

    #[test]
    fn render_dev_plan_uses_manifest_modules() {
        let dir = std::env::temp_dir().join(format!("wasvy-cli-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("crates/modules/combat/src")).unwrap();
        fs::create_dir_all(dir.join("crates/game_host")).unwrap();
        let manifest = dir.join("wasvy.toml");
        fs::write(
            &manifest,
            r#"
[workspace]
host = "crates/game_host"

[[module]]
name = "combat"
path = "crates/modules/combat"

[world]
modules = ["combat"]
"#,
        )
        .unwrap();
        fs::write(
            dir.join("crates/game_host/Cargo.toml"),
            "[package]\nname=\"host\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("crates/modules/combat/Cargo.toml"),
            "[package]\nname=\"combat\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();

        let session = load_dev_session(&manifest).unwrap();
        let plan = render_dev_plan(&session, true);
        assert!(plan.contains("mode: native"));
        assert!(plan.contains("modules: combat"));
    }
}
