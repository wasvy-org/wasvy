use bevy_ecs::resource::Resource;
use serde::Serialize;

#[cfg(feature = "devtools")]
mod mods;

#[cfg(feature = "devtools")]
mod plugin;

pub struct DevtoolsPlugin(pub Config);

#[cfg(not(feature = "devtools"))]
impl bevy_app::Plugin for DevtoolsPlugin {
    fn build(&self, _: &mut bevy_app::App) {
        use bevy_log::prelude::*;
        error!("you must enable wasvy's \"devtools\" feature in your Cargo.toml");
    }
}

/// A config for the DevtoolsPlugin.
///
/// See `ModloaderPlugin::devtools` for examples of how to create one.
#[derive(Resource, Debug, Clone, Serialize)]
pub struct Config {
    /// The name of your app or game, defaults to "Bevy App powered by Wasvy"
    name: &'static str,

    /// This is a list of your application's wasi bindings compiled into the application.
    ///
    /// Give it all the wit bindings you implement via the `include_str` macro.
    interfaces: &'static [&'static str],
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: "Bevy App powered by Wasvy",
            interfaces: &[
                include_str!("./../../wit/bevy.wit"),
                include_str!("./../../wit/wasvy.wit"),
            ],
        }
    }
}

impl From<&'static str> for Config {
    fn from(name: &'static str) -> Self {
        Config {
            name,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default() {
        let _ = Config::default();
    }
}
