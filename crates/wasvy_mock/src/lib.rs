use std::{
    mem,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU16, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bevy_app::{PluginsState, prelude::*};
use bevy_asset::AssetPlugin;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_internal::MinimalPlugins;
use bevy_platform::thread::sleep;
use bevy_remote::http::{DEFAULT_PORT, RemoteHttpPlugin};
use clap::Parser;
use wasvy::prelude::{Devtools, ModLoaderPlugin};
use wasvy_cli::{cli::Args, remote::RemoteUri, source::Source};

const WAIT: Duration = Duration::from_millis(10);

/// Prepare and run a [Mock] Bevy app running [ModLoaderPlugin].
///
/// Construct via [MockConfig::default()], use like a Bevy [App].
#[derive(Default, Debug, Deref, DerefMut)]
pub struct MockApp {
    #[deref]
    app: App,
    devtools: Option<Devtools>,
}

impl MockApp {
    pub fn devtools(mut self, devtools: impl Into<Devtools>) -> Self {
        self.devtools = Some(devtools.into());
        self
    }

    #[must_use = "The returned handle will end execution of the app when dropped"]
    pub fn run(self) -> Mock {
        let Self { mut app, devtools } = self;
        let mut devtools = devtools.unwrap_or_default();
        devtools.program_name = "wasvy-test-host".into();
        let port = next_test_port();
        let uri = RemoteUri::new(port);

        let exit = Arc::new(AtomicBool::new(false));
        let (ready_sender, ready_receiver) = mpsc::channel();

        // App cannot be moved between threads, but SubApps can be
        let sub_apps = mem::take(app.sub_apps_mut());

        let exit_clone = exit.clone();
        let join = thread::spawn(move || {
            let mut app = App::empty();
            let _ = mem::replace(app.sub_apps_mut(), sub_apps);

            #[derive(Component)]
            struct Ready(mpsc::Sender<()>);

            app.world_mut().spawn(Ready(ready_sender));

            fn ready(sender: Single<&mut Ready>) -> Result {
                sender.0.send(())?;
                Ok(())
            }

            #[derive(Component)]
            struct Exit(Arc<AtomicBool>);

            app.world_mut().spawn(Exit(exit_clone));

            fn exit(mut exits: MessageWriter<AppExit>, signal: Single<&Exit>) {
                if signal.0.load(Ordering::Relaxed) {
                    exits.write(AppExit::Success);
                }
            }

            app.add_plugins((
                MinimalPlugins,
                AssetPlugin {
                    file_path: "target/tests/host_assets".into(),
                    processed_file_path: "target/tests/host_assets/processed".into(),
                    ..Default::default()
                },
                RemoteHttpPlugin::default().with_port(port),
                ModLoaderPlugin::default().devtools(devtools),
            ))
            .add_systems(PostStartup, ready)
            .add_systems(Last, exit);

            let (sender, receiver) = mpsc::channel();

            // chore: keep up to date with `impl Plugin for ScheduleRunnerPlugin`
            app.set_runner(move |mut app: App| {
                assert_eq!(app.plugins_state(), PluginsState::Ready);
                app.finish();
                app.cleanup();

                loop {
                    let start_time = Instant::now();

                    app.update();

                    if let Some(exit) = app.should_exit() {
                        // Handoff world instead of dropping it along with the app
                        let world = mem::replace(app.world_mut(), World::new());
                        sender.send(world).unwrap();

                        return exit;
                    };

                    let end_time = Instant::now();

                    let exe_time = end_time - start_time;
                    if exe_time < WAIT {
                        sleep(WAIT - exe_time);
                    }
                }
            });

            let exit = app.run();
            if exit.is_error() {
                panic!("App exit {exit:?}");
            }

            // Handoff world
            receiver.recv().unwrap()
        });

        ready_receiver
            .recv_timeout(Duration::from_millis(50))
            .expect("Host app ready");

        Mock {
            uri,
            exit,
            cleanup: Vec::new(),
            join: Some(join),
        }
    }
}

pub struct Mock {
    uri: RemoteUri,
    exit: Arc<AtomicBool>,
    cleanup: Vec<Source>,
    join: Option<JoinHandle<World>>,
}

impl Mock {
    pub fn uri(&self) -> RemoteUri {
        self.uri.clone()
    }

    /// Run a cli command with the connected host
    pub fn cli(
        &mut self,
        args: impl TryInto<MockArgs, Error = impl Into<anyhow::Error>>,
    ) -> anyhow::Result<()> {
        let MockArgs(mut args) = args.try_into().map_err(Into::into)?;
        if args.uri.is_none() {
            args.uri = Some(self.uri.to_string());
        }
        let require_cleanup = matches!(&args.command, Some(wasvy_cli::cli::Command::Create(_)));
        let mut sources = wasvy_cli::cli::cli(args)?;
        if require_cleanup {
            self.cleanup.append(&mut sources);
        }

        Ok(())
    }

    /// Attempts to exit the host app as quickly as possible
    pub fn exit(mut self) -> World {
        self.exit_inner()
    }

    /// Exits the host app once duration has elapsed
    pub fn wait(mut self, duration: Duration) -> World {
        self.wait_inner(duration, false)
    }

    fn exit_inner(&mut self) -> World {
        self.exit.store(true, Ordering::Relaxed);
        self.wait_inner(WAIT * 2, true)
    }

    fn wait_inner(&mut self, duration: Duration, last: bool) -> World {
        let start = Instant::now();
        while start.elapsed() < duration {
            sleep(WAIT);
            if self.join.as_ref().unwrap().is_finished() {
                let world = self.join.take().unwrap().join().unwrap();
                return world;
            }
        }

        if last {
            panic!("App did not exit")
        } else {
            self.exit_inner()
        }
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        for source in self.cleanup.drain(..) {
            let _ = source.delete();
        }
        if self.join.is_some() {
            self.exit_inner();
        }
    }
}

pub struct MockArgs(Args);

impl From<Args> for MockArgs {
    fn from(value: Args) -> Self {
        Self(value)
    }
}

impl TryFrom<&'static str> for MockArgs {
    type Error = anyhow::Error;

    fn try_from(value: &'static str) -> std::result::Result<Self, anyhow::Error> {
        let args =
            shlex::split(value).ok_or_else(|| anyhow::anyhow!("invalid shell string: {value}"))?;
        let value = Args::try_parse_from(args)?;
        Ok(value.into())
    }
}

#[test]
fn host_exit() {
    #[derive(Resource, Debug, PartialEq)]
    struct State(u32);

    let mut host = MockApp::default();
    host.add_systems(Startup, |mut commands: Commands| {
        commands.insert_resource(State(1234));
    });

    let app = host.run();

    let world = app.exit();
    assert_eq!(world.resource::<State>(), &State(1234));
}

static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(DEFAULT_PORT + 1);

// Prevent conflicts between two different tests
pub fn next_test_port() -> u16 {
    NEXT_TEST_PORT.fetch_add(1, Ordering::Relaxed)
}

#[test]
fn host_wait() {
    #[derive(Resource, Debug, PartialEq)]
    struct Count(u32);

    let mut host = MockApp::default();
    host.insert_resource(Count(0));
    host.add_systems(Update, |mut count: ResMut<Count>| {
        count.0 += 1;
    });

    let app = host.run();

    let world = app.wait(Duration::from_millis(50)); // wait 50ms + WAIT = 60ms or 6 updates
    let count = world.resource::<Count>().0;
    assert_eq!(count, 6);
}
