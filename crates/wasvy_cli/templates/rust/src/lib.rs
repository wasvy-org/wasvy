// filename: src/lib.rs
use bevy_math::{Quat, Vec3};
use bevy_transform::components::Transform;
use serde::{Deserialize, Serialize};

use bindings::*;

struct GuestComponent;

impl Guest for GuestComponent {
    fn setup(app: App) {
        // Define an example system with commands that run on startup
        let start = System::new("start");
        start.add_commands();
        app.add_systems(&Schedule::ModStartup, &[&start]);

        // Define another system that runs every update
        let update = System::new("update");
        update.add_query(&[
            QueryFor::Mut("bevy_transform::components::transform::Transform".to_string()),
            QueryFor::Without("bevy_camera::camera::Camera".to_string()),
            QueryFor::Without("bevy_ecs::hierarchy::ChildOf".to_string()),
        ]);
        app.add_systems(&Schedule::Update, &[&update]);
    }

    fn start(commands: Commands) {
        println!("Mod {{ name }} startup");

        commands.spawn(&[(
            "bevy_ecs::name::Name".to_string(),
            r#"{ "name": "Example entity" }"#,
        )]);
    }

    fn spin_cube(query: Query) {
        while let Some(results) = query.iter() {
            // Get the first component
            let component = results.component(0);

            // Deserialize the first component
            let mut transform: Transform = from_json(&component.get());

            // Spin the entity
            transform.rotate(Quat::from_rotation_y(0.025));

            // Set the new component value
            component.set(&to_json(&transform));
        }
    }
}

export!(GuestComponent);

fn from_json<'a, T>(component: &'a str) -> T
where
    T: Deserialize<'a>,
{
    serde_json::from_str(component).expect("serializable component")
}

fn to_json<T>(component: &T) -> String
where
    T: Serialize,
{
    serde_json::to_string(&component).expect("serializable component")
}
