use crate::system::ModSystem;

/// A reusable system asset. Its optional source keeps the loader asset alive.
/// The system also retains its own executable revision across source reloads.
#[derive(bevy_asset::Asset, bevy_reflect::TypePath)]
pub struct ModSystemAsset {
    pub definition: ModSystem,
    #[dependency]
    pub source: Option<bevy_asset::Handle<crate::asset::ModAsset>>,
}

impl ModSystemAsset {
    pub fn native(definition: ModSystem) -> Self {
        Self {
            definition,
            source: None,
        }
    }
}
