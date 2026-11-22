//! Issue #216 — Prove the event stream is enough to rebuild a commitment's
//! state without reading storage.
//!
//! Because `env.events().all()` returns events from the **last invocation
//! only**, each test accumulates events into a `Vec` after every call.
//! The reducer folds that accumulated log into an `IndexerView` and asserts
//! that every event-derivable field matches the value returned by
//! `get_commitment()`.
//!
//! Fields that cannot be derived from events alone are catalogued at the
//! bottom of this file.

#![allow(dead_code)]

#[path = "common/mod.rs"]
mod common;

use common::{world, World, CONTRACTED_QUANTITY};
use harvestlock_escrow::{
    AllocationMember, AllocationRecorded, BuyerReassigned, DeliveryConfirmed, Status,
    StatusChanged, TrancheClaimed, TrancheReclaimed,
};
use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env, Event as _};
use soroban_sdk::xdr;

// ---------------------------------------------------------------------------
// EventLog — accumulated across invocations
// ---------------------------------------------------------------------------

struct EventLog(std::vec::Vec<xdr::ContractEvent>);

impl EventLog {
    fn new() -> Self { Self(std::vec::Vec::new()) }

    /// Drain events from the last invocation into the log.
    fn collect(&mut self, env: &Env, contract: &Address) {
        let filtered = env.events().all().filter_by_contract(contract);
        self.0.extend_from_slice(filtered.events());
    }
}

// ---------------------------------------------------------------------------
// IndexerView — what a pure-event indexer can know about a commitment
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct IndexerView {
    status: Option<Status>,
    advance1_claimed: bool,
    advance1_expired: bool,
    advance2_claimed: bool,
    advance2_expired: bool,
    remainder_funded: bool,
    delivered_quantity: Option<u32>,
    grade_index: Option<u32>,
    settlement_bps: Option<u32>,
    fx_resolved: bool,
    fx_shortfall_funded: bool,
    buyer_reassigned: bool,
    member_count: Option<u32>,
    total_share_bps: Option<u32>,
}

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

