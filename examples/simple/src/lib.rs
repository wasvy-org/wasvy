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
    fn setup(app: App) {
        // Define an example system with commands that run on startup
        let spawn_entities = System::new("spawn-entities");
        spawn_entities.add_commands();
        app.add_systems(&Schedule::ModStartup, &[&spawn_entities]);

        // Define another new system that queries for entities with a Transform and a Marker component
        let spin_cube = System::new("spin-cube");
        spin_cube.add_query(&[
            QueryFor::Mut("bevy_transform::components::transform::Transform".to_string()),
            QueryFor::With("host_example::MyMarker".to_string()),
        ]);
        app.add_systems(&Schedule::Update, &[&spin_cube]);
    }

    fn spawn_entities(commands: Commands) {
        println!("Spawning an entity with MyStruct component");

        #[derive(Serialize, Deserialize)]
        struct MyStruct {
            value: i32,
        }

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

    fn spin_cube(query: Query) {
        while let Some(results) = query.iter() {
            // Get the first component
            let component = results.component(0);

            // Deserialize the first component
            let mut transform: Transform = from_json(&component.get());

            // Spin the cube
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
