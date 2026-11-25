//! Overlapping advance-tranche windows.
//!
//! Every other lifecycle test in this repo resolves tranche 1 (claim or
//! reclaim) before tranche 2 is ever opened. Nothing in the contract
//! enforces that ordering — `mark_checkpoint` only needs
//! `Status::Advance1Released`, and its own doc comment says so explicitly:
//! "Independent of whether tranche 1 has actually been claimed yet — crop
//! progress doesn't wait on paperwork." With a short-enough checkpoint
//! delay, tranche 2's window opens while tranche 1's is still open, which
//! is entirely realistic.
//!
//! The two tranches are tracked by separate flags (`advance1_claimed`/
//! `advance1_expired` and the `advance2_*` pair) and separate deadlines,
//! and `claim_tranche`/`reclaim_tranche` are parameterized by a `Tranche`
//! enum, so the overlap *should* be inert. This suite proves it rather
//! than trusting the shape: tranche 2 is claimed while tranche 1's window
//! is still open, tranche 1 then lapses and is reclaimed, and every party's
//! balance is checked after each step — then the commitment is driven to
//! `settle` and the final split is asserted, so a cross-contamination bug
//! that only shows up in the payout math can't hide behind the flags.

#[path = "common/mod.rs"]
mod common;

use common::{world_with_window, World};
use harvestlock_escrow::{Error, Status};

/// A short claim window, so both windows are genuinely open at once without
/// having to move the simulated clock by days.
const WINDOW: u64 = 1_000;
const ADVANCE1_BPS: u32 = 1_500;
const ADVANCE2_BPS: u32 = 2_000;
const CHECKPOINT_AT: u64 = 400;
const CLAIM2_AT: u64 = 500;
const RECLAIM1_AT: u64 = 1_001; // one tick past `advance1_deadline`

/// `(buyer, cooperative, contract)` token balances, asserted as one tuple
/// so a failure names every leg instead of just the one that moved.
fn balances(s: &World) -> (i128, i128, i128) {
    (
        s.token.balance(&s.buyer),
        s.token.balance(&s.cooperative),
        s.token.balance(&s.contract.address),
    )
}

