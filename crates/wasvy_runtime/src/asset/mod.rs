use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use bevy_asset::Asset;
use bevy_platform::collections::HashSet;
use bevy_reflect::{Reflect, TypePath};
use error_collection::Errors;
use serde::{Deserialize, Serialize};

use crate::system::{ModSystem, ModSystemName, ModSystemNamespace};

mod discovery;
pub use discovery::ModDiscovery;

/// Permission for a [ModAsset] to replace or delete previously published systems on load. New names are always allowed.
///
/// Belongs to the [ModAsset], so every [Mod](crate::mods::Mod) using it shares the same replacement. Each published
/// revision is checked against this setting. Changing it does not revoke prior replacements or deletions.
/// Narrowing the policy can leave old systems running after hot reload. For best results choose one per asset and stick with it.
#[derive(Clone, Debug, Default, Reflect, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModReplacement {
    /// A newly loaded mod may override any [ModSystem], overriding any existing behaviour in your game
    #[default]
    Any,

    /// A newly loaded mod may never override existing [ModSystem]s.
    ///
    /// Warning: this will break hot-reloading, since a mod will not be able to patch the systems it declared
    None,

    /// A newly loaded mod may only override [ModSystem]s in the same namespace as the mod itself
    Namespace,

    /// A newly loaded mod may only override specified [ModSystem]s.
    Only(HashSet<ModSystemName>),
}

/// [AssetLoader](bevy_asset::AssetLoader) settings for loaders producing [ModAsset]s.
///
/// Set them with [AssetServer::load_builder](bevy_asset::AssetServer::load_builder) or
/// [Mods::load_with_settings](crate::mods::Mods::load_with_settings). The AssetServer applies them again
/// when it reloads the asset. It ignores the settings of later loads of an already loaded path
/// (<https://github.com/bevyengine/bevy/issues/11111>), so the first load of a path decides its policy.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModLoaderSettings {
    /// See [ModReplacement]
    pub replacements: ModReplacement,
}

impl From<ModReplacement> for ModLoaderSettings {
    fn from(replacements: ModReplacement) -> Self {
        Self { replacements }
    }
}

/// An asset representing a loaded wasvy mod.
#[derive(Asset, TypePath)]
pub struct ModAsset {
    systems: Arc<[ModSystem]>,
    errors: Arc<Errors>,
    namespace: ModSystemNamespace,
    settings: ModLoaderSettings,
    #[cfg(test)]
    pub(crate) revision: u64,
    pub(crate) retain_source: bool,
}

impl ModAsset {
    /// Creates a mod asset after a completed "discovery phase" by a mod loader.
    ///
    /// You must first provide a unique namespace identifier for this mod.
    ///
    /// During mod discovery the asset loader creates:
    /// - [ModSystem]s
    /// - Optional errors that signify partial failures of mod setup
    ///
    /// The asset loader may also provide errors that happened during mod discovery.
    /// These usually signify partial/incomplete loading of systems.
    pub fn new(namespace: impl Into<ModSystemNamespace>, mut discovery: ModDiscovery) -> Self {
        let namespace = namespace.into();
        discovery.replace_missing_namespace(&namespace);
        discovery.retain_unique();
        #[cfg(test)]
        static REVISION: AtomicU64 = AtomicU64::new(1);
        Self {
            retain_source: false,
            #[cfg(test)]
            revision: REVISION.fetch_add(1, Ordering::Relaxed),
            namespace,
            settings: ModLoaderSettings::default(),
            systems: discovery.systems.into(),
            errors: discovery.errors.into(),
        }
    }

    /// Keeps the loader asset alive while its published systems are in use.
    pub fn with_source_retention(mut self) -> Self {
        self.retain_source = true;
        self
    }

    /// Applies [ModLoaderSettings] to the loaded mod asset.
    ///
    /// When manually replacing (not hot-reloading) the asset with a new one, remember to use the same settings again.
    /// See the docs for [ModLoaderSettings] and [ModReplacement].
    pub fn with_settings(mut self, settings: impl Into<ModLoaderSettings>) -> Self {
        self.settings = settings.into();
        self
    }

    /// See [ModLoaderSettings]
    pub fn settings(&self) -> &ModLoaderSettings {
        &self.settings
    }

    /// The reusable definitions prepared for this loaded revision.
    pub fn system(&self) -> &[ModSystem] {
        &self.systems
    }

    /// Discovery diagnostics. Missing systems are not deletions when this is nonempty.
    pub fn errors(&self) -> &Errors {
        &self.errors
    }

    pub fn systems(&self) -> &[ModSystem] {
        &self.systems
    }

    pub fn namespace(&self) -> &ModSystemNamespace {
        &self.namespace
    }
}
