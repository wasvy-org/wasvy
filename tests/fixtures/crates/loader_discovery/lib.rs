mod bindings {
    wit_bindgen::generate!({
        path: ["../../../../interface/wit/wasvy-ecs.wit"],
        inline: "package test:loader; world fixture { include wasvy:ecs/guest@0.0.9; use wasvy:ecs/app@0.0.9.{commands}; export first: func(commands: commands); export second: func(commands: commands); }",
        world: "test:loader/fixture",
        with: { "wasvy:ecs/app@0.0.9": generate },
    });
}

use bindings::{
    Guest,
    wasvy::ecs::app::{App, Commands, Schedule, System},
};

struct Fixture;

impl Guest for Fixture {
    fn setup(app: App) {
        let first = System::new("first");
        first.add_commands();
        app.add_systems(&Schedule::Update, &[&first]);
        app.add_systems(&Schedule::ModStartup, &[&first]);
        if cfg!(feature = "fail-setup") {
            panic!("failed after first declaration");
        }
        let second = System::new("second");
        second.add_commands();
        app.add_systems(&Schedule::Update, &[&second]);
        app.add_systems(&Schedule::Custom("custom".into()), &[&first]);
        first.before(&second);
        second.after(&first);
        let unscheduled = System::new("second");
        first.after(&unscheduled);
    }

    fn first(commands: Commands) {
        commands.spawn_empty();
    }

    fn second(commands: Commands) {
        commands.spawn_empty();
    }
}

bindings::export!(Fixture with_types_in bindings);
