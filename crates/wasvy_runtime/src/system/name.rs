use std::{any::type_name_of_val, borrow::Cow, fmt, hash::Hash};

use bevy_ecs::system::{IntoSystem, SystemInput};
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};

/// A system path, compared and hashed by its exact name.
#[derive(Clone, Debug, Reflect, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModSystemName {
    value: Cow<'static, str>,
    namespace: ModSystemNamespace,
}

impl ModSystemName {
    /// Construct a new system name and resolve its namespace.
    ///
    /// Examples:
    /// - "my_system" will resolve to the namespace for that mod
    /// - "my_game::my_system" will resolve with namespace "my_game"
    ///
    /// Both example names above are not equivalent.
    pub fn new(value: impl Into<Self>) -> Self {
        value.into()
    }

    /// Creates a name from the system's Rust type path.
    ///
    /// Panics if the type name is missing the crate name.
    pub fn from_system<In: SystemInput, Out, Marker>(
        system: impl IntoSystem<In, Out, Marker>,
    ) -> Self {
        // NOTE: We can't rely on system.name(), since DebugName can be disabled via a feature flag
        let path = type_name_of_val(&system);
        Self {
            value: path.into(),
            namespace: ModSystemNamespace::new(system),
        }
    }

    /// Returns the namespace for the system
    pub fn namespace(&self) -> &ModSystemNamespace {
        &self.namespace
    }

    pub(crate) fn replace_missing_namespace(&mut self, namespace: &ModSystemNamespace) {
        if self.namespace.as_str().is_none_or(str::is_empty) {
            self.namespace = namespace.clone();
        }
    }
}

impl From<Cow<'static, str>> for ModSystemName {
    fn from(value: Cow<'static, str>) -> Self {
        let namespace = match &value {
            Cow::Borrowed(path) => path.split_once("::").map(|(namespace, _)| namespace.into()),
            Cow::Owned(path) => path
                .split_once("::")
                .map(|(namespace, _)| namespace.to_owned().into()),
        }
        .unwrap_or_default();
        Self { value, namespace }
    }
}

impl From<&'static str> for ModSystemName {
    fn from(value: &'static str) -> Self {
        Cow::Borrowed(value).into()
    }
}

impl From<String> for ModSystemName {
    fn from(value: String) -> Self {
        Cow::<'static, str>::Owned(value).into()
    }
}

impl From<&String> for ModSystemName {
    fn from(value: &String) -> Self {
        value.clone().into()
    }
}

impl fmt::Display for ModSystemName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { value, namespace } = self;
        write!(f, "{namespace}/{value}")
    }
}

/// The unique crate/mod name
#[derive(Clone, Debug, Reflect, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct ModSystemNamespace {
    /// None means the system
    value: Option<Cow<'static, str>>,
}

impl ModSystemNamespace {
    /// Creates a namespace from the system's Rust type path.
    ///
    /// Panics if the type name is missing the crate name.
    pub fn new<In: SystemInput, Out, Marker>(system: impl IntoSystem<In, Out, Marker>) -> Self {
        // NOTE: We can't rely on system.name(), since DebugName can be disabled via a feature flag
        let path = type_name_of_val(&system);

        Self::raw(
            path.split_once("::")
                .expect("system type name includes crate name")
                .0,
        )
    }

    pub fn as_str(&self) -> Option<&str> {
        self.value.as_deref()
    }

    pub(crate) fn raw(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            value: Some(name.into()),
        }
    }
}

impl From<ModSystemName> for ModSystemNamespace {
    fn from(value: ModSystemName) -> Self {
        value.namespace
    }
}

impl From<&'static str> for ModSystemNamespace {
    fn from(value: &'static str) -> Self {
        Self::raw(value)
    }
}

impl From<String> for ModSystemNamespace {
    fn from(value: String) -> Self {
        Self::raw(value)
    }
}

impl fmt::Display for ModSystemNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str().unwrap_or(env!("CARGO_PKG_NAME")))
    }
}

#[cfg(test)]
mod tests {
    use bevy_platform::collections::HashSet;

    use super::*;

    #[test]
    fn names() {
        let long = ModSystemName::new("game::movement::integrate");
        let short_owned = ModSystemName::new("game::integrate".to_string());
        assert_ne!(long, short_owned);
        assert_eq!(HashSet::from([long.clone(), short_owned]).len(), 2);
        let owned = ModSystemName::new("game::movement::integrate".to_string());
        assert_eq!(long, owned);
        assert_eq!(HashSet::from([long.clone(), owned]).len(), 1);
    }

    #[test]
    fn namespaces() {
        let namespace: ModSystemNamespace = ModSystemName::new("random").into();
        assert_eq!(namespace, Default::default());
        let namespace: ModSystemNamespace = ModSystemName::new("game::system").into();
        assert_eq!(namespace, "game".into());
        assert_eq!(ModSystemNamespace::raw("game"), "game".into());
        assert_ne!(ModSystemNamespace::raw("game"), "other_game".into());
    }

    #[test]
    fn conflicts_include_resolved_namespaces() {
        let mut game = ModSystemName::new("tick");
        game.replace_missing_namespace(&"game".into());
        let mut other = ModSystemName::new("tick");
        other.replace_missing_namespace(&"other".into());
        assert_ne!(&game, &other);
        assert_ne!(&game, &"game::tick".into());
        assert_eq!(&game, &game.clone());
    }

    fn bevy_system() {}

    #[test]
    fn system() {
        let system = ModSystemName::from_system(bevy_system);
        assert_eq!(system.value, concat!(module_path!(), "::bevy_system"));
        assert_eq!(system.namespace.value.unwrap(), "wasvy_runtime");
    }

    #[test]
    fn system_no_alloc() {
        let system = ModSystemName::from_system(bevy_system);
        assert!(matches!(system.value, Cow::Borrowed(_)));
        assert!(matches!(system.namespace.value, Some(Cow::Borrowed(_))));
    }
}
