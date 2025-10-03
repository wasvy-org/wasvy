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
    wasvy::ecs::app::{App, Query, QueryFor, Schedule, System},
    *,
};
use serde::{Deserialize, Serialize};

struct GuestComponent;

impl Guest for GuestComponent {
    fn setup() {
        // Define a new system that queries for entities with a Transform and a Marker component
        let my_system = System::new("my-system");
        my_system.add_commands();
        my_system.add_query(&[
            QueryFor::Mut("simple::MyStruct".to_string()),
            QueryFor::Ref("bevy_transform::components::transform::Transform".to_string()),
        ]);

        // Register the system to run in the Update schedule
        let app = App::new();
        app.add_systems(Schedule::Update, vec![my_system]);
    }

    fn my_system(commands: Commands, query: Query) -> () {
        println!("Running my-system");

        let mut count = 0;
        while let Some(components) = query.iter() {
            count += 1;

            let mut my_struct: MyStruct =
                serde_json::from_str(&components[0].get()).expect("serializable component");

            my_struct.value += 1;

            let string = serde_json::to_string(&my_struct).expect("serializable component");
            components[0].set(&string);

            if count == 1 {
                println!("Update my_struct {}", string);
                println!("Read transform {}", components[1].get());
            }
        }
        println!("query entity count {count}");

        #[derive(Serialize, Deserialize)]
        struct MyStruct {
            value: i32,
        }

        if count < 10 {
            let component_1 = MyStruct { value: 0 };
            let component_2 = Transform::IDENTITY.looking_at(Vec3::ONE, Vec3::Y);

            let component_1_json =
                serde_json::to_string(&component_1).expect("serializable component");
            let component_2_json =
                serde_json::to_string(&component_2).expect("serializable component");

            commands.spawn(&[
                ("simple::MyStruct".to_string(), component_1_json),
                (
                    "bevy_transform::components::transform::Transform".to_string(),
                    component_2_json,
                ),
            ]);
        }
    }
}

export!(GuestComponent);
