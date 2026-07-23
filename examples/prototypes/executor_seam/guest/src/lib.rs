mod bindings {
    wit_bindgen::generate!({
        path: "../wit",
        world: "prototype",
    });
}

use bindings::{Actors, Counter};

struct Component;

impl bindings::Guest for Component {
    fn tick(counter: Counter, actors: Actors) {
        counter.set(counter.get() + 10);
        while let Some(actor) = actors.next() {
            actor.set(actor.get() + 10);
        }
    }
}

bindings::export!(Component with_types_in bindings);
