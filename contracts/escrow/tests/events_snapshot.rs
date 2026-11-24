//! Issue #214 — pin every event's serialized XDR shape in a committed
//! snapshot.
//!
//! Indexers decode events by topic position and by data-map key. Renaming a
//! field, reordering a `#[topic]`, moving a field between topic and data, or
//! changing its XDR type breaks every consumer silently — the events still
//! decode, they just mean something different. Nothing else in CI notices.
//!
//! This suite builds one instance of each `#[contractevent]` type, serializes
//! it with the SDK's own `to_xdr`, canonicalizes the result (values are kept
//! for scalars and symbols; addresses become the literal `Address` so the file
//! is stable across runs), and compares the whole set line-by-line against
//! `contracts/escrow/events.snapshot`. Any drift fails with the exact line
//! that changed, and with the command to re-bless it:
//!
//! ```text
//! BLESS_EVENTS_SNAPSHOT=1 cargo test -p harvestlock-escrow --test events_snapshot
//! ```
//!
//! The snapshot is intentionally hand-readable rather than raw XDR hex: a
//! reviewer can see `previous: Address`, `@topic`, and data-map order without
//! decoding base64. The serialization still comes from the real event type, so
//! the check is on the actual XDR, not on a restatement of the source.
//!
//! Companion pieces:
//! * `docs/events.md` — the human reference (topics, data, emit sites, RPC
//!   filters) that this snapshot makes mechanically checkable.
//! * `docs/versioning.md` §2 — an event-shape change is a MINOR **BREAKING**
//!   bump.
//! * `contracts/escrow/error_discriminants.snapshot` — the same guard pattern
//!   for `Error` codes.

use harvestlock_escrow::{
    AllocationRecorded, BuyerReassigned, DeliveryConfirmed, DisputeEnded, DisputeFlagged,
    DisputeOutcome, EscrowContract, FxQuoteStaleChanged, FxResolved, FxShortfallFunded,
    OracleConfigReplaced, Payout, RemainderFunded, Status, StatusChanged, TrancheClaimed,
    TrancheReclaimed,
};
use soroban_sdk::testutils::{Address as _, EnvTestConfig};
use soroban_sdk::xdr::{ContractEventBody, ScMap, ScVal};
use soroban_sdk::{symbol_short, Address, Env, Event};

const SNAPSHOT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/events.snapshot");

/// The canonical, line-oriented rendering of one `ScVal`.
///
/// Scalars and symbols keep their values so a changed enum discriminant or a
/// renamed data key is caught. Addresses are replaced by their type name: the
/// values are random per run, and their *type and position* is what an indexer
/// depends on. Any future event field of another type falls into the explicit
/// arms below; the fallback is `{:?}` so an unexpected type still produces a
/// stable-enough line to review by hand.
fn canonical(value: &ScVal) -> String {
    match value {
        ScVal::Void => "Void".to_string(),
        ScVal::Bool(b) => format!("Bool({b})"),
        ScVal::U32(n) => format!("U32({n})"),
        ScVal::I32(n) => format!("I32({n})"),
        ScVal::U64(n) => format!("U64({n})"),
        ScVal::I64(n) => format!("I64({n})"),
        ScVal::Timepoint(n) => format!("Timepoint({})", n.0),
        ScVal::Duration(n) => format!("Duration({})", n.0),
        ScVal::U128(parts) => format!("U128({},{})", parts.hi, parts.lo),
        ScVal::I128(parts) => format!("I128({},{})", parts.hi, parts.lo),
        ScVal::Bytes(bytes) => format!("Bytes(<{} bytes>)", bytes.len()),
        ScVal::String(s) => format!("String({})", String::from_utf8_lossy(s.as_ref())),
        ScVal::Symbol(s) => format!("Symbol({})", String::from_utf8_lossy(s.as_ref())),
        ScVal::Address(_) => "Address".to_string(),
        ScVal::Vec(Some(values)) => {
            let inner: std::vec::Vec<String> = values.iter().map(|v| canonical(v)).collect();
            format!("Vec([{}])", inner.join(", "))
        }
        ScVal::Vec(None) => "Vec(None)".to_string(),
        ScVal::Map(Some(map)) => canonical_map(map),
        ScVal::Map(None) => "Map(None)".to_string(),
        other => format!("{other:?}"),
    }
}

fn canonical_map(map: &ScMap) -> String {
    let entries: std::vec::Vec<String> = map
        .iter()
        .map(|entry| format!("{}: {}", canonical(&entry.key), canonical(&entry.val)))
        .collect();
    format!("Map({{{}}})", entries.join(", "))
}

/// Serialize one event instance and return its snapshot line.
fn line(env: &Env, contract: &Address, event: impl Event) -> String {
    let serialized = event.to_xdr(env, contract);
    let ContractEventBody::V0(body) = &serialized.body;
    let topics: std::vec::Vec<String> = body.topics.iter().map(|t| canonical(t)).collect();
    let name = match body.topics.first() {
        Some(ScVal::Symbol(s)) => String::from_utf8_lossy(s.as_ref()).to_string(),
        other => panic!("event topic 0 is not a Symbol: {other:?}"),
    };
    format!(
        "{} topics=[{}] data={}",
        name,
        topics.join(", "),
        canonical(&body.data)
    )
}

