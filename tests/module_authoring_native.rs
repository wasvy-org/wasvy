use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

mod combat_module {
    use super::*;
    use bevy_app::Update;

    #[derive(Resource, Reflect, Default, Debug, PartialEq)]
    #[reflect(Resource)]
    pub struct Counter(pub u32);

    wasvy::module! {
        name: "combat"
    }

    #[wasvy::on_first_load]
    fn init(mut commands: Commands) {
        commands.insert_resource(Counter(1));
    }

    #[wasvy::system(Update)]
    fn tick(mut counter: ResMut<Counter>) {
        counter.0 += 1;
    }
}

mod isolated_module {
    use super::*;

    #[derive(Resource, Reflect, Default, Debug, PartialEq)]
    #[reflect(Resource)]
    pub struct Flag(pub bool);

    wasvy::module! {
        name: "isolated"
    }

    #[wasvy::on_first_load]
    fn init(mut commands: Commands) {
        commands.insert_resource(Flag(true));
    }
}

#[test]
fn native_adapter_runs_first_load_once_and_update_systems() {
    let mut app = App::new();
    app.register_type::<combat_module::Counter>();
    app.add_plugins(combat_module::NativeAdapterPlugin);

    app.update();
    assert_eq!(app.world().resource::<combat_module::Counter>().0, 2);

    app.update();
    assert_eq!(app.world().resource::<combat_module::Counter>().0, 3);
}

#[test]
fn native_adapter_is_scoped_to_its_declared_module() {
    let mut app = App::new();
    app.register_type::<combat_module::Counter>();
    app.register_type::<isolated_module::Flag>();
    app.add_plugins(combat_module::NativeAdapterPlugin);

    app.update();

    assert_eq!(app.world().resource::<combat_module::Counter>().0, 2);
    assert!(
        app.world()
            .get_resource::<isolated_module::Flag>()
            .is_none()
    );
}
