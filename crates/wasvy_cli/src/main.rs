use wasvy_cli::{builder::Config, languages::*};

fn main() {
    // Determine if we can plug into a running bevy app
    // if we can then load its dependencies and then indentify/search
    let mut config = Config::default();
    config.add_language(Rust::default());
    config.add_language(Python);
    let builder = config.build();

    // Open up the current source
    let source = builder.identify(".");
    let sources = builder.search(".");
}
