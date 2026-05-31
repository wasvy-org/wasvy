//! PROTOTYPE ONLY.
//! Question: does the Wasvy Modules runtime model feel right when driven by hand?
//! In particular: workspace inventory vs world composition, first-load init once per world,
//! preserve-state reload on success, and blocked reload keeping the old generation active.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct InventoryEntry {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct SharedWorldState {
    pub player_health: i32,
    pub balance_seed: i32,
}

#[derive(Debug, Clone)]
pub struct PrivateModuleState {
    pub cooldown_ticks: i32,
    pub init_token: u32,
}

#[derive(Debug, Clone)]
pub enum ReloadStatus {
    Active,
    Pending,
    BlockedRegistration,
    BlockedCompatibility,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub active_generation: Option<u64>,
    pub pending_generation: Option<u64>,
    pub first_load_runs_in_world: u32,
    pub reload_status: ReloadStatus,
    pub last_error: Option<String>,
    pub incompatible_changes: Vec<String>,
    pub private_state: PrivateModuleState,
}

#[derive(Debug, Clone)]
pub struct WorldState {
    pub epoch: u32,
    pub composition: BTreeSet<String>,
    pub shared: SharedWorldState,
    pub loaded_modules: BTreeMap<String, LoadedModule>,
}

#[derive(Debug, Clone)]
pub struct PrototypeState {
    pub next_generation: u64,
    pub inventory: BTreeMap<String, InventoryEntry>,
    pub world: WorldState,
    pub last_event: String,
}

impl PrototypeState {
    pub fn new() -> Self {
        Self {
            next_generation: 1,
            inventory: BTreeMap::new(),
            world: WorldState {
                epoch: 1,
                composition: BTreeSet::new(),
                shared: SharedWorldState {
                    player_health: 100,
                    balance_seed: 1,
                },
                loaded_modules: BTreeMap::new(),
            },
            last_event: "Prototype started. Seed the workspace inventory first.".to_string(),
        }
    }

    pub fn seed_inventory(&mut self) {
        self.inventory.insert(
            "combat".to_string(),
            InventoryEntry {
                path: "crates/modules/combat".to_string(),
            },
        );
        self.inventory.insert(
            "ai".to_string(),
            InventoryEntry {
                path: "crates/modules/ai".to_string(),
            },
        );
        self.last_event = "Seeded workspace inventory with combat and ai.".to_string();
    }

    pub fn toggle_composition(&mut self, module: &str) {
        if !self.inventory.contains_key(module) {
            self.last_event =
                format!("Cannot compose {module}: not present in workspace inventory.");
            return;
        }

        if self.world.composition.remove(module) {
            self.world.loaded_modules.remove(module);
            self.last_event = format!(
                "Removed {module} from world composition and unloaded it from the current world."
            );
        } else {
            self.world.composition.insert(module.to_string());
            self.last_event = format!("Added {module} to world composition.");
        }
    }

    pub fn boot_world_composition(&mut self) {
        if self.world.composition.is_empty() {
            self.last_event = "World composition is empty. Nothing booted.".to_string();
            return;
        }

        let modules: Vec<String> = self.world.composition.iter().cloned().collect();
        let mut booted = Vec::new();
        let mut already_active = Vec::new();

        for module in modules {
            if self.world.loaded_modules.contains_key(&module) {
                already_active.push(module);
                continue;
            }

            let generation = self.next_generation();
            let init_token = self.world.epoch;
            self.world.loaded_modules.insert(
                module.clone(),
                LoadedModule {
                    active_generation: Some(generation),
                    pending_generation: None,
                    first_load_runs_in_world: 1,
                    reload_status: ReloadStatus::Active,
                    last_error: None,
                    incompatible_changes: Vec::new(),
                    private_state: PrivateModuleState {
                        cooldown_ticks: 0,
                        init_token,
                    },
                },
            );
            booted.push(module);
        }

        self.last_event = format!(
            "Booted modules: {}. Already active: {}.",
            display_list(&booted),
            display_list(&already_active)
        );
    }

    pub fn mutate_shared_health(&mut self, delta: i32) {
        self.world.shared.player_health += delta;
        self.last_event = format!(
            "Mutated shared world state: player_health {}=>{}.",
            self.world.shared.player_health - delta,
            self.world.shared.player_health
        );
    }