fn fold_log(log: &EventLog, env: &Env, contract: &Address) -> IndexerView {
    use soroban_sdk::xdr::{ContractEventBody, ScVal};
    let mut view = IndexerView::default();

    for event in &log.0 {
        let ContractEventBody::V0(body) = &event.body else { continue };
        let Some(ScVal::Symbol(sym)) = body.topics.first() else { continue };
        let sym_bytes: &[u8] = sym.as_ref();

        match sym_bytes {
            b"StatusChanged" => {
                if let Some(to_topic) = body.topics.get(2) {
                    for &s in all_statuses() {
                        let probe = StatusChanged { from: Status::Draft, to: s }
                            .to_xdr(env, contract);
                        if let ContractEventBody::V0(pb) = &probe.body {
                            if pb.topics.get(2) == Some(to_topic) {
                                view.status = Some(s);
                            }
                        }
                    }
                }
            }
            b"TrancheClaimed" => {
                if let Some(ScVal::U32(tr)) = body.topics.get(1) {
                    if *tr == 1 { view.advance1_claimed = true; }
                    else { view.advance2_claimed = true; }
                }
            }
            b"TrancheReclaimed" => {
                if let Some(ScVal::U32(tr)) = body.topics.get(1) {
                    if *tr == 1 { view.advance1_expired = true; }
                    else { view.advance2_expired = true; }
                }
            }
            b"RemainderFunded" => {
                view.remainder_funded = true;
            }
            b"DeliveryConfirmed" => {
                if let Some(ScVal::U32(gi)) = body.topics.get(1) {
                    view.grade_index = Some(*gi);
                }
                if let ScVal::Map(Some(map)) = &body.data {
                    for entry in map.iter() {
                        if let ScVal::Symbol(k) = &entry.key {
                            let kb: &[u8] = k.as_ref();
                            match kb {
                                b"delivered_quantity" => {
                                    if let ScVal::U32(v) = &entry.val { view.delivered_quantity = Some(*v); }
                                }
                                b"settlement_bps" => {
                                    if let ScVal::U32(v) = &entry.val { view.settlement_bps = Some(*v); }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            b"FxResolved" => { view.fx_resolved = true; }
            b"FxShortfallFunded" => { view.fx_shortfall_funded = true; }
            b"BuyerReassigned" => { view.buyer_reassigned = true; }
            b"AllocationRecorded" => {
                if let ScVal::Map(Some(map)) = &body.data {
                    for entry in map.iter() {
                        if let ScVal::Symbol(k) = &entry.key {
                            let kb: &[u8] = k.as_ref();
                            match kb {
                                b"member_count" => {
                                    if let ScVal::U32(v) = &entry.val { view.member_count = Some(*v); }
                                }
                                b"total_share_bps" => {
                                    if let ScVal::U32(v) = &entry.val { view.total_share_bps = Some(*v); }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    view
}

fn all_statuses() -> &'static [Status] {
    &[
        Status::Draft,
        Status::Locked,
        Status::Advance1Released,
        Status::CheckpointPassed,
        Status::Advance2Released,
        Status::ReadyForDelivery,
        Status::Delivered,
        Status::Settled,
        Status::Defaulted,
        Status::Forfeited,
        Status::Cancelled,
        Status::Disputed,
    ]
}

/// Check if a specific event XDR is in the log.
fn log_has_event(log: &EventLog, expected: &xdr::ContractEvent) -> bool {
    log.0.iter().any(|e| e == expected)
}

// ---------------------------------------------------------------------------
// Path 1: Draft → Locked → Advance1Released → advance1 claimed
// ---------------------------------------------------------------------------

#[test]
fn path_lock_and_claim_advance1() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();

    w.contract.lock();
    log.collect(&w.env, &w.contract.address);
    w.contract.release_advance_1();
    log.collect(&w.env, &w.contract.address);
    w.contract.claim_advance_1();
    log.collect(&w.env, &w.contract.address);

    let view = fold_log(&log, &w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(c.status), "status mismatch");
    assert_eq!(view.advance1_claimed, c.advance1_claimed, "advance1_claimed");
    assert_eq!(view.advance1_expired, c.advance1_expired, "advance1_expired");
}

// ---------------------------------------------------------------------------
// Path 2: Through to ReadyForDelivery with remainder funded
// ---------------------------------------------------------------------------

fn drive_to_remainder_funded_logged(w: &World, log: &mut EventLog) {
    w.contract.lock();
    log.collect(&w.env, &w.contract.address);
    w.contract.release_advance_1();
    log.collect(&w.env, &w.contract.address);
    w.contract.claim_advance_1();
    log.collect(&w.env, &w.contract.address);
    w.contract.mark_checkpoint();
    log.collect(&w.env, &w.contract.address);
    w.contract.release_advance_2();
    log.collect(&w.env, &w.contract.address);
    w.contract.claim_advance_2();
    log.collect(&w.env, &w.contract.address);
    w.contract.ready_for_delivery();
    log.collect(&w.env, &w.contract.address);
    w.contract.fund_remainder();
    log.collect(&w.env, &w.contract.address);
}

#[test]
fn path_remainder_funded() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();
    drive_to_remainder_funded_logged(&w, &mut log);

    let view = fold_log(&log, &w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(c.status), "status mismatch");
    assert_eq!(view.advance1_claimed, c.advance1_claimed, "advance1_claimed");
    assert_eq!(view.advance2_claimed, c.advance2_claimed, "advance2_claimed");
    assert!(view.remainder_funded, "remainder_funded not set in view");
    assert_eq!(view.remainder_funded, c.remainder_funded, "remainder_funded mismatch");
}

// ---------------------------------------------------------------------------
// Path 3: Through to Delivered — confirm_delivery fields
// ---------------------------------------------------------------------------

#[test]
fn path_delivered_full_quantity() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();
    drive_to_remainder_funded_logged(&w, &mut log);
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    log.collect(&w.env, &w.contract.address);

    let view = fold_log(&log, &w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(Status::Delivered), "expected Delivered");
    assert_eq!(view.status, Some(c.status), "status mismatch");
    assert_eq!(view.delivered_quantity, Some(c.delivered_quantity), "delivered_quantity");
    assert_eq!(view.grade_index, Some(c.grade_index), "grade_index");
    assert_eq!(view.settlement_bps, Some(c.settlement_bps), "settlement_bps");
}

// ---------------------------------------------------------------------------
// Path 4: Tranche expiry (reclaim path)
// ---------------------------------------------------------------------------

#[test]
fn path_tranche1_expired_reclaimed() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();

    w.contract.lock();
    log.collect(&w.env, &w.contract.address);
    w.contract.release_advance_1();
    log.collect(&w.env, &w.contract.address);
    let deadline = w.contract.get_commitment().advance1_deadline;
    w.set_time(deadline + 1);
    w.contract.reclaim_advance_1();
    log.collect(&w.env, &w.contract.address);

    let view = fold_log(&log, &w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert!(view.advance1_expired, "advance1_expired should be true");
    assert!(!view.advance1_claimed, "advance1_claimed should be false");
    assert_eq!(view.advance1_expired, c.advance1_expired, "advance1_expired mismatch");
}

// ---------------------------------------------------------------------------
// Path 5: Buyer reassignment — BuyerReassigned event in log
// ---------------------------------------------------------------------------

#[test]
fn path_buyer_reassigned() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();
    let new_buyer = Address::generate(&w.env);
    let old_buyer = w.buyer.clone();

    w.contract.reassign_buyer(&new_buyer);
    log.collect(&w.env, &w.contract.address);

    let expected = BuyerReassigned {
        previous: old_buyer,
        new_buyer: new_buyer.clone(),
    }
    .to_xdr(&w.env, &w.contract.address);

    assert!(log_has_event(&log, &expected), "BuyerReassigned event not found");

    let c = w.contract.get_commitment();
    assert_eq!(c.buyer, new_buyer);
}

// ---------------------------------------------------------------------------
// Path 6: Allocation recorded — AllocationRecorded fields
// ---------------------------------------------------------------------------

#[test]
fn path_allocation_recorded() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();
    let members = soroban_sdk::vec![
        &w.env,
        AllocationMember {
            member_hash: BytesN::from_array(&w.env, &[1u8; 32]),
            share_bps: 6_000,
        },
        AllocationMember {
            member_hash: BytesN::from_array(&w.env, &[2u8; 32]),
            share_bps: 4_000,
        },
    ];
    w.contract.set_allocation(&members);
    log.collect(&w.env, &w.contract.address);

    let view = fold_log(&log, &w.env, &w.contract.address);
    assert_eq!(view.member_count, Some(2), "member_count");
    assert_eq!(view.total_share_bps, Some(10_000), "total_share_bps");
}

// ---------------------------------------------------------------------------
// Path 7: Cancelled — final Cancelled status
// ---------------------------------------------------------------------------

#[test]
fn path_cancelled() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();

    w.contract.lock();
    log.collect(&w.env, &w.contract.address);
    w.contract.cancel();
    log.collect(&w.env, &w.contract.address);

    let view = fold_log(&log, &w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(Status::Cancelled), "expected Cancelled");
    assert_eq!(view.status, Some(c.status), "status mismatch");
}

// ---------------------------------------------------------------------------
// Path 8: Both tranches expired
// ---------------------------------------------------------------------------

#[test]
fn path_both_tranches_expired() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();

    w.contract.lock();
    log.collect(&w.env, &w.contract.address);
    w.contract.release_advance_1();
    log.collect(&w.env, &w.contract.address);
    let d1 = w.contract.get_commitment().advance1_deadline;
    w.set_time(d1 + 1);
    w.contract.reclaim_advance_1();
    log.collect(&w.env, &w.contract.address);
    w.contract.mark_checkpoint();
    log.collect(&w.env, &w.contract.address);
    w.contract.release_advance_2();
    log.collect(&w.env, &w.contract.address);
    let d2 = w.contract.get_commitment().advance2_deadline;
    w.set_time(d2 + 1);
    w.contract.reclaim_advance_2();
    log.collect(&w.env, &w.contract.address);

    let view = fold_log(&log, &w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert!(view.advance1_expired, "advance1_expired");
    assert!(view.advance2_expired, "advance2_expired");
    assert_eq!(view.advance1_expired, c.advance1_expired, "advance1 mismatch");
    assert_eq!(view.advance2_expired, c.advance2_expired, "advance2 mismatch");
}

// ---------------------------------------------------------------------------
// Path 9: DeliveryConfirmed event XDR exact match
// ---------------------------------------------------------------------------

#[test]
fn delivery_confirmed_event_exact_match() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();
    drive_to_remainder_funded_logged(&w, &mut log);
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    log.collect(&w.env, &w.contract.address);

    let c = w.contract.get_commitment();
    let expected = DeliveryConfirmed {
        grade_index: 0,
        delivered_quantity: CONTRACTED_QUANTITY,
        settlement_bps: c.settlement_bps,
    }
    .to_xdr(&w.env, &w.contract.address);

    assert!(log_has_event(&log, &expected), "DeliveryConfirmed event not found");
}

// ---------------------------------------------------------------------------
// Path 10: AllocationRecorded XDR exact match
// ---------------------------------------------------------------------------

#[test]
fn allocation_recorded_event_exact_match() {
    let w = world(3_000, 2_000);
    let mut log = EventLog::new();
    let members = soroban_sdk::vec![
        &w.env,
        AllocationMember {
            member_hash: BytesN::from_array(&w.env, &[0xABu8; 32]),
            share_bps: 10_000,
        },
    ];
    w.contract.set_allocation(&members);
    log.collect(&w.env, &w.contract.address);

    let expected = AllocationRecorded {
        member_count: 1,
        total_share_bps: 10_000,
    }
    .to_xdr(&w.env, &w.contract.address);

    assert!(log_has_event(&log, &expected), "AllocationRecorded event not found");
}

// ---------------------------------------------------------------------------
// Fields NOT derivable from events alone (catalogued per issue #216)
// ---------------------------------------------------------------------------
//
//   • buyer (initial)           — set at initialize; BuyerReassigned only
//                                 tracks *changes*, not the original value
//   • cooperative               — set at initialize, never emitted
//   • warehouse_operator        — set at initialize, never emitted
//   • token                     — set at initialize, never emitted
//   • total_amount              — set at initialize, never emitted
//   • advance1_bps              — set at initialize, never emitted
//   • advance2_bps              — set at initialize, never emitted
//   • claim_window_secs         — set at initialize, never emitted
//   • remainder_window_secs     — set at initialize, never emitted
//   • created_at                — set at initialize, never emitted
//   • delivery_deadline         — computed at initialize, never emitted
//   • advance1_deadline         — computed at release_advance_1, not in events
//   • advance2_deadline         — computed at release_advance_2, not in events
//   • remainder_deadline        — computed at ready_for_delivery, not in events
//   • fx_shortfall_deadline     — FxResolved.deadline covers this when non-zero
//   • contracted_quantity       — set at initialize, never emitted
//   • grade_price_bps           — set at initialize, never emitted
//   • fx_quote_stale            — derivable via FxQuoteStaleChanged (not
//                                 tested above: requires a live oracle mock)
//   • fx_adjusted_total         — derivable via FxResolved.fx_adjusted_total
//   • fx_shortfall_amount       — derivable via FxResolved.shortfall
//   • pre_dispute_status        — internal bookkeeping, not in events
