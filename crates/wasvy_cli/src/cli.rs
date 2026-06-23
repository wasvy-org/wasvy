use anyhow::{Context, Result, bail};
use clap::Parser;
use derive_more::Display;
use error_collection::Errors;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    command::Logging,
    id::Id,
    named::Named,
    remote::{Remote, RemoteUri},
    runtime::Runtime,
    source::Source,
};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short, long, default_value = ".")]
    pub path: PathBuf,

    /// A pattern for the name of the remote app, defined by the devtools config
    #[arg(short, long)]
    pub app: Option<String>,

    /// An alternate remote address
    #[arg(long)]
    pub uri: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self::parse_from(["wasvy"])
    }
}

impl From<Command> for Args {
    fn from(value: Command) -> Self {
        Self {
            command: Some(value),
            ..Default::default()
        }
    }
}

#[derive(clap::Subcommand, Debug, Eq, PartialEq)]
pub enum Command {
    /// Creates a new mod source
    Create(CreateArgs),

    /// Searches the filesystem for compatible sources for the remote app
    Search(ModArgs),

    /// Compiles and loads one or more mods into the remote app
    Load(ModArgs),

    /// Unloads one or more mods from the remote app
    Unload(ModArgs),

    /// Watch mod sources for changes and compile
    Watch(WatchArgs),
}

impl Default for Command {
    fn default() -> Self {
        Self::Watch(WatchArgs::default())
    }
}

#[derive(clap::Args, Debug, Eq, PartialEq)]
pub struct CreateArgs {
    /// What language to use
    #[arg(short, long, default_value = "rust")]
    pub language: String,

    /// The project name
    #[arg(short, long, default_value = "my-bevy-mod")]
    pub name: String,
}

#[derive(clap::Args, Debug, Eq, PartialEq, Default)]
pub struct ModArgs {
    /// One or more patterns to filter sources
    #[arg(short, long)]
    pub mods: Vec<String>,
}

#[derive(clap::Args, Debug, Eq, PartialEq, Default)]
pub struct WatchArgs {
    #[command(flatten)]
    pub mods: ModArgs,

    /// Exit after this many seconds
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Exit after this many reloads
    #[arg(long)]
    pub count: Option<usize>,
}

pub fn cli(args: Args, logging: Logging) -> Result<Vec<Source>> {
    let Args {
        command,
        path,
        app,
        uri,
    } = &args;
    let default = Command::default();
    let command = command.as_ref().unwrap_or(&default);
    let uri: RemoteUri = uri
        .as_ref()
        .and_then(|a| a.parse().ok())
        .unwrap_or_default();
    let remote = Remote::connect(uri)
        .context("No remote found!\nIs your Bevy app running with wasvy devtools enabled?")?;

    // Assert remote is the correct one
    let name = &remote.name;
    if let Some(pattern) = app
        && !name.contains(pattern.as_str())
    {
        bail!("remote server \"{name}\" and pattern \"{pattern}\" do not match");
    }

    let runtime = Runtime::new(&remote).context("initializing runtime")?;

    match command {
        Command::Create(args) => {
            let source = create(path, args, &runtime)?;
            Ok(vec![source])
        }
        Command::Search(mods) => {
            let mut sources = get_sources(&runtime, mods, &remote, path)?;
            if sources.is_empty() {
                bail!("no source was found");
            }
            print_search(&mut sources);
            Ok(sources)
        }
        Command::Load(mods) => {
            let sources = get_sources(&runtime, mods, &remote, path)?;
            remote.load(&sources, logging)?;
            Ok(Vec::new()) // TODO: the user probably expects these to be the built, loaded sources
        }
        Command::Unload(mods) => {
            let sources = get_sources(&runtime, mods, &remote, path)?;
            remote.unload(&sources, logging)?;
            Ok(Vec::new()) // TODO: the user probably expects these to be the unloaded sources
        }
        Command::Watch(args) => {
            let sources = get_sources(&runtime, &args.mods, &remote, path)?;
            remote.load(&sources, logging.clone())?;

            let timeout = args
                .timeout
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(30 * 24 * 60 * 60));

            remote.watch(&sources, timeout, args.count, logging)?;
            Ok(Vec::new()) // TODO: the user probably expects these to be the watched sources
        }
    }
}

fn get_sources(
    runtime: &Runtime,
    mods: &ModArgs,
    remote: &Remote,
    path: &Path,
) -> Result<Vec<Source>> {
    let ModArgs { mods } = mods;

    let mut sources = runtime.search(remote, path)?;
    if !mods.is_empty() {
        sources.retain(|source| mods.iter().any(|pattern| source.name().contains(pattern)));
    }

    Ok(sources)
}

fn create(path: &Path, args: &CreateArgs, runtime: &Runtime) -> Result<Source> {
    let mut errors = Errors::new();

    let language = args.language.to_lowercase();
    let matches: Vec<Id> = runtime
        .languages()
        .iter()
        .filter(|(_, (_, synonyms))| synonyms.contains(&language))
        .map(|(id, _)| id)
        .cloned()
        .collect();
    let language = if matches.len() == 1 {
        matches.first()
    } else {
        errors.push(InvalidLanguageError {
            language: args.language.clone(),
            runtime: runtime.clone(),
        });
        None
    };

    let directory = path.join(&args.name);
    let mut source = None;
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
            source = errors.collect(runtime.scaffold(
                &args.name,
                directory,
                language,
                Default::default(),
            ));
        }
    }

    errors.as_result().map(|_| source.unwrap())
}

#[derive(Display)]
#[display("Language \"{language}\" is not a valid choice")]
struct InvalidLanguageError {
    language: String,
    runtime: Runtime,
}

impl std::error::Error for InvalidLanguageError {}

impl fmt::Debug for InvalidLanguageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { language, runtime } = self;
        write!(
            f,
            "Language \"{language}\" is not a valid choice. Options include:"
        )?;

        for (lang, synonyms) in runtime.languages().values() {
            write!(f, "\n{}", lang.name())?;
            if !synonyms.is_empty() {
                write!(f, " ({})", synonyms.join(", "))?;
            }
        }

        Ok(())
    }
}

fn print_search(sources: &mut [Source]) {
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