    pub fn mutate_private_cooldown(&mut self, module: &str, delta: i32) {
        let Some(loaded) = self.world.loaded_modules.get_mut(module) else {
            self.last_event =
                format!("Cannot mutate {module} private state: module is not active.");
            return;
        };

        let before = loaded.private_state.cooldown_ticks;
        loaded.private_state.cooldown_ticks += delta;
        self.last_event = format!(
            "Mutated {module} module-private state: cooldown_ticks {before}=>{}.",
            loaded.private_state.cooldown_ticks
        );
    }

    pub fn reload_success(&mut self, module: &str) {
        let next_generation = self.next_generation();
        let Some(loaded) = self.world.loaded_modules.get_mut(module) else {
            self.last_event =
                format!("Cannot reload {module}: module is not active in this world.");
            return;
        };

        let old_generation = loaded.active_generation;
        loaded.pending_generation = Some(next_generation);
        loaded.reload_status = ReloadStatus::Pending;

        // Transaction commits successfully: preserve state, swap code generation.
        loaded.active_generation = Some(next_generation);
        loaded.pending_generation = None;
        loaded.reload_status = ReloadStatus::Active;
        loaded.last_error = None;
        loaded.incompatible_changes.clear();

        self.last_event = format!(
            "Reloaded {module} successfully. Preserved world state. Generation {:?}=>{:?}.",
            old_generation, loaded.active_generation
        );
    }

    pub fn reload_registration_failure(&mut self, module: &str, error: &str) {
        let attempted_generation = self.next_generation();
        let Some(loaded) = self.world.loaded_modules.get_mut(module) else {
            self.last_event =
                format!("Cannot reload {module}: module is not active in this world.");
            return;
        };

        let old_generation = loaded.active_generation;
        loaded.pending_generation = Some(attempted_generation);
        loaded.reload_status = ReloadStatus::Pending;

        loaded.pending_generation = None;
        loaded.reload_status = ReloadStatus::BlockedRegistration;
        loaded.last_error = Some(error.to_string());
        loaded.incompatible_changes.clear();

        self.last_event = format!(
            "Reload for {module} blocked by registration failure. Old generation {:?} remains active. Error: {error}",
            old_generation
        );
    }

    pub fn reload_compatibility_failure(&mut self, module: &str, changes: &[&str]) {
        let attempted_generation = self.next_generation();
        let Some(loaded) = self.world.loaded_modules.get_mut(module) else {
            self.last_event =
                format!("Cannot reload {module}: module is not active in this world.");
            return;
        };

        let old_generation = loaded.active_generation;
        loaded.pending_generation = Some(attempted_generation);
        loaded.reload_status = ReloadStatus::Pending;

        loaded.pending_generation = None;
        loaded.reload_status = ReloadStatus::BlockedCompatibility;
        loaded.last_error =
            Some("Reload compatibility failure: relaunch required to run latest code.".to_string());
        loaded.incompatible_changes = changes.iter().map(|s| s.to_string()).collect();

        self.last_event = format!(
            "Reload for {module} blocked by compatibility failure. Old generation {:?} remains active. Relaunch required.",
            old_generation
        );
    }

    pub fn restart_world(&mut self) {
        self.world.epoch += 1;
        self.world.shared = SharedWorldState {
            player_health: 100,
            balance_seed: self.world.epoch as i32,
        };
        self.world.loaded_modules.clear();

        let composed: Vec<String> = self.world.composition.iter().cloned().collect();
        let mut booted = Vec::new();
        for module in composed {
            let generation = self.next_generation();
            self.world.loaded_modules.insert(
                module.clone(),
                LoadedModule {
                    active_generation: Some(generation),
                    pending_generation: None,
                    first_load_runs_in_world: 1,
                    reload_status: ReloadStatus::Active,
                    last_error: None,
                    incompatible_changes: Vec::new(),
                    private_state: PrivateModuleState {
                        cooldown_ticks: 0,
                        init_token: self.world.epoch,
                    },
                },
            );
            booted.push(module);
        }

        self.last_event = format!(
            "Restarted world. New epoch {}. First-load init reran for: {}.",
            self.world.epoch,
            display_list(&booted)
        );
    }

    fn next_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        generation
    }
}

fn display_list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}
