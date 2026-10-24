//! #2 — regression suite for the recorded security review of the two
//! permissionless tranche-opening calls (`release_advance_1` /
//! `release_advance_2`).
//!
//! The review's full reasoning is in `docs/permissionless-release-review.md`.
//! This suite exists so its conclusions can't quietly go stale: every claim
//! that review made on paper is asserted here against the actual build.
//!
//! What is pinned, and why each one matters to the review's verdict:
//!
//! 1. **Both calls succeed under a stranger-only auth set.** `mock_all_auths`
//!    (used by the shared fixture during setup) can satisfy a `require_auth()`
//!    that shouldn't exist, so it cannot prove *absence* of an auth gate.
//!    `mock_auths(&[stranger])` can: every `require_auth()` except the
//!    stranger's own exact invocation would fail, so the call succeeding
//!    proves no signature at all is required — the pattern already used by
//!    `expire_remainder_window_works_when_only_a_third_party_signs`.
//! 2. **Opening moves no funds.** The whole "doesn't move money" claim, at
//!    the balance level, for both tranches.
//! 3. **The clock is written exactly once.** A second open attempt fails
//!    `InvalidState` and the recorded deadline is untouched — no restart, no
//!    extension, no shopping for a better instant.
//! 4. **An early third-party open still leaves the cooperative its full
//!    agreed window.** The deadline is `open time + claim_window_secs`
//!    regardless of who opened, and a claim landing in the deadline second
//!    (the last admissible instant) still pays in full.
//! 5. **Opening a clock empowers nobody.** The claim it enables stays
//!    cooperative-auth-gated and the reclaim it can end in stays
//!    buyer-auth-gated, even when the caller of the open was a stranger —
//!    proven with a stranger-only auth set, not assumed under
//!    `mock_all_auths`.
//! 6. **No clock can be started during a dispute freeze**, and the state
//!    machine resumes exactly where it left off afterwards.
//! 7. **The end-to-end griefing scenario from the issue**: a stranger races
//!    every eligible transition, both windows lapse unclaimed, and every
//!    stroop ends with the party the recorded terms entitle — the stranger
//!    never holds, pays, or redirects anything.
//!
//! Scope note: this suite covers *only* what issue #2 asked (the two
//! permissionless opens and their downstream effects). The tranche
//! claim/reclaim boundary arithmetic itself is pinned separately by
//! `tests/deadline_boundary.rs` (#111); the wider "permissionless call can
//! only complete a pre-fixed fact" argument lives in
//! `docs/authorization-matrix.md` §2 and `docs/threat-model.md` entry 21.

#[path = "common/mod.rs"]
mod common;

use common::{world, World};
use harvestlock_escrow::{Error, Status};
use soroban_sdk::testutils::{Address as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{Address, IntoVal, Val, Vec as OnChainVec};

/// Installs a stranger-only authorization set for one exact invocation —
/// every `require_auth()` that is not that invocation's own fails as a host
/// error. Copied shape-for-shape from `src/test.rs`'s `mock_invoke` (the
/// in-crate helper isn't visible from an integration target).
fn mock_invoke<'a>(
    contract: &'a Address,
    fn_name: &'a str,
    args: OnChainVec<Val>,
) -> MockAuthInvoke<'a> {
    MockAuthInvoke {
        contract,
        fn_name,
        args,
        sub_invokes: &[],
    }
}

fn mock_stranger_only(w: &World, fn_name: &str, stranger: &Address) {
    let invoke = mock_invoke(&w.contract.address, fn_name, ().into_val(&w.env));
    w.env.mock_auths(&[MockAuth {
        address: stranger,
        invoke: &invoke,
    }]);
}

// ---------- 1. permissionless, genuinely ----------

#[test]
fn release_advance_1_requires_no_signature_even_from_a_stranger() {
    let s = world(1_500, 2_000);
    s.contract.lock();

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_1", &stranger);

    s.contract.release_advance_1();
    assert_eq!(s.status(), Status::Advance1Released);
}

#[test]
fn release_advance_2_requires_no_signature_even_from_a_stranger() {
    let s = world(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_2", &stranger);

    s.contract.release_advance_2();
    assert_eq!(s.status(), Status::Advance2Released);
}

// ---------- 2. opening moves no funds ----------

#[test]
fn opening_a_tranche_moves_no_funds_for_either_tranche() {
    let s = world(1_500, 2_000);
    s.contract.lock();
    let deposit = s.bps(1_500 + 2_000);
    assert_eq!(s.token.balance(&s.contract.address), deposit);
    let buyer_before = s.token.balance(&s.buyer);

    s.contract.release_advance_1();
    assert_eq!(s.token.balance(&s.contract.address), deposit);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.buyer), buyer_before);

    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    assert_eq!(s.token.balance(&s.contract.address), deposit);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.buyer), buyer_before);
}

