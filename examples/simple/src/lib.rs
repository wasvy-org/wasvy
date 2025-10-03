mod bindings {
    wit_bindgen::generate!({
        path: ["./wit"],
        world: "component:simple/example",
        with: {
            "wasvy:ecs/app": generate,
        }
    });
}
use bevy_math::{Quat, Vec3};
use bevy_transform::components::Transform;
use bindings::{
    wasvy::ecs::app::{App, Query, QueryFor, Schedule, System},
    *,
};
use serde::{Deserialize, Serialize};

struct GuestComponent;

impl Guest for GuestComponent {
    fn setup() {
        // Define a new system that queries for entities with a Transform and a Marker component
        let spin_cube = System::new("spin-cube");
        spin_cube.add_query(&[
            QueryFor::Mut("bevy_transform::components::transform::Transform".to_string()),
            QueryFor::With("host_example::MyMarker".to_string()),
        ]);

        // Define another example system with commands
        let my_system = System::new("my-system");
        my_system.add_commands();
        my_system.add_query(&[QueryFor::With("simple::MyStruct".to_string())]);

        // Register the systems to run in the Update schedule
        let app = App::new();
        app.add_systems(Schedule::Update, vec![my_system, spin_cube]);
    }

    fn spin_cube(query: Query) {
        while let Some(components) = query.iter() {
            // Get and deserialize the first component
            let mut transform: Transform = from_json(&components[0].get());

            // Spin the cube
            transform.rotate(Quat::from_rotation_y(0.025));

            // Set the new component value
            components[0].set(&to_json(&transform));
        }
    }

    fn my_system(commands: Commands, query: Query) {
        // Count how many entities we've spawned
        let mut count = 0;
        while let Some(_) = query.iter() {
            count += 1;
        }

        // Avoid spawning more than 10
        if count > 10 {
            return;
        }

        #[derive(Serialize, Deserialize)]
        struct MyStruct {
            value: i32,
        }

        println!("Spawning an entity with MyStruct component in my-system");

        let component_1 = MyStruct { value: 0 };
        let component_2 = Transform::default().looking_at(Vec3::ONE, Vec3::Y);

        commands.spawn(&[
            ("simple::MyStruct".to_string(), to_json(&component_1)),
            (
                "bevy_transform::components::transform::Transform".to_string(),
                to_json(&component_2),
            ),
        ]);
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
