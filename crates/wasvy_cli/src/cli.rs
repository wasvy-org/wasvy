use anyhow::{Context, Result, bail};
use clap::Parser;
use derive_more::Display;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    command::Logging,
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
    New(NewArgs),

    /// Load mod sources and then watch for changes
    Dev(DevArgs),

    /// Searches the filesystem for compatible sources for the remote app
    List(ModArgs),

    /// Compiles and loads one or more mods into the remote app
    Load(ModArgs),

    /// Unloads one or more mods from the remote app
    Unload(ModArgs),
}

impl Default for Command {
    fn default() -> Self {
        Self::Dev(DevArgs::default())
    }
}

#[derive(clap::Args, Debug, Eq, PartialEq)]
pub struct NewArgs {
    /// What language to use
    #[arg(short, long, default_value = "rust")]
    pub language: String,

    /// The project name
    #[arg(default_value = "my-bevy-mod")]
    pub name: String,
}

#[derive(clap::Args, Debug, Eq, PartialEq, Default)]
pub struct ModArgs {
    /// One or more patterns to filter sources
    #[arg(short, long)]
    pub mods: Vec<String>,
}

#[derive(clap::Args, Debug, Eq, PartialEq, Default)]
pub struct DevArgs {
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
        Command::New(args) => {
            let source = new(path, args, &runtime, logging)?;
            Ok(vec![source])
        }
        Command::List(mods) => {
            let mut sources = get_sources(&runtime, mods, &remote, path)?;
            if sources.is_empty() {
                bail!("no source was found");
            }
            list(&mut sources);
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
        Command::Dev(args) => {
            let sources = get_sources(&runtime, &args.mods, &remote, path)?;
            remote.load(&sources, logging.clone())?;

            let timeout = args
                .timeout
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(30 * 24 * 60 * 60));

            remote.watch(&sources, timeout, args.count, logging)?;
            Ok(sources)
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

fn new(path: &Path, args: &NewArgs, runtime: &Runtime, logging: Logging) -> Result<Source> {
    let input = args.language.to_lowercase();
    let Some((language, name)) = runtime
        .languages()
        .iter()
        .filter(|(_, (_, synonyms))| synonyms.contains(&input))
        .map(|(id, (language, _))| (id.clone(), language.name().to_string()))
        .next()
    else {
        return Err(InvalidLanguageError {
            language: args.language.clone(),
            runtime: runtime.clone(),
        }
        .into());
    };

    println!("Scaffolding new {name} mod \"{}\"", &args.name);

    if !path.is_dir() {
        bail!("Invalid path: {path:?}");
    }

    let directory = path.join(&args.name);
    if directory.exists() {
        bail!("Directory already exists: {directory:?}")
    }
    fs::create_dir_all(&directory).with_context(|| format!("Creating directory {directory:?}"))?;

    runtime.scaffold(&args.name, directory, language, logging)
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

fn list(sources: &mut [Source]) {
    sources.sort_by(|a, b| a.name().cmp(b.name()));
    for source in sources.iter() {
        println!("- {source}");
    }
}
