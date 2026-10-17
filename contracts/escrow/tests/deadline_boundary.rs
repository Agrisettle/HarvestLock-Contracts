//! #111 — exact deadline-boundary coverage for `advance1_deadline` /
//! `advance2_deadline`.
//!
//! The question this suite answers: when the ledger timestamp is *exactly*
//! equal to a tranche's deadline — not one second either side — which party
//! is allowed to act?
//!
//! **Actual operators in `lib.rs` (confirmed by reading the code, not
//! assumed):**
//!
//! - `claim_tranche` (reached via `claim_advance_1`/`claim_advance_2`):
//!   `if env.ledger().timestamp() > Self::tranche_deadline(&c, t) { Err(ClaimWindowPassed) }`
//!   → claiming is **inclusive** of the deadline instant.
//! - `reclaim_tranche` (reached via `reclaim_advance_1`/`reclaim_advance_2`):
//!   `if env.ledger().timestamp() <= Self::tranche_deadline(&c, t) { Err(ClaimWindowNotPassed) }`
//!   → reclaiming is **exclusive** of the deadline instant (earliest success
//!   is `deadline + 1`).
//!
//! The two guards are exact logical complements, so at every single ledger
//! timestamp exactly one of the two is available and neither is ever
//! simultaneously available or simultaneously blocked. There is no
//! off-by-one window where both are rejected (a deadlock) and no instant
//! where both are accepted (a double-spend of the tranche).
//!
//! Same shape, for reference, on the other deadline guards in this contract
//! (`fund_remainder` `>`, `expire_remainder_window` `<=`,
//! `reclaim_on_nondelivery` `<=`, `fund_fx_shortfall` `>`,
//! `expire_fx_shortfall_window` `<=`, `expire_dispute_window` `<=`) — each
//! pair is complementary. Only the advance-tranche pair is pinned here, per
//! the issue's scope.

#[path = "common/mod.rs"]
mod common;

use common::world;
use harvestlock_escrow::{Error, Status};

// ---------- tranche 1, the exact boundary ----------

#[test]
fn claim_advance_1_exactly_at_deadline_succeeds() {
    let s = world(1_500, 2_000);
    let deadline = s.locked_tranche1_open();
    assert_eq!(
        deadline,
        common::WINDOW,
        "deadline == release time + window"
    );

    s.set_time(deadline); // now == advance1_deadline, exactly
    assert_eq!(s.now(), s.contract.get_commitment().advance1_deadline);

    s.contract.claim_advance_1();
    assert!(s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.cooperative), s.bps(1_500));
    assert_eq!(s.token.balance(&s.buyer), s.total_amount - s.bps(3_500));
}

#[test]
fn claim_advance_1_exactly_one_tick_past_deadline_is_rejected() {
    let s = world(1_500, 2_000);
    let deadline = s.locked_tranche1_open();

    s.set_time(deadline + 1);
    assert_eq!(
        s.contract.try_claim_advance_1(),
        Err(Ok(Error::ClaimWindowPassed))
    );
    // Rejection must not resolve the tranche: no side effects landed.
    let c = s.contract.get_commitment();
    assert!(!c.advance1_claimed && !c.advance1_expired);
    assert_eq!(s.status(), Status::Advance1Released);
}

#[test]
fn reclaim_advance_1_exactly_at_deadline_is_rejected() {
    let s = world(1_500, 2_000);
    let deadline = s.locked_tranche1_open();

    s.set_time(deadline); // the instant *before* the window is considered passed
    assert_eq!(
        s.contract.try_reclaim_advance_1(),
        Err(Ok(Error::ClaimWindowNotPassed))
    );
    assert_eq!(s.token.balance(&s.buyer), s.total_amount - s.bps(3_500));
}

#[test]
fn reclaim_advance_1_exactly_one_tick_past_deadline_succeeds() {
    let s = world(1_500, 2_000);
    let deadline = s.locked_tranche1_open();

    s.set_time(deadline + 1);
    s.contract.reclaim_advance_1();

    let c = s.contract.get_commitment();
    assert!(c.advance1_expired && !c.advance1_claimed);
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - s.bps(3_500) + s.bps(1_500)
    );
    assert_eq!(s.token.balance(&s.cooperative), 0);
}

// ---------- tranche 2, the exact boundary ----------

#[test]
fn claim_advance_2_exactly_at_deadline_succeeds() {
    let s = world(1_500, 2_000);
    let deadline = s.tranche2_open();
    assert_eq!(deadline, common::WINDOW);

    s.set_time(deadline);
    assert_eq!(s.now(), s.contract.get_commitment().advance2_deadline);

    s.contract.claim_advance_2();
    assert!(s.contract.get_commitment().advance2_claimed);
    assert_eq!(s.token.balance(&s.cooperative), s.bps(2_000));
}

