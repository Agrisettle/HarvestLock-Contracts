//! #220 — what publishing an event actually costs, per `#[contractevent]`
//! `data_format`.
//!
//! `docs/gas-costs.md` records the numbers; this test is the reproducible
//! source of them and a regression guard on the one choice the contract
//! makes explicitly (`StatusChanged` / `FxQuoteStaleChanged` at
//! `single-value`). It measures both shapes that matter:
//!
//! - a **zero-data-field** event (the `StatusChanged` shape): the three
//!   formats differ only in the empty `data` payload they carry;
//! - a **three-data-field** event (the `DeliveryConfirmed` shape): here the
//!   map's field-name keys are the cost, and vec is materially smaller.
//!
//! Run with `cargo test --test event_cost -- --nocapture` to print the table.
//! The numbers are deterministic on the in-process test host; they are CPU
//! instructions and *event* bytes, not a live-network fee (see
//! `docs/gas-costs.md` for why the two must not be conflated).

#![allow(dead_code)]

use harvestlock_escrow::{Status, StatusChanged};
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{contract, contractevent, contractimpl, contracttype, Env};

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S {
    A,
    B,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapNoData {
    #[topic]
    pub from: S,
    #[topic]
    pub to: S,
}

#[contractevent(data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecNoData {
    #[topic]
    pub from: S,
    #[topic]
    pub to: S,
}

#[contractevent(data_format = "single-value")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SingleNoData {
    #[topic]
    pub from: S,
    #[topic]
    pub to: S,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapThreeData {
    #[topic]
    pub grade_index: u32,
    pub delivered_quantity: u32,
    pub settlement_bps: u32,
    pub reviewed_by: u32,
}

#[contractevent(data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecThreeData {
    #[topic]
    pub grade_index: u32,
    pub delivered_quantity: u32,
    pub settlement_bps: u32,
    pub reviewed_by: u32,
}

#[contract]
pub struct EventCostProbe;

#[contractimpl]
impl EventCostProbe {
    pub fn map_no_data(env: Env) {
        MapNoData {
            from: S::A,
            to: S::B,
        }
        .publish(&env);
    }
    pub fn vec_no_data(env: Env) {
        VecNoData {
            from: S::A,
            to: S::B,
        }
        .publish(&env);
    }
    pub fn single_no_data(env: Env) {
        SingleNoData {
            from: S::A,
            to: S::B,
        }
        .publish(&env);
    }
    pub fn map_three_data(env: Env) {
        MapThreeData {
            grade_index: 0,
            delivered_quantity: 1_000,
            settlement_bps: 10_000,
            reviewed_by: 0,
        }
        .publish(&env);
    }
    pub fn vec_three_data(env: Env) {
        VecThreeData {
            grade_index: 0,
            delivered_quantity: 1_000,
            settlement_bps: 10_000,
            reviewed_by: 0,
        }
        .publish(&env);
    }
}

struct Measurement {
    cpu: u64,
    memory: u64,
    event_bytes: u32,
}

fn measure(env: &Env, call: impl FnOnce()) -> Measurement {
    // Start from a clean meter so registration/setup cannot leak in. The
    // budget also resets automatically before each top-level invocation;
    // this makes the intent explicit.
    env.cost_estimate().budget().reset_default();
    call();
    let budget = env.cost_estimate().budget();
    let resources = env.cost_estimate().resources();
    Measurement {
        cpu: budget.cpu_instruction_cost(),
        memory: budget.memory_bytes_cost(),
        event_bytes: resources.contract_events_size_bytes,
    }
}

#[test]
fn data_format_costs_match_the_documented_ranking() {
    let env = Env::default();
    let id = env.register(EventCostProbe, ());
    let probe = EventCostProbeClient::new(&env, &id);

    let map_no = measure(&env, || probe.map_no_data());
    let vec_no = measure(&env, || probe.vec_no_data());
    let single_no = measure(&env, || probe.single_no_data());

    let map_three = measure(&env, || probe.map_three_data());
    let vec_three = measure(&env, || probe.vec_three_data());

    std::println!(
        "no-data  : map {} bytes/{} cpu/{} mem, vec {} bytes/{} cpu/{} mem, single {} bytes/{} cpu/{} mem",
        map_no.event_bytes,
        map_no.cpu,
        map_no.memory,
        vec_no.event_bytes,
        vec_no.cpu,
        vec_no.memory,
        single_no.event_bytes,
        single_no.cpu,
        single_no.memory
    );
    std::println!(
        "3-data   : map {} bytes/{} cpu/{} mem, vec {} bytes/{} cpu/{} mem",
        map_three.event_bytes,
        map_three.cpu,
        map_three.memory,
        vec_three.event_bytes,
        vec_three.cpu,
        vec_three.memory
    );

    // Zero data fields: single-value carries no empty container, so it is
    // strictly smallest and never larger than the other two.
    assert!(
        single_no.event_bytes < map_no.event_bytes,
        "single-value should be smaller than map for a no-data event"
    );
    assert!(
        single_no.event_bytes <= vec_no.event_bytes,
        "single-value should not be larger than vec for a no-data event"
    );

    // Three data fields: vec is positional and sheds the per-field name keys,
    // so it is materially smaller than map. This is why multi-field events
    // stay on map only deliberately (robustness), never by byte count.
    assert!(
        vec_three.event_bytes < map_three.event_bytes,
        "vec should be smaller than map for a multi-data event"
    );

    // The real StatusChanged (now single-value) emits from the test contract
    // with the chosen format, proving it round-trips through the SDK. Its
    // absolute byte count differs a little from the probe because the symbol
    // is `StatusChanged` rather than `single_no_data`, and topic metadata is
    // part of the event bytes — so the assertion is the ranking, not equality.
    let real = measure(&env, || {
        StatusChanged {
            from: Status::Draft,
            to: Status::Locked,
        }
        .publish(&env);
    });
    std::println!(
        "StatusChanged (shipped, single-value): {} bytes/{} cpu/{} mem",
        real.event_bytes,
        real.cpu,
        real.memory
    );
    let events = env.events().all();
    assert!(!events.events().is_empty());
    assert!(
        real.event_bytes < map_no.event_bytes,
        "the shipped StatusChanged should be smaller than a map-format no-data event"
    );
}
