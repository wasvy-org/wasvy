//! PROTOTYPE ONLY.
//! Question: does the Wasvy Modules runtime model feel right when driven by hand?
//!
//! This prototype is intentionally throwaway. It explores:
//! - Workspace Inventory vs World Composition
//! - Module activation in one shared world
//! - First-load Initialization once per world
//! - successful preserve-state reload
//! - blocked reload keeping the old generation active

#[path = "prototypes/wasvy-modules/logic.rs"]
mod logic;

use std::io::{self, Write};

use logic::{LoadedModule, PrototypeState, ReloadStatus};

const CLEAR: &str = "\x1b[2J\x1b[H";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn main() {
    let mut state = PrototypeState::new();

    loop {
        render(&state);

        print!("{}command>{} ", BOLD, RESET);
        io::stdout().flush().expect("flush stdout");

        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("read command");
        let command = input.trim();

        match command {
            "s" => state.seed_inventory(),
            "c" => state.toggle_composition("combat"),
            "a" => state.toggle_composition("ai"),
            "b" => state.boot_world_composition(),
            "h" => state.mutate_shared_health(10),
            "k" => state.mutate_private_cooldown("combat", 1),
            "j" => state.mutate_private_cooldown("ai", 1),
            "r" => state.reload_success("combat"),
            "g" => state.reload_success("ai"),
            "f" => state.reload_registration_failure(
                "combat",
                "generated registration metadata did not match guest exports",
            ),
            "x" => state.reload_compatibility_failure(
                "combat",
                &[
                    "combat::CombatCooldown: added field source: AttackId",
                    "combat::ComboWindow: removed field legacy_end",
                ],
            ),
            "w" => state.restart_world(),
            "q" => break,
            "" => state.last_event = "No-op. Enter a shortcut or q to quit.".to_string(),
            other => {
                state.last_event =
                    format!("Unknown command `{other}`. See the shortcuts section below the state.")
            }
        }
    }
}

fn render(state: &PrototypeState) {
    print!("{CLEAR}");

    println!("{BOLD}WASVY MODULES RUNTIME PROTOTYPE{RESET}");
    println!(
        "{DIM}Question: does the proposed runtime model feel coherent before we implement it?{RESET}"
    );
    println!();

    println!("{BOLD}workspace inventory{RESET}");
    if state.inventory.is_empty() {
        println!("  {DIM}<empty>{RESET}");
    } else {
        for (id, entry) in &state.inventory {
            println!("  - {BOLD}{id}{RESET} {DIM}=> {}{RESET}", entry.path);
        }
    }
    println!();

    println!("{BOLD}world composition{RESET}");
    if state.world.composition.is_empty() {
        println!("  {DIM}<empty>{RESET}");
    } else {
        for id in &state.world.composition {
            println!("  - {BOLD}{id}{RESET}");
        }
    }
    println!();

    println!("{BOLD}world state{RESET}");
    println!("  {BOLD}epoch:{RESET} {}", state.world.epoch);
    println!(
        "  {BOLD}shared.player_health:{RESET} {}",
        state.world.shared.player_health
    );
    println!(
        "  {BOLD}shared.balance_seed:{RESET} {} {DIM}(simulates a shared resource){RESET}",
        state.world.shared.balance_seed
    );
    println!();

    println!("{BOLD}loaded modules{RESET}");
    if state.world.loaded_modules.is_empty() {
        println!("  {DIM}<none active in this world>{RESET}");
    } else {
        for (id, module) in &state.world.loaded_modules {
            render_loaded_module(id, module);
        }
    }
    println!();

    println!("{BOLD}last event{RESET}");
    println!("  {}", state.last_event);
    println!();

    println!("{BOLD}shortcuts{RESET}");
    println!("  {BOLD}s{RESET} {DIM}seed sample workspace inventory (combat + ai){RESET}");
    println!("  {BOLD}c{RESET} {DIM}toggle combat in world composition{RESET}");
    println!("  {BOLD}a{RESET} {DIM}toggle ai in world composition{RESET}");
    println!("  {BOLD}b{RESET} {DIM}boot current world composition{RESET}");
    println!("  {BOLD}h{RESET} {DIM}mutate shared world state: player_health +10{RESET}");
    println!("  {BOLD}k{RESET} {DIM}mutate combat module-private state: cooldown +1{RESET}");
    println!("  {BOLD}j{RESET} {DIM}mutate ai module-private state: cooldown +1{RESET}");
    println!(
        "  {BOLD}r{RESET} {DIM}successful reload of combat (preserve state, swap generation){RESET}"
    );
    println!("  {BOLD}g{RESET} {DIM}successful reload of ai{RESET}");
    println!("  {BOLD}f{RESET} {DIM}combat reload blocked by registration failure{RESET}");
    println!("  {BOLD}x{RESET} {DIM}combat reload blocked by compatibility failure{RESET}");
    println!(
        "  {BOLD}w{RESET} {DIM}restart world (first-load init reruns for composed modules){RESET}"
    );
    println!("  {BOLD}q{RESET} {DIM}quit{RESET}");
}

fn render_loaded_module(id: &str, module: &LoadedModule) {
    println!("  - {BOLD}{id}{RESET}");
    println!(
        "      {BOLD}active_generation:{RESET} {:?}",
        module.active_generation
    );
    println!(
        "      {BOLD}pending_generation:{RESET} {:?}",
        module.pending_generation
    );
    println!(
        "      {BOLD}reload_status:{RESET} {}",
        reload_status_label(&module.reload_status)
    );
    println!(
        "      {BOLD}first_load_runs_in_world:{RESET} {}",
        module.first_load_runs_in_world
    );
    println!(
        "      {BOLD}private.cooldown_ticks:{RESET} {}",
        module.private_state.cooldown_ticks
    );
    println!(
        "      {BOLD}private.init_token:{RESET} {} {DIM}(shows first-load init reran after world restart){RESET}",
        module.private_state.init_token
    );

    match &module.last_error {
        Some(error) => println!("      {BOLD}last_error:{RESET} {error}"),
        None => println!("      {BOLD}last_error:{RESET} {DIM}<none>{RESET}"),
    }

    if module.incompatible_changes.is_empty() {
        println!("      {BOLD}incompatible_changes:{RESET} {DIM}<none>{RESET}");
    } else {
        println!("      {BOLD}incompatible_changes:{RESET}");
        for change in &module.incompatible_changes {
            println!("        - {change}");
        }
    }
}

fn reload_status_label(status: &ReloadStatus) -> &'static str {
    match status {
        ReloadStatus::Active => "Active",
        ReloadStatus::Pending => "Pending",
        ReloadStatus::BlockedRegistration => "BlockedRegistration",
        ReloadStatus::BlockedCompatibility => "BlockedCompatibility",
    }
}
