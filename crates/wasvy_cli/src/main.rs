#[cfg(not(feature = "cli"))]
compile_error!("The `cli` feature must be enabled to build wasvy-cli.");

use anyhow::{Context, Result, bail};
use clap::Parser;
use error_collection::Errors;
use std::{
    fs,
    path::{Path, PathBuf},
    process::exit,
};

use wasvy_cli::{
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

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Opens the Wasvy Terminal User Interface
    TUI,

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

#[derive(clap::Args, Debug)]
struct CreateArgs {
    /// What language to use
    #[arg(short, long, default_value = "rust")]
    language: String,

    /// The project name
    #[arg(short, long, default_value = "my-bevy-mod")]
    name: String,
}

#[derive(clap::Args, Debug)]
struct ModArgs {
    /// One or more patterns to filter sources
    #[arg(short, long)]
    mods: Vec<String>,
}

mod tui;

pub fn main() {
    let args = Args::parse();

    if matches!(args.command, None | Some(Command::TUI)) {
        println!("Starting the TUI");
        if matches!(args.command, None) {
            println!("Looking for the help menu? Try `wasvy-cli --help` instead");
        }
        tui::main();
        return;
    } else if let Err(err) = cli(args) {
        eprintln!("Error: {err:?}");
        exit(1)
    }
}

fn cli(args: Args) -> Result<()> {
    let Args { command, path, app } = &args;
    let remote = Remote::connect(RemoteEndpoint::default())
        .context("no remote found\nIs your Bevy app running with wasvy devtools enabled?")?;

    // Assert remote is the correct one
    let name = &remote.name;
    if let Some(pattern) = app
        && !name.contains(pattern.as_str())
    {
        bail!("remote server \"{name}\" and pattern \"{pattern}\" do not match");
    }

    // Create the cli runtime
    let runtime = Runtime::new(&remote).context("initializing runtime")?;

    // Find and filter sources
    let sources = if let Some(Command::Search(args))
    | Some(Command::Load(args))
    | Some(Command::Unload(args))
    | Some(Command::Watch(args)) = command
    {
        let mut sources = runtime.search(path)?;
        if !args.mods.is_empty() {
            sources = sources
                .into_iter()
                .filter(|source| {
                    args.mods
                        .iter()
                        .any(|pattern| source.name().contains(pattern))
                })
                .collect();
        }

        // For most commands we need at least one valid source
        if sources.is_empty() && !matches!(command, Some(Command::Search { .. })) {
            bail!("no source was found");
        }

        sources
    } else {
        Vec::new()
    };

    match command {
        Some(Command::Create(args)) => create(path, args, &runtime)?,
        Some(Command::Search(_)) => print_sources(sources),
        Some(Command::Load(_)) => todo!(),
        Some(Command::Unload(_)) => todo!(),
        Some(Command::Watch(_)) => todo!(),
        None | Some(Command::TUI) => unreachable!(),
    };

    Ok(())
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
            errors.collect(runtime.create(&args.name, directory, language));
        }
    }

    errors.as_result()
}

fn print_sources(mut sources: Vec<Source>) {
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
}
