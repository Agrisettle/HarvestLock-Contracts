//! Issue #216 — Prove the event stream is enough to rebuild a commitment's
//! state without reading storage.
//!
//! Each test drives the contract through a lifecycle path, then rebuilds an
//! `IndexerView` by scanning the filtered event list (`filter_by_contract`)
//! for known event XDR signatures produced by `.to_xdr()`. After folding,
//! every event-derivable field is asserted against `get_commitment()`.
//!
//! Fields that cannot be derived from events alone are catalogued at the
//! bottom of this file.

#![allow(dead_code)]

#[path = "common/mod.rs"]
mod common;

use common::{world, World, CONTRACTED_QUANTITY};
use harvestlock_escrow::{
    AllocationMember, AllocationRecorded, BuyerReassigned, DeliveryConfirmed, FxShortfallFunded,
    RemainderFunded, Status, StatusChanged, TrancheClaimed, TrancheReclaimed,
};
use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env, Event as _};
use soroban_sdk::xdr;

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
// Reducer: fold all events emitted by `contract` into an IndexerView.
//
// We iterate over `env.events().all().filter_by_contract()` which returns
// an XDR event slice. We decode each event's topics and data map using the
// XDR types. For StatusChanged we probe each known Status variant.
// ---------------------------------------------------------------------------