#[test]
fn claim_advance_2_exactly_one_tick_past_deadline_is_rejected() {
    let s = world(1_500, 2_000);
    let deadline = s.tranche2_open();

    s.set_time(deadline + 1);
    assert_eq!(
        s.contract.try_claim_advance_2(),
        Err(Ok(Error::ClaimWindowPassed))
    );
    let c = s.contract.get_commitment();
    assert!(!c.advance2_claimed && !c.advance2_expired);
}

#[test]
fn reclaim_advance_2_exactly_at_deadline_is_rejected() {
    let s = world(1_500, 2_000);
    let deadline = s.tranche2_open();

    s.set_time(deadline);
    assert_eq!(
        s.contract.try_reclaim_advance_2(),
        Err(Ok(Error::ClaimWindowNotPassed))
    );
}

#[test]
fn reclaim_advance_2_exactly_one_tick_past_deadline_succeeds() {
    let s = world(1_500, 2_000);
    let deadline = s.tranche2_open();

    s.set_time(deadline + 1);
    s.contract.reclaim_advance_2();

    let c = s.contract.get_commitment();
    assert!(c.advance2_expired && !c.advance2_claimed);
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - s.bps(3_500) + s.bps(2_000)
    );
}

// ---------- the boundary as a partition ----------

/// Both sides of the boundary, in one test, against the *same* deadline
/// value: at `D` only the cooperative's claim is available; at `D + 1` only
/// the buyer's reclaim is. This is the property no clearly-before /
/// clearly-after test can prove on its own.
#[test]
fn advance1_deadline_instant_belongs_to_claim_not_reclaim() {
    let s = world(1_500, 2_000);
    let deadline = s.locked_tranche1_open();

    s.set_time(deadline);
    s.contract.claim_advance_1(); // claim wins the boundary instant

    let s2 = world(1_500, 2_000);
    s2.locked_tranche1_open();
    s2.set_time(deadline);
    // and reclaim is precisely what loses it
    assert_eq!(
        s2.contract.try_reclaim_advance_1(),
        Err(Ok(Error::ClaimWindowNotPassed))
    );
    s2.set_time(deadline + 1);
    s2.contract.reclaim_advance_1();
    assert!(s2.contract.get_commitment().advance1_expired);
}

#[test]
fn advance2_deadline_instant_belongs_to_claim_not_reclaim() {
    let s = world(1_500, 2_000);
    let deadline = s.tranche2_open();

    s.set_time(deadline);
    s.contract.claim_advance_2();

    let s2 = world(1_500, 2_000);
    s2.tranche2_open();
    s2.set_time(deadline);
    assert_eq!(
        s2.contract.try_reclaim_advance_2(),
        Err(Ok(Error::ClaimWindowNotPassed))
    );
    s2.set_time(deadline + 1);
    s2.contract.reclaim_advance_2();
    assert!(s2.contract.get_commitment().advance2_expired);
}

/// The complement of the previous two: no timestamp where both actions are
/// available. Asserted on fresh worlds, since claiming and reclaiming both
/// mutate the tranche's resolution flags.
#[test]
fn no_timestamp_lets_both_claim_and_reclaim_the_same_tranche() {
    for offset in [common::WINDOW - 1, common::WINDOW, common::WINDOW + 1] {
        let a = world(1_500, 2_000);
        let deadline = a.locked_tranche1_open();
        a.set_time(offset);
        let claim_ok = a.contract.try_claim_advance_1().is_ok();

        let b = world(1_500, 2_000);
        b.locked_tranche1_open();
        b.set_time(offset);
        let reclaim_ok = b.contract.try_reclaim_advance_1().is_ok();

        assert!(
            claim_ok != reclaim_ok,
            "t={offset} (deadline={deadline}): claim_allowed={claim_ok} \
             reclaim_allowed={reclaim_ok} — exactly one must be allowed"
        );
    }
}

/// A tranche whose deadline has lapsed keeps both guards' *other* errors
/// intact at the boundary: `reclaim` at exactly `D` is `ClaimWindowNotPassed`
/// (a deadline error), not `AlreadyClaimed`/`AlreadyExpired`/`NotYetOpened`.
#[test]
fn boundary_error_is_the_deadline_error_not_a_state_error() {
    let s = world(1_500, 2_000);
    let deadline = s.locked_tranche1_open();
    s.set_time(deadline);

    assert_eq!(
        s.contract.try_reclaim_advance_1(),
        Err(Ok(Error::ClaimWindowNotPassed))
    );
    // And the tranche is still open one tick later, i.e. the boundary
    // instant did not itself expire anything.
    let c = s.contract.get_commitment();
    assert!(!c.advance1_claimed && !c.advance1_expired);
    s.set_time(deadline + 1);
    s.contract.reclaim_advance_1();
}