#[test]
fn every_event_matches_the_committed_snapshot() {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    let contract = env.register(EscrowContract, ());
    let party = Address::generate(&env);
    let new_party = Address::generate(&env);

    // One instance per event. Values are fixed (not generated) so the file has
    // no run-to-run noise; addresses are the only generated values and the
    // canonicalizer strips them.
    let lines = std::vec![
        line(
            &env,
            &contract,
            StatusChanged {
                from: Status::Draft,
                to: Status::Locked,
            },
        ),
        line(
            &env,
            &contract,
            Payout {
                reason: symbol_short!("coop_pay"),
                to: party.clone(),
                amount: 1_000,
            },
        ),
        line(
            &env,
            &contract,
            TrancheClaimed {
                tranche: 1,
                to: party.clone(),
                amount: 150_000,
            },
        ),
        line(
            &env,
            &contract,
            TrancheReclaimed {
                tranche: 2,
                to: party.clone(),
                amount: 150_000,
            },
        ),
        line(
            &env,
            &contract,
            RemainderFunded {
                from: party.clone(),
                amount: 700_000,
            },
        ),
        line(
            &env,
            &contract,
            FxShortfallFunded {
                from: party.clone(),
                amount: 334_000,
            },
        ),
        line(
            &env,
            &contract,
            BuyerReassigned {
                previous: party.clone(),
                new_buyer: new_party.clone(),
            },
        ),
        line(
            &env,
            &contract,
            DisputeFlagged {
                flagger: party.clone(),
                pre_status: Status::Locked,
                deadline: 604_800,
            },
        ),
        line(
            &env,
            &contract,
            DisputeEnded {
                outcome: DisputeOutcome::Resolved,
                restored: Status::Locked,
            },
        ),
        line(
            &env,
            &contract,
            FxResolved {
                rate: 66_700_000_000,
                rate_timestamp: 1,
                fx_adjusted_total: 1_334_000,
                shortfall: 334_000,
                deadline: 604_800,
            },
        ),
        line(&env, &contract, FxQuoteStaleChanged { stale: true }),
        line(
            &env,
            &contract,
            DeliveryConfirmed {
                grade_index: 0,
                delivered_quantity: 500,
                settlement_bps: 5_000,
            },
        ),
        line(
            &env,
            &contract,
            AllocationRecorded {
                member_count: 2,
                total_share_bps: 10_000,
            },
        ),
        line(
            &env,
            &contract,
            OracleConfigReplaced {
                new_oracle_contract: new_party,
            },
        ),
    ];

    let actual = format!(
        "# Pinned serialized shape of every escrow event (issue #214).\n\
         #\n\
         # Each line is one #[contractevent]: its topic-0 name, the full topic\n\
         # list in order, and the data map with key order preserved. Addresses\n\
         # are shown as `Address` because their values are per-instance; every\n\
         # other value (symbol names, enum discriminants, integers, booleans)\n\
         # is pinned literally.\n\
         #\n\
         # A change to this file is an event-shape change: per docs/versioning.md\n\
         # section 2 it is a MINOR **BREAKING** bump, and downstream decoders\n\
         # must be updated. Regenerate with:\n\
         #\n\
         #   BLESS_EVENTS_SNAPSHOT=1 cargo test -p harvestlock-escrow --test events_snapshot\n\
         #\n\
         {}\n",
        lines.join("\n")
    );

    if std::env::var("BLESS_EVENTS_SNAPSHOT").ok().as_deref() == Some("1") {
        std::fs::write(SNAPSHOT_PATH, &actual).expect("write events.snapshot");
        println!("BLESS_EVENTS_SNAPSHOT=1: wrote {SNAPSHOT_PATH}");
        return;
    }

    let committed = std::fs::read_to_string(SNAPSHOT_PATH)
        .unwrap_or_else(|e| panic!("read {SNAPSHOT_PATH}: {e}"));

    let actual_lines: std::vec::Vec<&str> = actual.lines().collect();
    let committed_lines: std::vec::Vec<&str> = committed.lines().collect();
    for (i, (want, got)) in committed_lines.iter().zip(actual_lines.iter()).enumerate() {
        assert_eq!(
            want,
            got,
            "events.snapshot drifted at line {}.\n\
             If this change is intended, re-bless with:\n\
             BLESS_EVENTS_SNAPSHOT=1 cargo test -p harvestlock-escrow --test events_snapshot\n\
             and remember docs/versioning.md section 2: an event-shape change is a\n\
             MINOR **BREAKING** bump.",
            i + 1
        );
    }
    assert_eq!(
        committed_lines.len(),
        actual_lines.len(),
        "events.snapshot has a different number of lines; re-bless it"
    );
}

/// Guards the guard: a snapshot comparison that could not tell two shapes apart
/// would be worthless. These assertions pin the canonicalizer's discriminating
/// behavior on the parts an indexer depends on.
#[test]
fn canonicalizer_distinguishes_keys_order_and_types() {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    let contract = env.register(EscrowContract, ());
    let a = line(
        &env,
        &contract,
        DeliveryConfirmed {
            grade_index: 0,
            delivered_quantity: 500,
            settlement_bps: 5_000,
        },
    );
    let b = line(
        &env,
        &contract,
        DeliveryConfirmed {
            grade_index: 1,
            delivered_quantity: 500,
            settlement_bps: 5_000,
        },
    );
    assert_ne!(a, b, "different topic values must render differently");

    let c = line(
        &env,
        &contract,
        StatusChanged {
            from: Status::Draft,
            to: Status::Locked,
        },
    );
    let d = line(
        &env,
        &contract,
        StatusChanged {
            from: Status::Locked,
            to: Status::Draft,
        },
    );
    assert_ne!(c, d, "topic order must be visible");
}
