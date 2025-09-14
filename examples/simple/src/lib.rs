mod bindings {
    wit_bindgen::generate!({
        path: ["./wit"],
        world: "component:simple/example",
        with: {
            "wasvy:ecs/app": generate,
        }
    });
}
use bindings::{
    wasvy::ecs::app::{App, Schedule, System},
    *,
};

struct GuestComponent;

impl Guest for GuestComponent {
    fn setup() {
        // Define a new system that queries for entities with a Transform and a Marker component
        let my_system = System::new("my-system");

        // Register the system to run in the Update schedule
        let app = App::new();
        app.add_systems(Schedule::Update, vec![my_system]);
    }

    fn my_system() -> () {
        println!("Running my-system");
    }
}

export!(GuestComponent);
