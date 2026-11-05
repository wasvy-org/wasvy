use anyhow::anyhow;
use bevy_platform::collections::HashSet;
use error_collection::Errors;

use crate::system::{ModSystem, ModSystemNamespace};

/// Definitions discovered in a loaded mod revision, including partial successes.
#[derive(Default)]
pub struct ModDiscovery {
    pub systems: Vec<ModSystem>,
    pub errors: Errors,
}

impl ModDiscovery {
    pub(crate) fn replace_missing_namespace(&mut self, namespace: &ModSystemNamespace) {
        for system in &mut self.systems {
            system.name.replace_missing_namespace(namespace);
            for names in [&mut system.before, &mut system.after] {
                *names = std::mem::take(names)
                    .into_iter()
                    .map(|mut name| {
                        name.replace_missing_namespace(namespace);
                        name
                    })
                    .collect();
            }
        }
    }

    /// Must run after [Self::replace_missing_namespace]
    pub(crate) fn retain_unique(&mut self) {
        let mut duplicates = HashSet::new();
        for (index, system) in self.systems.iter().enumerate() {
            for other in &self.systems[..index] {
                if system.name == other.name {
                    self.errors.push(anyhow!(
                        "Conflicting mod system names: {} and {}",
                        other.name,
                        system.name
                    ));
                    duplicates.insert(other.name.clone());
                    duplicates.insert(system.name.clone());
                }
            }
        }
        self.systems
            .retain(|definition| !duplicates.contains(&definition.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::ModSystemName;

    fn system(name: &'static str) -> ModSystem {
        let mut system = ModSystem::new(|| {});
        system.name = name.into();
        system
    }

    #[test]
    fn replace_missing_namespaces_in_names_and_ordering() {
        let names = ["tick", "::empty", "other::tick"];
        let mut discovery = ModDiscovery {
            systems: names.map(system).into(),
            ..Default::default()
        };
        for system in &mut discovery.systems {
            system.before = names.map(ModSystemName::new).into();
            system.after = system.before.clone();
        }

        discovery.replace_missing_namespace(&"game".into());
        discovery.replace_missing_namespace(&"another".into());

        for (system, expected_namespace) in discovery.systems.iter().zip(["game", "game", "other"])
        {
            assert_eq!(system.name.namespace().as_str(), Some(expected_namespace));
            assert_eq!(system.before.len(), names.len());
            assert_eq!(system.after.len(), names.len());
            for target in &discovery.systems {
                assert!(system.before.contains(&target.name));
                assert!(system.after.contains(&target.name));
            }
        }
    }

    #[test]
    fn remove_duplicates() {
        let mut discovery = ModDiscovery {
            systems: vec![
                system("game::one::first"),
                system("game::first"),
                system("other::two::second"),
                system("other::second"),
                system("identical"),
                system("identical"),
                system("unique"),
            ],
            ..Default::default()
        };
        discovery.retain_unique();

        assert_eq!(discovery.errors.len(), 1);
        assert!(discovery.errors[0].to_string().contains("identical"));
        assert_eq!(discovery.systems.len(), 5);
    }

    #[test]
    fn distinct_siblings() {
        let mut discovery = ModDiscovery {
            systems: vec![system("game::one::tick"), system("game::two::tick")],
            ..Default::default()
        };
        discovery.retain_unique();
        assert!(discovery.errors.is_empty());
        assert_eq!(discovery.systems.len(), 2);
    }

    #[test]
    fn short_paths_are_distinct_identities() {
        for alias_first in [false, true] {
            let mut discovery = ModDiscovery {
                systems: vec![
                    system("game::one::tick"),
                    system("game::two::tick"),
                    system("game::tick"),
                ],
                ..Default::default()
            };
            if alias_first {
                discovery.systems.reverse();
            }
            discovery.retain_unique();
            assert!(discovery.errors.is_empty());
            assert_eq!(discovery.systems.len(), 3);
        }
    }
}