// ---------- 3. one clock per tranche, written once ----------

#[test]
fn the_clock_cannot_be_restarted_or_extended_once_open() {
    let s = world(1_500, 2_000);
    s.contract.lock();

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_1", &stranger);
    s.contract.release_advance_1();
    let deadline = s.contract.get_commitment().advance1_deadline;
    assert_eq!(deadline, s.now() + common::WINDOW);

    // Even the same stranger calling again — the window can never be
    // rewritten, so no party (adversarial or not) can shop for a better
    // opening instant by calling repeatedly.
    mock_stranger_only(&s, "release_advance_1", &stranger);
    assert_eq!(
        s.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(s.contract.get_commitment().advance1_deadline, deadline);
    assert_eq!(s.status(), Status::Advance1Released);

    // Same property for tranche 2.
    s.env.mock_all_auths(); // the operator's gated checkpoint
    s.contract.mark_checkpoint();
    mock_stranger_only(&s, "release_advance_2", &stranger);
    s.contract.release_advance_2();
    let deadline2 = s.contract.get_commitment().advance2_deadline;

    mock_stranger_only(&s, "release_advance_2", &stranger);
    assert_eq!(
        s.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(s.contract.get_commitment().advance2_deadline, deadline2);
}

// ---------- 4. the full agreed window survives an early open ----------

#[test]
fn a_stranger_opening_early_still_leaves_the_cooperative_its_full_agreed_window() {
    // The issue's griefing scenario, at its narrowest: a third party opens
    // the clock the moment it becomes eligible. The cooperative's window is
    // still the full `claim_window_secs` from *that* instant — and claiming
    // in the deadline second itself (the last admissible instant, #111) is
    // still inclusive and still pays in full.
    let s = world(1_500, 2_000);
    s.contract.lock();

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_1", &stranger);
    s.contract.release_advance_1();

    let deadline = s.contract.get_commitment().advance1_deadline;
    assert_eq!(
        deadline,
        s.now() + common::WINDOW,
        "an early third-party open must not shorten the agreed window"
    );

    s.env.mock_all_auths(); // cooperative's own signature from here on
    s.set_time(deadline); // the very last claimable instant
    s.contract.claim_advance_1();
    assert!(s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.cooperative), s.bps(1_500));
}

// ---------- 5. an opened clock empowers nobody ----------

#[test]
fn a_stranger_cannot_claim_a_window_it_opened() {
    let s = world(1_500, 2_000);
    s.contract.lock();

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_1", &stranger);
    s.contract.release_advance_1();

    // Stranger tries to collect the tranche whose clock they started: the
    // cooperative's signature is the only one that can, so this must abort
    // as a host auth error with nothing written and nothing paid.
    let contract_before = s.token.balance(&s.contract.address);
    mock_stranger_only(&s, "claim_advance_1", &stranger);
    let result = s.contract.try_claim_advance_1();
    assert!(
        matches!(result, Err(Err(_))),
        "a window opened by a stranger must still be the cooperative's to claim: {result:?}"
    );
    assert!(!s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.contract.address), contract_before);
    assert_eq!(s.token.balance(&stranger), 0);

    // The one signature the gate actually demands, from the same state.
    s.env.mock_all_auths();
    s.contract.claim_advance_1();
    assert!(s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.cooperative), s.bps(1_500));
}

#[test]
fn a_stranger_cannot_reclaim_a_window_it_opened() {
    let s = world(1_500, 2_000);
    s.contract.lock();

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_1", &stranger);
    s.contract.release_advance_1();
    s.advance(common::WINDOW + 1); // window lapsed unclaimed

    // The lapse-path beneficiary is the buyer and only the buyer — a
    // stranger who forced the clock open cannot route the refund to
    // themselves.
    mock_stranger_only(&s, "reclaim_advance_1", &stranger);
    let result = s.contract.try_reclaim_advance_1();
    assert!(
        matches!(result, Err(Err(_))),
        "a reclaim opened by a stranger's clock must still be the buyer's to take: {result:?}"
    );
    assert!(!s.contract.get_commitment().advance1_expired);
    assert_eq!(s.token.balance(&stranger), 0);

    s.env.mock_all_auths();
    s.contract.reclaim_advance_1();
    assert!(s.contract.get_commitment().advance1_expired);
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - s.bps(3_500) + s.bps(1_500)
    );
    assert_eq!(s.token.balance(&stranger), 0);
}

