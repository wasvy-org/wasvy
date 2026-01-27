mod bindings {
    wit_bindgen::generate!({
        path: ["./wit"],
        world: "component:guest-wit/example",
        with: {
            "wasvy:ecs/app": generate,
            "game:components/components": generate,
        }
    });
}

use bindings::game::components::components::Health;
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};
use bindings::*;

wasvy_macros::guest_type_paths! {
    path = "wit",
    package = "game:components",
    interface = "components",
    module = bindings::game::components::components,
}

struct GuestComponent;

impl Guest for GuestComponent {
    fn setup(app: App) {
        let heal_system = System::new("heal-system");
        heal_system.add_query(&[QueryFor::Mut(Health::type_path())]);
        println!("Type path: {:?}", Health::type_path());
        app.add_systems(&Schedule::Update, &[&heal_system]);

        let pct_system = System::new("pct-system");
        pct_system.add_query(&[QueryFor::Ref(Health::type_path())]);
        app.add_systems(&Schedule::Update, &[&pct_system]);
    }

    fn heal_system(query: Query) {
        while let Some(result) = query.iter() {
            let component = result.component(0);
            let health = Health::new(component);
            health.heal(1.0);
        }
    }

    fn pct_system(query: Query) {
        while let Some(result) = query.iter() {
            let component = result.component(0);
            let health = Health::new(component);
            let pct = health.pct();
            println!("Health pct: {pct}");
        }
    }
}

export!(GuestComponent);
