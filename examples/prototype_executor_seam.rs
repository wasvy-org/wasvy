//! PROTOTYPE — throwaway interactive executor-seam experiment for GitHub issue #81.
//!
//! Question: can one stable, Wasvy-owned typed Bevy system invoke native code
//! directly, invoke a compatible WASM Artifact through a bridge, and atomically
//! switch between Generations without replacing the Bevy executor?

#[path = "prototypes/executor_seam/logic.rs"]
mod logic;

use std::{
    io::{self, Write},
    ptr,
    sync::atomic::{AtomicPtr, AtomicU64, Ordering},
    time::Instant,
};

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use logic::{
    ACTIVE_PLAN, ArtifactKind, INVOCATION_CHANGED_PLAN, PrototypeState, SCHEDULING_CHANGED_PLAN,
};
use serde::{Deserialize, Serialize};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
struct Counter {
    ticks: u64,
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
struct Actor {
    energy: i64,
}

struct ActiveRuntime {
    generation: u64,
    kind: ArtifactKind,
}

/// A single atomically published pointer contains both Generation and Kind.
/// Old values are intentionally leaked: this is a throwaway prototype that
/// models retirement as "still safe for an in-flight executor to hold".
#[derive(Resource)]
struct ExecutionSlot {
    active: AtomicPtr<ActiveRuntime>,
}

impl ExecutionSlot {
    fn new(generation: u64, kind: ArtifactKind) -> Self {
        Self {
            active: AtomicPtr::new(Box::into_raw(Box::new(ActiveRuntime { generation, kind }))),
        }
    }

    fn load(&self) -> &ActiveRuntime {
        let value = self.active.load(Ordering::Acquire);
        assert_ne!(value, ptr::null_mut());
        // SAFETY: published values are immutable and intentionally remain alive.
        unsafe { &*value }
    }

    fn publish(&self, generation: u64, kind: ArtifactKind) {
        let next = Box::into_raw(Box::new(ActiveRuntime { generation, kind }));
        let _retiring = self.active.swap(next, Ordering::AcqRel);
    }
}

#[derive(Resource, Default)]
struct Telemetry {
    executor_runs: AtomicU64,
    native_calls: AtomicU64,
    wasm_calls: AtomicU64,
    serialized_values: AtomicU64,
}

#[derive(Clone, Copy)]
struct TelemetrySnapshot {
    executor_runs: u64,
    native_calls: u64,
    wasm_calls: u64,
    serialized_values: u64,
}

impl Telemetry {
    fn snapshot(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            executor_runs: self.executor_runs.load(Ordering::Relaxed),
            native_calls: self.native_calls.load(Ordering::Relaxed),
            wasm_calls: self.wasm_calls.load(Ordering::Relaxed),
            serialized_values: self.serialized_values.load(Ordering::Relaxed),
        }
    }
}

/// The sole Bevy schedule node. It always owns the real typed Bevy parameters.
fn wasvy_executor(
    slot: Res<ExecutionSlot>,
    telemetry: Res<Telemetry>,
    mut counter: ResMut<Counter>,
    mut actors: Query<&mut Actor>,
) {
    telemetry.executor_runs.fetch_add(1, Ordering::Relaxed);
    let active = slot.load();
    std::hint::black_box(active.generation);

    match active.kind {
        ArtifactKind::Native => {
            telemetry.native_calls.fetch_add(1, Ordering::Relaxed);
            native_export(&mut counter, &mut actors);
        }
        ArtifactKind::Wasm => {
            telemetry.wasm_calls.fetch_add(1, Ordering::Relaxed);
            wasm_bridge(&mut counter, &mut actors, &telemetry);
        }
    }
}

/// Native path: direct access to real typed Bevy parameters. No reflection,
/// serialization, allocation, hash lookup, or dynamic query reconstruction.
fn native_export(counter: &mut Counter, actors: &mut Query<&mut Actor>) {
    counter.ticks += 1;
    for mut actor in actors.iter_mut() {
        actor.energy += 1;
    }
}

/// Simulates the WASM transport while deliberately reusing the same typed
/// executor and Bevy access declaration. Different behavior makes swaps visible.
fn wasm_bridge(counter: &mut Counter, actors: &mut Query<&mut Actor>, telemetry: &Telemetry) {
    let bytes = serde_json::to_vec(counter).unwrap();
    let mut guest_counter: Counter = serde_json::from_slice(&bytes).unwrap();
    telemetry.serialized_values.fetch_add(1, Ordering::Relaxed);
    guest_counter.ticks += 10;
    *counter = guest_counter;

    for mut actor in actors.iter_mut() {
        let bytes = serde_json::to_vec(&*actor).unwrap();
        let mut guest_actor: Actor = serde_json::from_slice(&bytes).unwrap();
        telemetry.serialized_values.fetch_add(1, Ordering::Relaxed);
        guest_actor.energy += 10;
        *actor = guest_actor;
    }
}