fn fold_events(env: &Env, contract: &Address) -> IndexerView {
    use soroban_sdk::xdr::{ContractEventBody, ScVal};

    let mut view = IndexerView::default();
    let filtered = env.events().all().filter_by_contract(contract);
    let events = filtered.events();

    for event in events {
        let ContractEventBody::V0(body) = &event.body else { continue };
        let Some(ScVal::Symbol(sym)) = body.topics.first() else { continue };
        let sym_bytes: &[u8] = sym.as_ref();

        match sym_bytes {
            b"StatusChanged" => {
                // topic[2] = `to` Status — probe each variant to find which.
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
            b"FxResolved" => {
                view.fx_resolved = true;
            }
            b"FxShortfallFunded" => {
                view.fx_shortfall_funded = true;
            }
            b"BuyerReassigned" => {
                view.buyer_reassigned = true;
            }
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

/// Check whether a specific event XDR appears anywhere in the stream.
fn has_event(env: &Env, contract: &Address, expected: &xdr::ContractEvent) -> bool {
    env.events().all().events().iter().any(|e| e == expected)
}

// ---------------------------------------------------------------------------
// Path 1: Draft → Locked → Advance1Released → advance1 claimed
// ---------------------------------------------------------------------------

#[test]
fn path_lock_and_claim_advance1() {
    let w = world(3_000, 2_000);
    w.contract.lock();
    w.contract.release_advance_1();
    w.contract.claim_advance_1();

    let view = fold_events(&w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(c.status));
    assert_eq!(view.advance1_claimed, c.advance1_claimed);
    assert_eq!(view.advance1_expired, c.advance1_expired);
}

// ---------------------------------------------------------------------------
// Path 2: Through to ReadyForDelivery with remainder funded
// ---------------------------------------------------------------------------

#[test]
fn path_remainder_funded() {
    let w = world(3_000, 2_000);
    w.drive_to_remainder_funded();

    let view = fold_events(&w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(c.status));
    assert_eq!(view.advance1_claimed, c.advance1_claimed);
    assert_eq!(view.advance2_claimed, c.advance2_claimed);
    assert!(view.remainder_funded);
    assert_eq!(view.remainder_funded, c.remainder_funded);
}

// ---------------------------------------------------------------------------
// Path 3: Through to Delivered — confirm_delivery fields
// ---------------------------------------------------------------------------

#[test]
fn path_delivered_full_quantity() {
    let w = world(3_000, 2_000);
    w.drive_to_delivered();

    let view = fold_events(&w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(Status::Delivered));
    assert_eq!(view.status, Some(c.status));
    assert_eq!(view.delivered_quantity, Some(c.delivered_quantity));
    assert_eq!(view.grade_index, Some(c.grade_index));
    assert_eq!(view.settlement_bps, Some(c.settlement_bps));
}

// ---------------------------------------------------------------------------
// Path 4: Tranche expiry (reclaim path)
// ---------------------------------------------------------------------------

#[test]
fn path_tranche1_expired_reclaimed() {
    let w = world(3_000, 2_000);
    w.contract.lock();
    w.contract.release_advance_1();
    let deadline = w.contract.get_commitment().advance1_deadline;
    w.set_time(deadline + 1);
    w.contract.reclaim_advance_1();

    let view = fold_events(&w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.advance1_expired, c.advance1_expired);
    assert!(view.advance1_expired);
    assert!(!view.advance1_claimed);
}

// ---------------------------------------------------------------------------
// Path 5: Buyer reassignment — BuyerReassigned event in stream
// ---------------------------------------------------------------------------

#[test]
fn path_buyer_reassigned() {
    let w = world(3_000, 2_000);
    let new_buyer = Address::generate(&w.env);
    let old_buyer = w.buyer.clone();
    w.contract.reassign_buyer(&new_buyer);

    let c = w.contract.get_commitment();

    let expected = BuyerReassigned {
        previous: old_buyer,
        new_buyer: new_buyer.clone(),
    }
    .to_xdr(&w.env, &w.contract.address);

    assert!(
        has_event(&w.env, &w.contract.address, &expected),
        "BuyerReassigned event not found in stream"
    );
    assert_eq!(c.buyer, new_buyer);
}

// ---------------------------------------------------------------------------
// Path 6: Allocation recorded — AllocationRecorded fields
// ---------------------------------------------------------------------------

#[test]
fn path_allocation_recorded() {
    let w = world(3_000, 2_000);
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

    let view = fold_events(&w.env, &w.contract.address);
    assert_eq!(view.member_count, Some(2));
    assert_eq!(view.total_share_bps, Some(10_000));
}

// ---------------------------------------------------------------------------
// Path 7: Cancelled — final Cancelled status
// ---------------------------------------------------------------------------

#[test]
fn path_cancelled() {
    let w = world(3_000, 2_000);
    w.contract.lock();
    w.contract.cancel();

    let view = fold_events(&w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert_eq!(view.status, Some(Status::Cancelled));
    assert_eq!(view.status, Some(c.status));
}

// ---------------------------------------------------------------------------
// Path 8: Both tranches expired then cancelled
// ---------------------------------------------------------------------------

#[test]
fn path_both_tranches_expired() {
    let w = world(3_000, 2_000);
    w.contract.lock();
    w.contract.release_advance_1();
    let d1 = w.contract.get_commitment().advance1_deadline;
    w.set_time(d1 + 1);
    w.contract.reclaim_advance_1();
    w.contract.mark_checkpoint();
    w.contract.release_advance_2();
    let d2 = w.contract.get_commitment().advance2_deadline;
    w.set_time(d2 + 1);
    w.contract.reclaim_advance_2();

    let view = fold_events(&w.env, &w.contract.address);
    let c = w.contract.get_commitment();

    assert!(view.advance1_expired);
    assert!(view.advance2_expired);
    assert_eq!(view.advance1_expired, c.advance1_expired);
    assert_eq!(view.advance2_expired, c.advance2_expired);
}

// ---------------------------------------------------------------------------
// Path 9: DeliveryConfirmed event XDR exact match
// ---------------------------------------------------------------------------

#[test]
fn delivery_confirmed_event_exact_match() {
    let w = world(3_000, 2_000);
    w.drive_to_remainder_funded();
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);

    let c = w.contract.get_commitment();
    let expected = DeliveryConfirmed {
        grade_index: 0,
        delivered_quantity: CONTRACTED_QUANTITY,
        settlement_bps: c.settlement_bps,
    }
    .to_xdr(&w.env, &w.contract.address);

    assert!(
        has_event(&w.env, &w.contract.address, &expected),
        "DeliveryConfirmed event not found or has wrong fields"
    );
}

// ---------------------------------------------------------------------------
// Path 10: AllocationRecorded event XDR exact match
// ---------------------------------------------------------------------------

#[test]
fn allocation_recorded_event_exact_match() {
    let w = world(3_000, 2_000);
    let members = soroban_sdk::vec![
        &w.env,
        AllocationMember {
            member_hash: BytesN::from_array(&w.env, &[0xABu8; 32]),
            share_bps: 10_000,
        },
    ];
    w.contract.set_allocation(&members);

    let expected = AllocationRecorded {
        member_count: 1,
        total_share_bps: 10_000,
    }
    .to_xdr(&w.env, &w.contract.address);

    assert!(
        has_event(&w.env, &w.contract.address, &expected),
        "AllocationRecorded event not found or has wrong fields"
    );
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
