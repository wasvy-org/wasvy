use bevy_asset::{AssetEvent, AssetId, AssetServer, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_log::warn;
use bevy_platform::collections::{HashMap, HashSet};

use super::{SetupParams, install::Installed};
use crate::{
    asset::{ModAsset, ModReplacement},
    mods::Mod,
    system::{ModSystem, ModSystemAsset, ModSystemName, ModSystemNamespace},
};

#[derive(Default)]
pub(crate) struct Publications {
    assets: HashMap<AssetId<ModAsset>, Revision>,
    sequence: Revision,
    pub(super) slots: HashMap<ModSystemName, SystemSlot>,
    pub(super) sources: HashMap<AssetId<ModAsset>, Source>,
}

impl Publications {
    pub(super) fn track_asset_loads<'a>(
        &mut self,
        events: impl Iterator<Item = &'a AssetEvent<ModAsset>>,
    ) {
        for event in events {
            match *event {
                AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                    self.assets.insert(id, self.sequence.next());
                }
                AssetEvent::Removed { id } => {
                    self.assets.remove(&id);
                }
                _ => {}
            }
        }
    }

    pub(super) fn remove_unused_sources(
        &mut self,
        mods: &Query<(Entity, &Mod, Option<&Installed>)>,
    ) -> HashSet<AssetId<ModAsset>> {
        let mut active: HashSet<_> = mods
            .iter()
            .flat_map(|(_, module, installed)| {
                std::iter::once(module.asset().id())
                    .chain(installed.and_then(|installed| installed.source))
            })
            .collect();
        self.slots.retain(|_, slot| active.contains(&slot.home));
        active.extend(self.slots.values().map(|slot| slot.owner));
        self.sources.retain(|id, _| active.contains(id));
        active
    }

    pub(super) fn publish_assets(
        &mut self,
        params: &mut SetupParams,
        active: &HashSet<AssetId<ModAsset>>,
    ) {
        let mut candidates: Vec<_> = params
            .assets
            .ids()
            .filter_map(|id| {
                let revision = *self.assets.get(&id)?;
                if !active.contains(&id)
                    || self
                        .sources
                        .get(&id)
                        .is_some_and(|source| source.revision == revision)
                {
                    return None;
                }
                Some((revision, id))
            })
            .collect();
        candidates.sort_unstable_by_key(|(revision, _)| *revision);
        for (revision, id) in candidates {
            self.publish_asset(params, id, revision);
        }
    }

    fn publish_asset(
        &mut self,
        params: &mut SetupParams,
        id: AssetId<ModAsset>,
        revision: Revision,
    ) {
        let asset = params.assets.get(id).unwrap();
        let namespace = asset.namespace().clone();
        if self
            .sources
            .iter()
            .any(|(other, source)| *other != id && source.namespace == namespace)
        {
            warn!("Mod namespace {namespace:?} is already owned by another asset; refusing {id:?}");
            return;
        }
        let source = Source {
            revision,
            namespace,
            policy: asset.settings().replacements.clone(),
        };
        let source_handle =
            retained_source_handle(&mut params.assets, params.server.as_deref(), id);
        let asset = params.assets.get(id).unwrap();
        for definition in asset.systems() {
            self.publish_system(
                &mut params.systems,
                id,
                &source,
                source_handle.as_ref(),
                definition.clone(),
            );
        }
        if asset.errors().is_empty() {
            for (name, slot) in &mut self.slots {
                if slot.owner == id && slot.publication < revision && source.may_replace(name) {
                    slot.system = None;
                }
            }
        }
        self.sources.insert(id, source);
    }

    fn publish_system(
        &mut self,
        systems: &mut Assets<ModSystemAsset>,
        id: AssetId<ModAsset>,
        source: &Source,
        source_handle: Option<&Handle<ModAsset>>,
        definition: ModSystem,
    ) {
        let existing = self.slots.get(&definition.name);
        if let Some(slot) = existing {
            if slot.publication > source.revision {
                return;
            }
            if !source.may_replace(&definition.name) {
                warn!(
                    "Mod {} may not replace {}",
                    source.namespace, definition.name
                );
                return;
            }
        }
        let name = definition.name.clone();
        let system = systems.add(ModSystemAsset {
            definition,
            source: source_handle.cloned(),
        });
        self.slots.insert(
            name,
            SystemSlot {
                home: existing.map_or(id, |slot| slot.home),
                owner: id,
                publication: source.revision,
                system: Some(system),
            },
        );
    }
}

/// Order in which asset loads were observed; later revisions win contested system slots.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Revision(u64);

impl Revision {
    fn next(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}

pub(super) struct Source {
    revision: Revision,
    namespace: ModSystemNamespace,
    policy: ModReplacement,
}

impl Source {
    fn may_replace(&self, name: &ModSystemName) -> bool {
        match &self.policy {
            ModReplacement::Any => true,
            ModReplacement::None => false,
            ModReplacement::Namespace => &self.namespace == name.namespace(),
            ModReplacement::Only(allowed) => allowed.contains(name),
        }
    }
}

pub(super) struct SystemSlot {
    pub(super) home: AssetId<ModAsset>,
    owner: AssetId<ModAsset>,
    publication: Revision,
    pub(super) system: Option<Handle<ModSystemAsset>>,
}

fn retained_source_handle(
    assets: &mut Assets<ModAsset>,
    server: Option<&AssetServer>,
    id: AssetId<ModAsset>,
) -> Option<Handle<ModAsset>> {
    if !assets.get(id)?.retain_source {
        return None;
    }
    // AssetServer's handle also retains the path metadata needed for file watching.
    server
        .and_then(|server| server.get_id_handle(id))
        .or_else(|| assets.get_strong_handle(id))
}
