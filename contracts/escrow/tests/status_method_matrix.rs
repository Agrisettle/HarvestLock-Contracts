//! #257 — Status-by-method matrix: machine-checkable version of the
//! status-gate table in `docs/authorization-matrix.md`.
//!
//! For every public state-changing function this suite records the set of
//! statuses in which it succeeds and verifies that calling it in a
//! representative *wrong* status returns `Error::InvalidState`.
//!
//! ## Why test `InvalidState`, not every status cell?
//!
//! The status machine has 12 × 20+ = 240+ cells. Exhaustive coverage would
//! mean hundreds of micro-tests that mostly test Rust's match arm ordering.
//! Instead this suite pins three things that are actually risky to get wrong:
//!
//! 1. **Terminal-state barricade** — every non-view function returns
//!    `InvalidState` from each of the five terminal states
//!    (Settled, Cancelled, Defaulted, Forfeited, Disputed).
//!    This is the class of regression that matters most: a future refactor
//!    that forgets to list a terminal status in a `match _ => InvalidState`
//!    arm is caught here, not by a happy-path test.
//!
//! 2. **Fencepost transitions** — the two adjacent states to a function's
//!    correct state (one step before, one step after). An off-by-one in a
//!    match arm is caught here.
//!
//! 3. **Guard-only functions** — functions whose status gate is a single-
//!    variant match (`lock` wants `Draft`, `mark_checkpoint` wants
//!    `Advance1Released`). Testing them from the wrong state is the only
//!    machine check of the guard short of reading the code.
//!
//! Happy-path tests for each function live in `src/test.rs` (the original
//! unit suite) and the other regression suites.

#[path = "common/mod.rs"]
mod common;

use common::{world, World, CONTRACTED_QUANTITY, DELIVERY_WINDOW};
use harvestlock_escrow::Error;
use soroban_sdk::testutils::Address as _;

// ---------------------------------------------------------------------------
// Drive helpers (without touching status-gated functions we're testing)
// ---------------------------------------------------------------------------

fn drive_to_locked(w: &World) {
    w.contract.lock();
}

fn drive_to_advance1_released(w: &World) {
    drive_to_locked(w);
    w.contract.release_advance_1();
}

fn drive_to_checkpoint_passed(w: &World) {
    drive_to_advance1_released(w);
    w.contract.mark_checkpoint();
}

fn drive_to_advance2_released(w: &World) {
    drive_to_checkpoint_passed(w);
    w.contract.release_advance_2();
}

fn drive_to_ready_for_delivery(w: &World) {
    drive_to_advance2_released(w);
    w.contract.ready_for_delivery();
}

fn drive_to_remainder_funded(w: &World) {
    drive_to_advance2_released(w);
    w.contract.claim_advance_1();
    w.contract.claim_advance_2();
    w.contract.ready_for_delivery();
    w.contract.fund_remainder();
}

fn drive_to_delivered(w: &World) {
    drive_to_remainder_funded(w);
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
}

fn drive_to_settled(w: &World) {
    drive_to_delivered(w);
    w.contract.settle();
}

fn drive_to_cancelled(w: &World) {
    drive_to_locked(w);
    w.contract.cancel();
}

fn drive_to_defaulted(w: &World) {
    drive_to_ready_for_delivery(w);
    // Advance past the remainder deadline.
    let deadline = w.contract.get_commitment().remainder_deadline;
    w.set_time(deadline + 1);
    w.contract.expire_remainder_window();
}

fn drive_to_forfeited(w: &World) {
    drive_to_locked(w);
    w.advance(DELIVERY_WINDOW + 1);
    w.contract.reclaim_on_nondelivery();
}

fn drive_to_disputed_from_locked(w: &World) {
    drive_to_locked(w);
    w.contract.flag_dispute(&w.buyer);
}

// ---------------------------------------------------------------------------
// §1 — `lock`: requires Status::Draft
// ---------------------------------------------------------------------------

#[test]
fn lock_fails_in_locked() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    assert_eq!(w.contract.try_lock(), Err(Ok(Error::InvalidState)));
}

#[test]
fn lock_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(w.contract.try_lock(), Err(Ok(Error::InvalidState)));
}

#[test]
fn lock_fails_in_cancelled() {
    let w = world(1_500, 2_000);
    drive_to_cancelled(&w);
    assert_eq!(w.contract.try_lock(), Err(Ok(Error::InvalidState)));
}

// ---------------------------------------------------------------------------
// §2 — `release_advance_1`: requires Status::Locked
// ---------------------------------------------------------------------------

