use std::{
    borrow::Cow,
    fmt,
    hash::{Hash, Hasher},
};

use bevy_ecs::utils::DebugName;

/// A system path. Short qualified names can match module-qualified paths
/// through [`Self::conflicts_with`], separately from exact equality and hashing.
#[derive(Clone, Debug)]
pub struct ModSystemName(Cow<'static, str>);

impl ModSystemName {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Matches exact paths or a crate-qualified short alias of a full path.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self == other
            || (self.identity() == other.identity()
                && self.identity().0.is_some()
                && (self.0.matches("::").count() == 1 || other.0.matches("::").count() == 1))
    }

    fn identity(&self) -> (Option<&str>, &str) {
        match self.0.split_once("::") {
            Some((crate_name, path)) => (Some(crate_name), path.rsplit("::").next().unwrap()),
            None => (None, &self.0),
        }
    }
}

impl From<&'static str> for ModSystemName {
    fn from(name: &'static str) -> Self {
        Self::new(name)
    }
}

impl From<String> for ModSystemName {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<DebugName> for ModSystemName {
    fn from(value: DebugName) -> Self {
        value.to_string().into()
    }
}

impl PartialEq for ModSystemName {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for ModSystemName {}

impl Hash for ModSystemName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Display for ModSystemName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use bevy_platform::collections::HashSet;

    use super::*;

    #[test]
    fn exact() {
        let long: ModSystemName = "game::movement::integrate".into();
        let short_owned: ModSystemName = "game::integrate".to_string().into();
        assert_ne!(long, short_owned);
        assert_eq!(HashSet::from([long.clone(), short_owned]).len(), 2);
        let owned: ModSystemName = "game::movement::integrate".to_string().into();
        assert_eq!(long, owned);
        assert_eq!(HashSet::from([long.clone(), owned]).len(), 1);
        assert_eq!(long.as_str(), "game::movement::integrate");
        assert_ne!(long, "other_game::integrate".into());
        assert_ne!(long, "integrate".into());
    }

    #[test]
    fn name() {
        for (left, right, expected) in [
            (
                "game::movement::integrate",
                "game::movement::integrate",
                true,
            ),
            ("game::movement::integrate", "game::integrate", true),
            ("game::movement::integrate", "game::other::integrate", false),
            ("game::movement::integrate", "other_game::integrate", false),
            ("game::movement::integrate", "integrate", false),
            ("integrate", "integrate", true),
        ] {
            let left = ModSystemName::from(left);
            let right = ModSystemName::from(right);
            assert_eq!(left.conflicts_with(&right), expected, "{left} vs {right}");
            assert_eq!(right.conflicts_with(&left), expected, "{right} vs {left}");
        }
    }
}