fn main() {
    let mut app = App::new();
    app.insert_resource(Counter { ticks: 0 })
        .insert_resource(ExecutionSlot::new(1, ArtifactKind::Native))
        .init_resource::<Telemetry>()
        .add_systems(Update, wasvy_executor);
    app.world_mut().spawn(Actor { energy: 0 });

    let mut state = PrototypeState::default();
    let mut benchmark = "not run".to_string();

    loop {
        render(&mut app, &state, &benchmark);
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).unwrap() == 0 {
            break;
        }

        match input.trim() {
            "t" | "" => app.update(),
            "n" => publish(&mut app, &mut state, ArtifactKind::Native),
            "w" => publish(&mut app, &mut state, ArtifactKind::Wasm),
            "i" => state.inspect(
                INVOCATION_CHANGED_PLAN,
                "inspected a candidate with a changed Invocation Shape",
            ),
            "s" => state.inspect(
                SCHEDULING_CHANGED_PLAN,
                "inspected a candidate with changed scheduling",
            ),
            "b" => benchmark = run_benchmark(&mut app),
            "q" => break,
            _ => state.last_action = "unknown command",
        }
    }
}

fn publish(app: &mut App, state: &mut PrototypeState, kind: ArtifactKind) {
    state.publish_dispatch_compatible(kind);
    app.world()
        .resource::<ExecutionSlot>()
        .publish(state.generation, kind);
}

fn run_benchmark(app: &mut App) -> String {
    const RUNS: usize = 100_000;
    let start = Instant::now();
    for _ in 0..RUNS {
        app.update();
    }
    format!("{RUNS} complete App::update calls in {:?}", start.elapsed())
}

fn render(app: &mut App, state: &PrototypeState, benchmark: &str) {
    let (generation, artifact_kind) = {
        let active = app.world().resource::<ExecutionSlot>().load();
        (active.generation, active.kind)
    };
    let counter = app.world().resource::<Counter>().ticks;
    let telemetry = app.world().resource::<Telemetry>().snapshot();
    let actor_energy = {
        let world = app.world_mut();
        let mut query = world.query::<&Actor>();
        query.single(world).unwrap().energy
    };

    print!("\x1b[2J\x1b[H");
    println!("{BOLD}PROTOTYPE — Wasvy executor seam{RESET}");
    println!("{DIM}One stable typed Bevy executor; atomically swappable dispatch target.{RESET}\n");

    println!("{BOLD}Active Module Instance{RESET}");
    println!("  module ID:             {}", ACTIVE_PLAN.module_id);
    println!("  Generation:            {generation}");
    println!("  Artifact Kind:         {artifact_kind:?}");
    println!("  executor installations:{}", state.executor_installations);
    println!("  system-set fingerprint:0x{:x}", ACTIVE_PLAN.system_set);
    println!("  invocation fingerprint:0x{:x}", ACTIVE_PLAN.invocation);
    println!("  scheduling fingerprint:0x{:x}\n", ACTIVE_PLAN.scheduling);

    println!("{BOLD}World state{RESET}");
    println!("  Counter.ticks:         {counter}");
    println!("  Actor.energy:          {actor_energy}\n");

    println!("{BOLD}Executor telemetry{RESET}");
    println!("  executor runs:         {}", telemetry.executor_runs);
    println!("  direct Native calls:   {}", telemetry.native_calls);
    println!("  bridged Wasm calls:    {}", telemetry.wasm_calls);
    println!("  serialized values:     {}", telemetry.serialized_values);
    println!("  benchmark:             {benchmark}\n");

    println!("{BOLD}Last action{RESET}");
    println!("  {}", state.last_action);
    println!("  assessment: {:?}\n", state.last_assessment);

    println!("{BOLD}Actions{RESET}");
    println!("  {BOLD}[t]{RESET} {DIM}tick once{RESET}");
    println!("  {BOLD}[n]{RESET} {DIM}atomically publish compatible Native Generation{RESET}");
    println!("  {BOLD}[w]{RESET} {DIM}atomically publish compatible Wasm Generation{RESET}");
    println!("  {BOLD}[i]{RESET} {DIM}inspect changed Invocation Shape{RESET}");
    println!("  {BOLD}[s]{RESET} {DIM}inspect changed scheduling{RESET}");
    println!("  {BOLD}[b]{RESET} {DIM}run rough current-backend benchmark{RESET}");
    println!("  {BOLD}[q]{RESET} {DIM}quit{RESET}");
}