#[test]
fn release_advance_1_fails_in_draft() {
    let w = world(1_500, 2_000);
    assert_eq!(
        w.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn release_advance_1_fails_in_advance1_released() {
    let w = world(1_500, 2_000);
    drive_to_advance1_released(&w);
    assert_eq!(
        w.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn release_advance_1_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(
        w.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §3 — `mark_checkpoint`: requires Status::Advance1Released
// ---------------------------------------------------------------------------

#[test]
fn mark_checkpoint_fails_in_locked() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    assert_eq!(
        w.contract.try_mark_checkpoint(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn mark_checkpoint_fails_in_checkpoint_passed() {
    let w = world(1_500, 2_000);
    drive_to_checkpoint_passed(&w);
    assert_eq!(
        w.contract.try_mark_checkpoint(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn mark_checkpoint_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(
        w.contract.try_mark_checkpoint(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §4 — `release_advance_2`: requires Status::CheckpointPassed
// ---------------------------------------------------------------------------

#[test]
fn release_advance_2_fails_in_advance1_released() {
    let w = world(1_500, 2_000);
    drive_to_advance1_released(&w);
    assert_eq!(
        w.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn release_advance_2_fails_in_advance2_released() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    assert_eq!(
        w.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §5 — `ready_for_delivery`: requires Status::Advance2Released
// ---------------------------------------------------------------------------

#[test]
fn ready_for_delivery_fails_in_checkpoint_passed() {
    let w = world(1_500, 2_000);
    drive_to_checkpoint_passed(&w);
    assert_eq!(
        w.contract.try_ready_for_delivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn ready_for_delivery_fails_in_ready_for_delivery() {
    let w = world(1_500, 2_000);
    drive_to_ready_for_delivery(&w);
    assert_eq!(
        w.contract.try_ready_for_delivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn ready_for_delivery_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    assert_eq!(
        w.contract.try_ready_for_delivery(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §6 — `fund_remainder`: requires Status::ReadyForDelivery
// ---------------------------------------------------------------------------

#[test]
fn fund_remainder_fails_in_advance2_released() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    assert_eq!(
        w.contract.try_fund_remainder(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn fund_remainder_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    assert_eq!(
        w.contract.try_fund_remainder(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn fund_remainder_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(
        w.contract.try_fund_remainder(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §7 — `expire_remainder_window`: requires Status::ReadyForDelivery
// ---------------------------------------------------------------------------

#[test]
fn expire_remainder_window_fails_in_advance2_released() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    assert_eq!(
        w.contract.try_expire_remainder_window(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn expire_remainder_window_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    assert_eq!(
        w.contract.try_expire_remainder_window(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn expire_remainder_window_fails_in_defaulted() {
    let w = world(1_500, 2_000);
    drive_to_defaulted(&w);
    assert_eq!(
        w.contract.try_expire_remainder_window(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §8 — `confirm_delivery`: requires Status::ReadyForDelivery + remainder funded
// ---------------------------------------------------------------------------

#[test]
fn confirm_delivery_fails_in_advance2_released() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    assert_eq!(
        w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn confirm_delivery_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    assert_eq!(
        w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn confirm_delivery_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(
        w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn confirm_delivery_fails_when_remainder_not_funded() {
    // ReadyForDelivery is correct status but remainder not funded yet.
    let w = world(1_500, 2_000);
    drive_to_ready_for_delivery(&w);
    assert_eq!(
        w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0),
        Err(Ok(Error::RemainderNotFunded))
    );
}

// ---------------------------------------------------------------------------
// §9 — `settle`: requires Status::Delivered (and tranches resolved)
// ---------------------------------------------------------------------------

#[test]
fn settle_fails_in_ready_for_delivery() {
    let w = world(1_500, 2_000);
    drive_to_ready_for_delivery(&w);
    assert_eq!(w.contract.try_settle(), Err(Ok(Error::InvalidState)));
}

#[test]
fn settle_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(w.contract.try_settle(), Err(Ok(Error::InvalidState)));
}

#[test]
fn settle_fails_before_tranches_resolved() {
    // Deliver without claiming tranches first — settle must refuse.
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    w.contract.ready_for_delivery();
    w.contract.fund_remainder();
    // Tranches NOT claimed — confirm_delivery still allowed, then settle should fail.
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    assert_eq!(w.contract.try_settle(), Err(Ok(Error::TrancheUnresolved)));
}

// ---------------------------------------------------------------------------
// §10 — `reclaim_on_nondelivery`: Locked through ReadyForDelivery + deadline passed
// ---------------------------------------------------------------------------

#[test]
fn reclaim_on_nondelivery_fails_in_draft() {
    let w = world(1_500, 2_000);
    w.advance(DELIVERY_WINDOW + 1);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn reclaim_on_nondelivery_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    w.advance(DELIVERY_WINDOW + 1);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn reclaim_on_nondelivery_fails_before_delivery_deadline_in_locked() {
    // Right status (Locked), but deadline not passed yet.
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::DeliveryDeadlineNotPassed))
    );
}

// ---------------------------------------------------------------------------
// §11 — `cancel`: Draft through ReadyForDelivery; barred from Delivered+
// ---------------------------------------------------------------------------

#[test]
fn cancel_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_fails_in_cancelled() {
    let w = world(1_500, 2_000);
    drive_to_cancelled(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_fails_in_defaulted() {
    let w = world(1_500, 2_000);
    drive_to_defaulted(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_fails_in_forfeited() {
    let w = world(1_500, 2_000);
    drive_to_forfeited(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_fails_in_disputed() {
    let w = world(1_500, 2_000);
    drive_to_disputed_from_locked(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
}

// ---------------------------------------------------------------------------
// §12 — `reassign_buyer`: same range as `cancel`
// ---------------------------------------------------------------------------

#[test]
fn reassign_buyer_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    let new_buyer = soroban_sdk::Address::generate(&w.env);
    assert_eq!(
        w.contract.try_reassign_buyer(&new_buyer),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn reassign_buyer_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    let new_buyer = soroban_sdk::Address::generate(&w.env);
    assert_eq!(
        w.contract.try_reassign_buyer(&new_buyer),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn reassign_buyer_fails_in_cancelled() {
    let w = world(1_500, 2_000);
    drive_to_cancelled(&w);
    let new_buyer = soroban_sdk::Address::generate(&w.env);
    assert_eq!(
        w.contract.try_reassign_buyer(&new_buyer),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §13 — `flag_dispute`: Locked through Delivered; barred from Draft + terminals
// ---------------------------------------------------------------------------

#[test]
fn flag_dispute_fails_in_draft() {
    let w = world(1_500, 2_000);
    assert_eq!(
        w.contract.try_flag_dispute(&w.buyer.clone()),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn flag_dispute_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn flag_dispute_fails_in_cancelled() {
    let w = world(1_500, 2_000);
    drive_to_cancelled(&w);
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn flag_dispute_fails_in_disputed() {
    let w = world(1_500, 2_000);
    drive_to_disputed_from_locked(&w);
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
}

// ---------------------------------------------------------------------------
// §14 — `resolve_dispute` / `expire_dispute_window`: require Status::Disputed
// ---------------------------------------------------------------------------

#[test]
fn resolve_dispute_fails_in_locked() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    assert_eq!(
        w.contract.try_resolve_dispute(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn resolve_dispute_fails_in_delivered() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    assert_eq!(
        w.contract.try_resolve_dispute(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn expire_dispute_window_fails_in_locked() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    assert_eq!(
        w.contract.try_expire_dispute_window(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn expire_dispute_window_fails_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_eq!(
        w.contract.try_expire_dispute_window(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn expire_dispute_window_fails_before_deadline_elapses() {
    let w = world(1_500, 2_000);
    drive_to_disputed_from_locked(&w);
    // Status is Disputed but deadline hasn't passed yet.
    assert_eq!(
        w.contract.try_expire_dispute_window(),
        Err(Ok(Error::DisputeWindowNotPassed))
    );
}

// ---------------------------------------------------------------------------
// §15 — Terminal-state barricade: every status-changing function returns
//        InvalidState from each terminal: Settled, Cancelled, Defaulted,
//        Forfeited, Disputed (the last blocks via its own status guard)
// ---------------------------------------------------------------------------

/// Helper: collect all InvalidState results from a commitment in a terminal
/// state, excluding the tranche/deadline-dependent functions that return
/// different guard errors from certain states.
fn assert_all_mutators_blocked(w: &World) {
    let nb = soroban_sdk::Address::generate(&w.env);
    // State-advancing functions:
    assert_eq!(w.contract.try_lock(), Err(Ok(Error::InvalidState)));
    assert_eq!(
        w.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_mark_checkpoint(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_ready_for_delivery(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_fund_remainder(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_expire_remainder_window(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(w.contract.try_settle(), Err(Ok(Error::InvalidState)));
    assert_eq!(
        w.contract.try_resolve_dispute(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_expire_dispute_window(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_reassign_buyer(&nb),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn all_mutators_blocked_in_settled() {
    let w = world(1_500, 2_000);
    drive_to_settled(&w);
    assert_all_mutators_blocked(&w);
    // These two take status AND non-status guards:
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
    w.advance(DELIVERY_WINDOW + 1);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn all_mutators_blocked_in_cancelled() {
    let w = world(1_500, 2_000);
    drive_to_cancelled(&w);
    assert_all_mutators_blocked(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
    w.advance(DELIVERY_WINDOW + 1);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn all_mutators_blocked_in_defaulted() {
    let w = world(1_500, 2_000);
    drive_to_defaulted(&w);
    assert_all_mutators_blocked(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
    w.advance(DELIVERY_WINDOW + 1);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn all_mutators_blocked_in_forfeited() {
    let w = world(1_500, 2_000);
    drive_to_forfeited(&w);
    assert_all_mutators_blocked(&w);
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
    w.advance(DELIVERY_WINDOW + 1);
    assert_eq!(
        w.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn disputed_state_blocks_all_mutators_except_dispute_resolution() {
    let w = world(1_500, 2_000);
    drive_to_disputed_from_locked(&w);
    // Everything except resolve_dispute / expire_dispute_window returns
    // InvalidState — the status is Disputed and no other function names it.
    assert_eq!(w.contract.try_lock(), Err(Ok(Error::InvalidState)));
    assert_eq!(
        w.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_mark_checkpoint(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_ready_for_delivery(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_fund_remainder(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_expire_remainder_window(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(w.contract.try_settle(), Err(Ok(Error::InvalidState)));
    assert_eq!(w.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    let buyer = w.buyer.clone();
    assert_eq!(
        w.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::InvalidState))
    );
    // expire_dispute_window IS valid in Disputed — it just needs the deadline.
    // resolve_dispute IS valid in Disputed — tested separately.
}

// ---------------------------------------------------------------------------
// §16 — Source guard: every status-gated function has an InvalidState return
// ---------------------------------------------------------------------------

/// A status guard is either a literal `Error::InvalidState`, a call into the
/// status table (`require_status`, which returns `InvalidState`), or a call
/// to a `Self::can_*` precondition helper that itself holds one of those.
fn has_status_guard(src: &str, window: &str) -> bool {
    if window.contains("Error::InvalidState") || window.contains("require_status(") {
        return true;
    }
    window.match_indices("Self::can_").any(|(i, _)| {
        let name: std::string::String = window[i + "Self::".len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        src.find(&format!("fn {name}(")).is_some_and(|pos| {
            let body = &src[pos..(pos + 1500).min(src.len())];
            body.contains("Error::InvalidState") || body.contains("require_status(")
        })
    })
}

#[test]
fn every_status_gated_function_has_invalid_state_guard_in_source() {
    const SRC: &str = include_str!("../src/lib.rs");
    // Every function that is status-gated must have `Error::InvalidState`
    // somewhere in its implementation. Views and claim_*/reclaim_* (which
    // check tranche state only) are exempt.
    // Functions with a direct `Error::InvalidState` guard in their body.
    let direct_gated = [
        "fn lock(",
        "fn mark_checkpoint(",
        "fn ready_for_delivery(",
        "fn fund_remainder(",
        "fn expire_remainder_window(",
        "fn reclaim_on_nondelivery(",
        "fn confirm_delivery(",
        "fn settle(",
        "fn resolve_fx_shortfall(",
        "fn fund_fx_shortfall(",
        "fn expire_fx_shortfall_window(",
        "fn cancel(",
        "fn reassign_buyer(",
        "fn flag_dispute(",
        "fn resolve_dispute(",
        "fn expire_dispute_window(",
    ];
    for fn_sig in direct_gated {
        let pos = SRC
            .find(fn_sig)
            .unwrap_or_else(|| panic!("function `{fn_sig}` not found in src/lib.rs"));
        // Scan 1500 chars — large enough for even the longest function body.
        let window = &SRC[pos..(pos + 1500).min(SRC.len())];
        assert!(
            has_status_guard(SRC, window),
            "`{fn_sig}` must have an InvalidState guard in its body \
             (authorization-matrix.md §4: every function picks a status shape)"
        );
    }

    // release_advance_1/2 delegate to `open_tranche` which holds the guard —
    // verify the delegation (and that open_tranche has the guard) rather than
    // scanning the one-line wrapper bodies.
    for fn_sig in ["fn release_advance_1(", "fn release_advance_2("] {
        let pos = SRC
            .find(fn_sig)
            .unwrap_or_else(|| panic!("{fn_sig} not found"));
        let window = &SRC[pos..(pos + 200).min(SRC.len())];
        assert!(
            window.contains("open_tranche"),
            "`{fn_sig}` must delegate to open_tranche for its status guard"
        );
    }
    let pos = SRC
        .find("fn open_tranche(")
        .expect("open_tranche not found");
    assert!(
        has_status_guard(SRC, &SRC[pos..(pos + 300).min(SRC.len())]),
        "open_tranche must have an InvalidState guard (used by release_advance_1/2)"
    );
}
