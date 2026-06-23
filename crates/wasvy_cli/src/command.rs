use std::process::{self, Stdio};

use anyhow::{Result, anyhow, bail};
use derive_more::{Deref, DerefMut};

use crate::source::Source;

/// A domain-specific Command util for commands acting on specific [Source]s.
#[derive(Deref, DerefMut)]
pub struct Command(process::Command);

impl Command {
    pub fn run<T>(ty: T, source: &Source, logging: Logging) -> Result<()>
    where
        T: CommandType,
    {
        Self::new(ty, source, logging)?.execute()
    }

    pub fn new<T>(ty: T, source: &Source, logging: Logging) -> Result<Self>
    where
        T: CommandType,
    {
        let mut command = Self(process::Command::new(T::PROGRAM));
        command
            .current_dir(source.path())
            .stdout(logging.stdio())
            .stderr(logging.stdio());
        ty.setup(&mut command, source)?;
        Ok(command)
    }

    pub fn execute(&mut self) -> Result<()> {
        let command = &mut self.0;
        let status = command
            .status()
            .map_err(|error| anyhow!("> {command:?}\n Failed with {error:?}"))?;

        if !status.success() {
            bail!("> {command:?}\n  Failed with {status}");
        }

        Ok(())
    }
}

/// Logging mode for commands
///
/// Default is to ignore all logs
#[derive(Debug, Default, Clone)]
pub enum Logging {
    Inherit,
    #[default]
    Ignore,
    Capture,
}

impl Logging {
    pub fn println(&self, value: impl AsRef<str>) {
        match self {
            Logging::Inherit => println!("{}", value.as_ref()),
            Logging::Capture => todo!(),
            Logging::Ignore => {}
        }
    }

    pub fn eprintln(&self, value: impl AsRef<str>) {
        match self {
            Logging::Inherit => eprintln!("{}", value.as_ref()),
            Logging::Capture => todo!(),
            Logging::Ignore => {}
        }
    }

    fn stdio(&self) -> Stdio {
        match self {
            Logging::Inherit => Stdio::inherit(),
            Logging::Capture => Stdio::piped(),
            Logging::Ignore => Stdio::null(),
        }
    }
}

pub trait CommandType {
    const PROGRAM: &str;

    fn setup(self, command: &mut Command, source: &Source) -> Result<()>;
}
