use wasvy_cli::{
    remote::Remote,
    runtime::{Config, Runtime},
    source::Source,
};

fn main() {
    // Connect to a wasvy remote session
    let Ok(remote) = Remote::new() else {
        // TODO: play some animation and wait for game to launch
        println!("There's no bevy apps running");
        return;
    };

    // Create the CLI runtime
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

    // Open up the current source without user confirmation
    if let Some(source) = runtime.identify(".") {
        handle_sources(vec![source])
    }
    // TODO: Offer the user the option to generate a new source while we search in a separate thread
    else {
        match runtime.search(".") {
            Err(err) => println!("No compatible sources found. Error reading file system: {err:?}"),
            Ok(sources) if sources.is_empty() => println!("No sources found."),
            Ok(sources) => {
                // TODO: user should select from sources
                handle_sources(sources)
            }
        }
    }
}

fn handle_sources(_sources: Vec<Source>) {
    todo!()
}