#[test]
fn tranche_2_claim_and_tranche_1_reclaim_do_not_cross_contaminate() {
    let s = world_with_window(ADVANCE1_BPS, ADVANCE2_BPS, WINDOW);
    let deposit = s.bps(ADVANCE1_BPS + ADVANCE2_BPS);
    let advance1 = s.bps(ADVANCE1_BPS);
    let advance2 = s.bps(ADVANCE2_BPS);
    let remainder = s.total_amount - deposit;

    // ---------- t=0: fund the deposit, open tranche 1 ----------
    s.contract.lock();
    assert_eq!(s.status(), Status::Locked);
    assert_eq!(balances(&s), (s.total_amount - deposit, 0, deposit));

    s.contract.release_advance_1();
    let c = s.contract.get_commitment();
    assert_eq!(c.status, Status::Advance1Released);
    assert_eq!(c.advance1_deadline, WINDOW);
    assert!(!c.advance1_claimed && !c.advance1_expired);
    assert!(!c.advance2_claimed && !c.advance2_expired);
    assert_eq!(balances(&s), (s.total_amount - deposit, 0, deposit));

    // ---------- t=400: checkpoint lands with tranche 1 still unresolved ----------
    s.set_time(CHECKPOINT_AT);
    s.contract.mark_checkpoint();
    let c = s.contract.get_commitment();
    assert_eq!(c.status, Status::CheckpointPassed);
    assert!(
        !c.advance1_claimed && !c.advance1_expired,
        "checkpoint must not resolve tranche 1 on the caller's behalf"
    );
    assert_eq!(balances(&s), (s.total_amount - deposit, 0, deposit));

    // ---------- tranche 2 opens while tranche 1's window is still open ----------
    s.contract.release_advance_2();
    let c = s.contract.get_commitment();
    assert_eq!(c.status, Status::Advance2Released);
    assert_eq!(c.advance2_deadline, CHECKPOINT_AT + WINDOW);
    assert!(
        c.advance2_deadline > c.advance1_deadline,
        "tranche 2's deadline must be later than tranche 1's, i.e. the \
         windows genuinely overlap (advance1={}, advance2={})",
        c.advance1_deadline,
        c.advance2_deadline
    );
    // The real invariant under test: right now, *both* machines are open.
    assert!(
        s.now() < c.advance1_deadline && s.now() < c.advance2_deadline,
        "both tranche windows must be open simultaneously at t={}",
        s.now()
    );
    assert_eq!(balances(&s), (s.total_amount - deposit, 0, deposit));

    // ---------- t=500: tranche 2 claimed while tranche 1 is still open ----------
    s.set_time(CLAIM2_AT);
    s.contract.claim_advance_2();
    let c = s.contract.get_commitment();
    assert!(c.advance2_claimed);
    assert!(!c.advance2_expired);
    // Tranche 2's resolution says nothing about tranche 1.
    assert!(
        !c.advance1_claimed && !c.advance1_expired,
        "claiming tranche 2 must not touch tranche 1's resolution flags"
    );
    assert_eq!(
        balances(&s),
        (s.total_amount - deposit, advance2, deposit - advance2)
    );
    // Tranche 2 is resolved once only.
    assert_eq!(
        s.contract.try_claim_advance_2(),
        Err(Ok(Error::AlreadyClaimed))
    );
    assert_eq!(
        s.contract.try_reclaim_advance_2(),
        Err(Ok(Error::AlreadyClaimed))
    );

    // ---------- t=1001: tranche 1 lapses and is reclaimed ----------
    s.set_time(RECLAIM1_AT);
    assert!(RECLAIM1_AT > s.contract.get_commitment().advance1_deadline);
    assert!(RECLAIM1_AT < s.contract.get_commitment().advance2_deadline);

    s.contract.reclaim_advance_1();
    let c = s.contract.get_commitment();
    assert!(c.advance1_expired && !c.advance1_claimed);
    assert!(
        c.advance2_claimed && !c.advance2_expired,
        "reclaiming tranche 1 must not touch tranche 2's resolution flags"
    );
    assert_eq!(
        balances(&s),
        (s.total_amount - deposit + advance1, advance2, 0)
    );
    // Tranche 1 is resolved once only, and the late claim is refused.
    assert_eq!(
        s.contract.try_claim_advance_1(),
        Err(Ok(Error::AlreadyExpired))
    );
    assert_eq!(
        s.contract.try_reclaim_advance_1(),
        Err(Ok(Error::AlreadyExpired))
    );

    // ---------- fund the remainder, then deliver in full ----------
    s.contract.ready_for_delivery();
    assert_eq!(s.status(), Status::ReadyForDelivery);
    assert_eq!(
        balances(&s),
        (s.total_amount - deposit + advance1, advance2, 0)
    );

    s.contract.fund_remainder();
    let buyer_after_funding = s.total_amount - deposit + advance1 - remainder;
    assert_eq!(balances(&s), (buyer_after_funding, advance2, remainder));

    s.contract
        .confirm_delivery(&common::CONTRACTED_QUANTITY, &0);
    let c = s.contract.get_commitment();
    assert_eq!(c.status, Status::Delivered);
    assert_eq!(c.settlement_bps, 10_000);
    assert_eq!(balances(&s), (buyer_after_funding, advance2, remainder));

    // ---------- settle: the final split ----------
    s.contract.settle();
    assert_eq!(s.status(), Status::Settled);
    s.contract.withdraw_payout(&s.cooperative);

    // Full delivery at full grade (settlement_bps = 10_000) is worth the
    // whole total. The cooperative already banked tranche 2 only, because
    // tranche 1 lapsed and went back to the buyer. So it is owed
    // `total - advance2` from the remainder, but only the remainder is
    // actually escrowed, so the shortfall against the full total stays with
    // the buyer — exactly what an unresolved-then-reclaimed tranche 1 means
    // for the split.
    let coop_total = advance2 + remainder;
    let buyer_total = s.total_amount - coop_total;
    assert_eq!(balances(&s), (buyer_total, coop_total, 0));
    assert_eq!(buyer_total + coop_total, s.total_amount);
    // Concretely: coop 850,000, buyer 150,000 of 1,000,000, i.e. the
    // cooperative is paid the full total minus the advance it never claimed
    // (which the buyer got back), and the buyer keeps nothing else.
    assert_eq!(coop_total, s.total_amount - advance1);
    assert_eq!(buyer_total, advance1);
}