// ---------- 6. the dispute freeze also blocks the clock ----------

#[test]
fn no_clock_can_be_started_during_a_dispute_freeze() {
    let s = world(1_500, 2_000);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);
    assert_eq!(s.status(), Status::Disputed);

    let stranger = Address::generate(&s.env);
    mock_stranger_only(&s, "release_advance_1", &stranger);
    assert_eq!(
        s.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState)),
        "Disputed is never a required status — nobody starts a clock mid-freeze"
    );
    assert_eq!(s.contract.get_commitment().advance1_deadline, 0);

    // The freeze lifts on its documented timeout and the machine resumes
    // exactly where it stopped — `Locked`, clock still unopened.
    s.advance(common::REMAINDER_WINDOW + 1);
    s.env.mock_all_auths();
    s.contract.expire_dispute_window();
    assert_eq!(s.status(), Status::Locked);
    s.contract.release_advance_1();
    assert_eq!(s.status(), Status::Advance1Released);
}

// ---------- 7. out-of-sequence states refuse the clock ----------

#[test]
fn out_of_sequence_states_refuse_the_clock() {
    let s = world(1_500, 2_000);

    // Before lock — nothing escrowed, no claim window to open.
    assert_eq!(
        s.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
    // Tranche 2 cannot be raced past the operator's checkpoint.
    s.contract.lock();
    assert_eq!(
        s.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );

    // Cancelled is terminal for both — mutual unwind already returned
    // everything; a clock started afterwards could never be honored.
    s.contract.cancel();
    assert_eq!(s.status(), Status::Cancelled);
    assert_eq!(
        s.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        s.contract.try_release_advance_2(),
        Err(Ok(Error::InvalidState))
    );
}

// ---------- 8. the issue's end-to-end griefing scenario ----------

#[test]
fn the_full_griefing_scenario_pays_only_the_entitled_parties() {
    // Issue #2's worst case, played to the end: an adversarial stranger
    // races *every* permissionless transition the moment it's eligible —
    // opens tranche 1 at lock-in, opens tranche 2 right after the operator's
    // checkpoint, and is present at every later step. Both windows lapse
    // unclaimed (the cooperative is "unavailable", the scenario's premise).
    // Verdict under test: the stranger ends holding nothing, every state
    // change matched the recorded terms, and the total token supply is
    // conserved across exactly the parties the terms name.
    let s = world(1_500, 2_000);
    let stranger = Address::generate(&s.env);

    s.contract.lock();
    let deposit = s.bps(1_500 + 2_000);
    assert_eq!(s.token.balance(&s.contract.address), deposit);

    // Stranger races tranche 1's clock the moment it's eligible.
    mock_stranger_only(&s, "release_advance_1", &stranger);
    s.contract.release_advance_1();
    assert_eq!(s.token.balance(&stranger), 0);

    // The operator's checkpoint is a judgment call and stays gated — the
    // stranger cannot pull it forward — but the tranche 2 clock the
    // checkpoint unlocks is again raced instantly.
    s.env.mock_all_auths();
    s.contract.mark_checkpoint();
    mock_stranger_only(&s, "release_advance_2", &stranger);
    s.contract.release_advance_2();
    assert_eq!(s.token.balance(&stranger), 0);

    // "Cooperative unavailable": both windows lapse unclaimed. Only the
    // buyer can reclaim, and does — the deposit comes home, not to the
    // stranger, not to the cooperative.
    s.advance(common::WINDOW + 1);
    s.env.mock_all_auths();
    s.contract.reclaim_advance_1();
    s.contract.reclaim_advance_2();
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);

    // The deal still completes on its own terms afterwards.
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract
        .confirm_delivery(&common::CONTRACTED_QUANTITY, &0);
    s.contract.settle();
    assert_eq!(s.status(), Status::Settled);

    // No stroop ever touched the stranger; the full total is conserved.
    assert_eq!(s.token.balance(&stranger), 0);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(
        s.token.balance(&s.buyer) + s.token.balance(&s.cooperative) + s.token.balance(&stranger),
        s.total_amount
    );
}
