mod bindings {
    wit_bindgen::generate!({
        path: ["./wit"],
        world: "component:simple/example",
        with: {
            "wasvy:ecs/app": generate,
        }
    });
}
use bevy_math::Vec3;
use bevy_transform::components::Transform;
use bindings::{
    wasvy::ecs::app::{App, Schedule, System},
    *,
};
use serde::{Deserialize, Serialize};

struct GuestComponent;

impl Guest for GuestComponent {
    fn setup() {
        // Define a new system that queries for entities with a Transform and a Marker component
        let my_system = System::new("my-system");
        my_system.add_commands();

        // Register the system to run in the Update schedule
        let app = App::new();
        app.add_systems(Schedule::Update, vec![my_system]);
    }

    fn my_system(commands: Commands) -> () {
        println!("Running my-system");

        #[derive(Serialize, Deserialize)]
        struct MyStruct {
            value: i32,
        }

        let component_1 = MyStruct { value: 123 };
        let component_2 = Transform::IDENTITY.looking_at(Vec3::ONE, Vec3::Y);

        let component_1_json = serde_json::to_string(&component_1).expect("serializable component");
        let component_2_json = serde_json::to_string(&component_2).expect("serializable component");

        commands.spawn(&[
            ("simple::MyStruct".to_string(), component_1_json),
            (
                "bevy_transform::components::transform::Transform".to_string(),
                component_2_json,
            ),
        ]);
    }
}

export!(GuestComponent);
