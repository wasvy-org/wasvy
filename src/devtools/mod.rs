use std::borrow::Cow;

use bevy_ecs::resource::Resource;
use serde::Serialize;

#[cfg(feature = "devtools")]
mod mods;

#[cfg(feature = "devtools")]
mod plugin;

pub struct DevtoolsPlugin(pub Devtools);

#[cfg(not(feature = "devtools"))]
impl bevy_app::Plugin for DevtoolsPlugin {
    fn build(&self, _: &mut bevy_app::App) {
        use bevy_log::prelude::*;
        error!("you must enable wasvy's \"devtools\" feature in your Cargo.toml");
    }
}

/// A config for the DevtoolsPlugin.
///
/// See `ModLoaderPlugin::devtools` for examples of how to create one.
#[derive(Resource, Debug, Clone, Serialize)]
pub struct Devtools {
    /// The name of your app or game, defaults to "Bevy App powered by Wasvy"
    pub program_name: String,

    /// This is a list of the application's supported wit interfaces
    ///
    /// Hint: Use your interface directly via `vec![include_str!("../wit/my-interface.wit").into()]`,
    pub interfaces: Vec<Cow<'static, str>>,
}

impl Default for Devtools {
    fn default() -> Self {
        Self {
            program_name: "Bevy App powered by Wasvy".into(),
            interfaces: vec![
                include_str!("./../../wit/bevy-ecs.wit").into(),
                include_str!("./../../wit/wasvy-ecs.wit").into(),
            ],
        }
    }
}

impl Devtools {
    /// Creates a default config with a custom name
    pub fn new(program_name: impl Into<String>) -> Self {
        Self {
            program_name: program_name.into(),
            ..Default::default()
        }
    }

    /// Adds to the interfaces implemented by our game
    pub fn implement(mut self, interface: impl Into<Cow<'static, str>>) -> Self {
        self.interfaces.push(interface.into());

        self
    }
}

impl From<&'static str> for Devtools {
    fn from(value: &'static str) -> Self {
        Devtools {
            program_name: value.into(),
            ..Default::default()
        }
    }
}

impl From<String> for Devtools {
    fn from(value: String) -> Self {
        Devtools {
            program_name: value,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default() {
        let _ = Devtools::default();
    }
}
