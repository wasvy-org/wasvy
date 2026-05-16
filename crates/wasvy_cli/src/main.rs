#[cfg(not(feature = "cli"))]
compile_error!("The `cli` feature must be enabled to build wasvy-cli.");

use anyhow::{Context, Result, bail};
use clap::Parser;
use error_collection::Errors;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Stdio, exit},
};

use wasvy_cli::{
    command::Logging,
    named::Named,
    remote::{Remote, RemoteEndpoint},
    runtime::Runtime,
    source::Source,
};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    /// A pattern for the name of the remote app, defined by the devtools config
    #[arg(short, long)]
    app: Option<String>,
}

#[derive(clap::Subcommand, Debug, Eq, PartialEq)]
enum Command {
    /// Opens the Wasvy Terminal User Interface
    Tui,

    /// Creates a new mod source
    Create(CreateArgs),

    /// Searches the filesystem for compatible sources for the remote app
    Search(ModArgs),

    /// Compiles and loads one or more mods into the remote app
    Load(ModArgs),

    /// Unloads one or more mods from the remote app
    Unload(ModArgs),

    /// Watch mod sources for changes and compile
    Watch(ModArgs),
}

#[derive(clap::Args, Debug, Eq, PartialEq)]
struct CreateArgs {
    /// What language to use
    #[arg(short, long, default_value = "rust")]
    language: String,

    /// The project name
    #[arg(short, long, default_value = "my-bevy-mod")]
    name: String,
}

#[derive(clap::Args, Debug, Eq, PartialEq)]
struct ModArgs {
    /// One or more patterns to filter sources
    #[arg(short, long)]
    mods: Vec<String>,
}

mod tui;

pub fn main() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Wasvy CLI v{version} for Bevy v0.18.0");
    println!();
    let args = Args::parse();

    if matches!(args.command, None | Some(Command::Tui)) {
        println!("Starting the TUI");
        tui::main();
    } else if let Err(err) = cli(args) {
        eprintln!("Error: {err:?}");
        exit(1)
    }
}

fn cli(args: Args) -> Result<()> {
    let Args { command, path, app } = &args;
    let command = command.as_ref().expect("unreachable");
    let remote = Remote::connect(RemoteEndpoint::default(), Stdio::inherit())
        .context("No remote found!\nIs your Bevy app running with wasvy devtools enabled?")?;

    // Assert remote is the correct one
    let name = &remote.name;
    if let Some(pattern) = app
        && !name.contains(pattern.as_str())
    {
        bail!("remote server \"{name}\" and pattern \"{pattern}\" do not match");
    }

    let runtime = Runtime::new(&remote).context("initializing runtime")?;
    let sources = get_sources(&runtime, command, path)?;

    match command {
        Command::Create(args) => create(path, args, &runtime)?,
        Command::Search(_) => search(sources)?,
        Command::Load(_) => remote.load(&sources, Logging::Inherit)?,
        Command::Unload(_) => remote.unload(&sources, Logging::Inherit)?,
        Command::Watch(_) => remote.watch(&sources)?,
        Command::Tui => unreachable!(),
    }

    Ok(())
}

fn get_sources(runtime: &Runtime, command: &Command, path: &Path) -> Result<Vec<Source>> {
    let (Command::Search(ModArgs { mods })
    | Command::Load(ModArgs { mods })
    | Command::Unload(ModArgs { mods })
    | Command::Watch(ModArgs { mods })) = command
    else {
        return Ok(Vec::new());
    };

    let mut sources = runtime.search(path)?;
    if !mods.is_empty() {
        sources.retain(|source| mods.iter().any(|pattern| source.name().contains(pattern)));
    }

    Ok(sources)
}

fn create(path: &Path, args: &CreateArgs, runtime: &Runtime) -> Result<()> {
    let mut errors = Errors::new();

    let language = runtime
        .languages()
        .iter()
        .find(|(_, lang)| lang.name() == args.language)
        .map(|(id, _)| id);
    if language.is_none() {
        errors.push(anyhow::anyhow!(
            "Language {} is not a valid choice. Options include: {:?}",
            args.language,
            runtime
                .languages()
                .values()
                .map(|lang| lang.name())
                .collect::<Vec<_>>()
        ));
    }

    let directory = path.join(&args.name);
    if !path.is_dir() {
        errors.push(anyhow::anyhow!("Invalid path: {path:?}"));
    } else if directory.exists() {
        errors.push(anyhow::anyhow!("Directory already exists: {directory:?}"))
    } else {
        errors.collect(
            fs::create_dir_all(&directory)
                .with_context(|| format!("Creating directory {directory:?}")),
        );

        if let Some(language) = language.cloned() {
            errors.collect(runtime.create(&args.name, directory, language, Logging::Inherit));
        }
    }

    errors.as_result()
}

fn search(mut sources: Vec<Source>) -> Result<()> {
    if sources.is_empty() {
        bail!("no source was found");
    }

    sources.sort_by(|a, b| a.name().cmp(b.name()));
    for source in sources.iter() {
        let name = source.name();
        let path = source.path();
        let language = source
            .language()
            .map(|language| language.name())
            .unwrap_or("wasm");
        eprintln!("{name} - {path:?} ({language})");
    }

    Ok(())
}
