//! Property-based coverage for every place this contract adds a window
//! length to a ledger timestamp: the resulting deadline must never
//! overflow `u64`.
//!
//! The four in-scope addition sites, confirmed against `src/lib.rs`:
//!
//! - `initialize`: `delivery_deadline = created_at + delivery_window_secs`
//!   (the only addition computed from `initialize`'s own arguments; the
//!   claim/remainder windows are stored untouched here and only added to
//!   a timestamp later, at their own call sites).
//! - `open_tranche` (via `release_advance_1`/`release_advance_2`):
//!   `advance{1,2}_deadline = now + claim_window_secs`, one shared code
//!   path — both tranches are exercised, since each writes its own
//!   deadline *field* even though the arithmetic is one expression.
//! - `ready_for_delivery`: `remainder_deadline = now + remainder_window_secs`.
//! - `flag_dispute`: `dispute_deadline = now + remainder_window_secs`
//!   (the documented reuse of the remainder window as the dispute
//!   freeze's bound).
//!
//! Out of scope, deliberately: `resolve_fx_shortfall`'s
//! `fx_shortfall_deadline = now + remainder_window_secs` is the
//! identical expression but only reachable on an oracle-configured
//! commitment — the oracle plumbing belongs to
//! `oracle_fault_injection.rs`, and the arithmetic shape itself is what
//! gets pinned here.
//!
//! **The property.** For arbitrary ledger timestamps and arbitrary
//! window lengths — including deliberately adversarial values at and
//! around `u64::MAX`, which uniform `any::<u64>()` generation almost
//! never produces on its own — each site must do exactly one of two
//! things:
//!
//! 1. store the *exact* sum (computed here independently in `u128`, so
//!    the oracle itself can't overflow), or
//! 2. refuse atomically: the call aborts, storage is unchanged, and no
//!    deadline is recorded at all.
//!
//! What must never happen is the silent wrap: a stored deadline *less*
//! than the timestamp it came from (a window that instantly looks
//! expired). The assertions are written so they catch that even if
//! `overflow-checks` were ever turned off in the test profile — success
//! is only accepted when the stored value equals the exact `u128`-
//! computed sum.
//!
//! **Why a host abort is the expected overflow behavior, and why it
//! matches production.** The contract adds with a plain `+` on purpose:
//! the workspace `release` profile sets `overflow-checks = true`, so the
//! deployed wasm *traps* on an overflowing window rather than wrapping —
//! Soroban invocation atomicity then discards the whole call. In these
//! host-native tests the same overflow panics, the env catches the panic
//! and translates it into a host error (the `Err(Err(_))` shape from a
//! `try_*` client — the same category the SDK documents for crashing
//! callees), so the test sees exactly the trap-shaped refusal the chain
//! would produce. `catch_unwind` is layered on top the same way
//! `oracle_fault_injection.rs` does it, so even a raw unwinding panic —
//! a shape an SDK version bump could produce instead of the clean host
//! error — is treated as a refusal rather than crashing the property
//! run.
//!
//! **Finding — nothing caught; overflow safety confirmed at all four
//! sites.** The generated properties (plus the pinned `u64::MAX` edge
//! cases at the bottom, which cover the exact-boundary inputs random
//! generation only hits by luck) confirm: every in-scope addition
//! either records the exact sum or aborts atomically, and the largest
//! legal deadline (`0 + u64::MAX`, or `t + w` landing exactly on
//! `u64::MAX`) is accepted as-is. One deliberate behavior worth knowing
//! about, not a bug: the contract places **no upper bound** on window
//! lengths (only `> 0`), so a window that doesn't quite overflow — e.g.
//! a ledger timestamp of 1 second with a `u64::MAX - 5` second window —
//! is accepted and produces a deadline billions of years out. That's
//! intentional (window sizing is business judgment, enforced at the API
//! layer; the contract stays unbounded on purpose — see HANDOFF.md's
//! "Next steps" item 5); this suite pins that such inputs are stored
//! *exactly*, never wrapped.

#[path = "common/mod.rs"]
mod common;

use common::{DELIVERY_WINDOW, GRADE_PRICE_BPS, TOTAL_AMOUNT, WINDOW};
use harvestlock_escrow::{Error, EscrowContract, EscrowContractClient, Status};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::{token, Address, ConversionError, Env, InvokeError, Vec};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------- strategies ----------

/// Ledger timestamps an adversarial initiator could actually pick:
/// zero, realistic epoch seconds, the whole `u64` range, and — the
/// interesting part — heavy mass in the last million seconds of the
/// type, where `timestamp + window` is most eager to overflow.
fn arb_timestamp() -> impl Strategy<Value = u64> {
    prop_oneof![
        2 => Just(0u64),
        3 => 1u64..1_000_000_000_000,           // realistic epoch seconds
        3 => any::<u64>(),                      // the full range
        4 => (u64::MAX - 1_000_000)..=u64::MAX, // near-u64::MAX
    ]
}

/// Window lengths, always > 0 (`initialize` rejects 0 with
/// `InvalidWindow` before any addition runs), with deliberate mass near
/// `u64::MAX` — the "large-but-valid inputs nobody writes by hand" this
/// suite exists for.
fn arb_window() -> impl Strategy<Value = u64> {
    prop_oneof![
        3 => 1u64..(365 * 24 * 60 * 60),        // plausible business windows
        3 => any::<u64>().prop_map(|w| w.max(1)), // full range, nonzero
        1 => u64::MAX / 2..=u64::MAX,           // upper half of the type
        4 => (u64::MAX - 1_000_000)..=u64::MAX, // near-u64::MAX
        1 => Just(u64::MAX),
    ]
}

/// The deadline `timestamp + window` must produce, computed in `u128`
/// so the oracle itself can never overflow — `None` exactly when the
/// `u64` addition in the contract *must* overflow.
fn exact_deadline(timestamp: u64, window: u64) -> Option<u64> {
    u64::try_from(timestamp as u128 + window as u128).ok()
}

// ---------- harness ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// The call returned `Ok` — whatever deadline it stored must be exact.
    Accepted,
    /// The call returned a contract-level `Error` (the `Err(Ok(_))`
    /// shape every other suite asserts against).
    ContractRefusal,
    /// The call failed at the host level — the trap-shaped overflow
    /// refusal (or an SDK-version unwind, caught below).
    Aborted,
}

/// Classifies a `try_*` client result without naming its full generic
/// shape: `Ok(Ok(()))` is success, `Err(Ok(_))` a contract `Error`,
/// `Err(Err(_))` a host error (the trap), and `Ok(Err(_))` a
/// return-value conversion failure (impossible for `()`; treated as a
/// host failure to keep the match exhaustive).
fn classify(r: Result<Result<(), ConversionError>, Result<Error, InvokeError>>) -> Outcome {
    match r {
        Ok(Ok(())) => Outcome::Accepted,
        Err(Ok(_)) => Outcome::ContractRefusal,
        Err(Err(_)) | Ok(Err(_)) => Outcome::Aborted,
    }
}

/// Runs one deadline-opening call the way the SDK itself guards crashing
/// callees (`oracle_fault_injection.rs`'s `attempt`): a raw unwinding
/// panic — a shape an SDK version bump could produce instead of the
/// clean host error — counts as a refusal, never as a success.
fn attempt(f: impl FnOnce() -> Outcome) -> Outcome {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Outcome::Aborted)
}

/// The one acceptance rule every property shares: an `Accepted` call's
/// stored deadline must be the exact `u128`-computed sum (and so never
/// less than the timestamp it came from — a wrapped value would be),
/// and a call must only ever be accepted when the addition doesn't
/// overflow. `None` + `Accepted` *is* the silent-wrap bug this suite
/// guards against.
fn verify_outcome(
    outcome: Outcome,
    expected: Option<u64>,
    timestamp: u64,
    window: u64,
    site: &str,
) -> Result<(), TestCaseError> {
    match (outcome, expected) {
        (Outcome::Accepted, Some(expected)) => {
            prop_assert!(
                expected >= timestamp,
                "{site}: exact deadline below its own timestamp is impossible without wrap"
            );
        }
        (Outcome::Accepted, None) => panic!(
            "{site}: accepted and stored a deadline that overflows u64 \
             (timestamp {timestamp}, window {window}) — silent wrap"
        ),
        (Outcome::ContractRefusal | Outcome::Aborted, Some(expected)) => panic!(
            "{site}: refused an addition that does not overflow u64 \
             (timestamp {timestamp}, window {window}, exact deadline {expected})"
        ),
        (Outcome::ContractRefusal | Outcome::Aborted, None) => {}
    }
    Ok(())
}

/// A registered escrow instance plus its parties, with any windows the
/// test wants — the same shape as `common::World`, which pins its
/// windows to constants and so can't carry generated ones.
struct Case {
    env: Env,
    contract: EscrowContractClient<'static>,
    token_address: Address,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
}

fn case() -> Case {
    // Snapshot-on-drop disabled on purpose: the SDK's default would write
    // one `test_snapshots/*.json` file *per generated proptest case* (the
    // committed snapshots under `test_snapshots/` pin the observable
    // behavior of named example tests; thousands of per-case files for
    // generated inputs would be noise, and the assertions below are this
    // suite's actual behavior pin).
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &TOTAL_AMOUNT);
    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    Case {
        env,
        contract,
        token_address,
        buyer,
        cooperative,
        warehouse,
    }
}

impl Case {
    /// `initialize` at ledger timestamp `t` with the given windows.
    /// Claim and remainder windows are arguments here but only *added*
    /// to a timestamp at their own later call sites — see the module
    /// docs for why only `delivery_window_secs` affects this call's own
    /// arithmetic.
    fn initialize_at(&self, t: u64, claim: u64, remainder: u64, delivery: u64) {
        self.env.ledger().set_timestamp(t);
        self.contract.initialize(
            &self.buyer,
            &self.cooperative,
            &self.warehouse,
            &self.token_address,
            &TOTAL_AMOUNT,
            &1_500,
            &2_000,
            &claim,
            &remainder,
            &delivery,
            &1_000,
            &Vec::from_array(&self.env, GRADE_PRICE_BPS),
            &None,
        );
    }

    /// Locked, tranche 1 opened and claimed, checkpoint marked — the
    /// state `release_advance_2` requires. Always at timestamp 0, so the
    /// fixed (small) windows used along the way can't overflow.
    fn drive_to_checkpoint_passed(&self) {
        self.contract.lock();
        self.contract.release_advance_1();
        self.contract.claim_advance_1();
        self.contract.mark_checkpoint();
    }

    /// Straight through to `Advance2Released` with both tranches claimed
    /// — the state `ready_for_delivery` requires. Always at timestamp 0.
    fn drive_to_advance2_released(&self) {
        self.drive_to_checkpoint_passed();
        self.contract.release_advance_2();
        self.contract.claim_advance_2();
    }
}

// ---------- the four properties ----------

proptest! {
    /// `delivery_window_secs + created_at` at `initialize`: the
    /// commitment is created with the exact deadline, or not at all.
    #[test]
    fn delivery_deadline_is_the_exact_sum_or_nothing_is_created(
        t in arb_timestamp(),
        w in arb_window(),
    ) {
        let c = case();
        c.env.ledger().set_timestamp(t);
        let outcome = attempt(|| {
            classify(c.contract.try_initialize(
                &c.buyer,
                &c.cooperative,
                &c.warehouse,
                &c.token_address,
                &TOTAL_AMOUNT,
                &1_500,
                &2_000,
                &WINDOW,
                &WINDOW,
                &w,
                &1_000,
                &Vec::from_array(&c.env, GRADE_PRICE_BPS),
                &None,
            ))
        });
        verify_outcome(outcome, exact_deadline(t, w), t, w, "initialize/delivery_window")?;

        match outcome {
            Outcome::Accepted => {
                let st = c.contract.get_commitment();
                prop_assert_eq!(st.created_at, t);
                prop_assert_eq!(st.delivery_deadline, t + w);
                prop_assert!(st.delivery_deadline >= st.created_at);
                prop_assert_eq!(st.status, Status::Draft);
            }
            Outcome::ContractRefusal | Outcome::Aborted => {
                // Atomic: an overflowing window must leave no commitment behind.
                prop_assert!(
                    matches!(
                        c.contract.try_get_commitment(),
                        Err(Ok(Error::NotInitialized))
                    ),
                    "initialize's abort must leave no commitment behind"
                );
            }
        }
    }

    /// `claim_window_secs + now` at `open_tranche`, both tranches: the
    /// deadline field is set to the exact sum, or nothing changes.
    #[test]
    fn tranche_deadline_is_the_exact_sum_or_nothing_changes(
        t in arb_timestamp(),
        w in arb_window(),
        tranche2 in any::<bool>(),
    ) {
        let c = case();
        c.initialize_at(0, w, WINDOW, DELIVERY_WINDOW);
        let status_before = if tranche2 {
            c.drive_to_checkpoint_passed();
            Status::CheckpointPassed
        } else {
            // `release_advance_1` itself is the call under test here — the
            // only precondition is `Locked`.
            c.contract.lock();
            Status::Locked
        };
        c.env.ledger().set_timestamp(t);

        let outcome = attempt(|| {
            if tranche2 {
                classify(c.contract.try_release_advance_2())
            } else {
                classify(c.contract.try_release_advance_1())
            }
        });
        let site = if tranche2 {
            "release_advance_2/claim_window"
        } else {
            "release_advance_1/claim_window"
        };
        verify_outcome(outcome, exact_deadline(t, w), t, w, site)?;

        match outcome {
            Outcome::Accepted => {
                let st = c.contract.get_commitment();
                if tranche2 {
                    prop_assert_eq!(st.advance2_deadline, t + w);
                    prop_assert_eq!(st.status, Status::Advance2Released);
                    // Tranche 1 opened at timestamp 0 with the same shared
                    // claim window — untouched by tranche 2's release.
                    prop_assert_eq!(st.advance1_deadline, w);
                } else {
                    prop_assert_eq!(st.advance1_deadline, t + w);
                    prop_assert_eq!(st.status, Status::Advance1Released);
                    prop_assert_eq!(st.advance2_deadline, 0);
                }
                prop_assert!(st.advance1_deadline.max(st.advance2_deadline) >= t);
            }
            Outcome::ContractRefusal | Outcome::Aborted => {
                let st = c.contract.get_commitment();
                prop_assert_eq!(st.status, status_before);
                if tranche2 {
                    prop_assert_eq!(st.advance2_deadline, 0);
                } else {
                    prop_assert_eq!(st.advance1_deadline, 0);
                }
            }
        }
    }

    /// `remainder_window_secs + now` at `ready_for_delivery`: the
    /// remainder deadline is the exact sum, or nothing changes.
    #[test]
    fn remainder_deadline_is_the_exact_sum_or_nothing_changes(
        t in arb_timestamp(),
        w in arb_window(),
    ) {
        let c = case();
        c.initialize_at(0, WINDOW, w, DELIVERY_WINDOW);
        c.drive_to_advance2_released();
        c.env.ledger().set_timestamp(t);

        let outcome =
            attempt(|| classify(c.contract.try_ready_for_delivery()));
        verify_outcome(outcome, exact_deadline(t, w), t, w, "ready_for_delivery/remainder_window")?;

        match outcome {
            Outcome::Accepted => {
                let st = c.contract.get_commitment();
                prop_assert_eq!(st.remainder_deadline, t + w);
                prop_assert!(st.remainder_deadline >= t);
                prop_assert_eq!(st.status, Status::ReadyForDelivery);
            }
            Outcome::ContractRefusal | Outcome::Aborted => {
                let st = c.contract.get_commitment();
                prop_assert_eq!(st.status, Status::Advance2Released);
                prop_assert_eq!(st.remainder_deadline, 0);
            }
        }
    }

    /// `remainder_window_secs + now` (the documented window reuse) at
    /// `flag_dispute`: the dispute deadline is the exact sum, or
    /// nothing changes.
    #[test]
    fn dispute_deadline_is_the_exact_sum_or_nothing_changes(
        t in arb_timestamp(),
        w in arb_window(),
    ) {
        let c = case();
        c.initialize_at(0, WINDOW, w, DELIVERY_WINDOW);
        c.contract.lock();
        c.env.ledger().set_timestamp(t);

        let outcome = attempt(|| classify(c.contract.try_flag_dispute(&c.buyer)));
        verify_outcome(outcome, exact_deadline(t, w), t, w, "flag_dispute/remainder_window")?;

        match outcome {
            Outcome::Accepted => {
                let st = c.contract.get_commitment();
                prop_assert_eq!(st.dispute_deadline, t + w);
                prop_assert!(st.dispute_deadline >= t);
                prop_assert_eq!(st.status, Status::Disputed);
                prop_assert_eq!(st.dispute_pre_status, Status::Locked);
            }
            Outcome::ContractRefusal | Outcome::Aborted => {
                let st = c.contract.get_commitment();
                prop_assert_eq!(st.status, Status::Locked);
                prop_assert_eq!(st.dispute_deadline, 0);
                prop_assert_eq!(st.dispute_pre_status, Status::Draft);
            }
        }
    }
}

// ---------- pinned u64::MAX edge cases ----------

// Random generation finds the exact boundary by luck; these pin the
// inputs closest to it deterministically, per site.

#[test]
fn initialize_accepts_the_largest_legal_delivery_deadline() {
    // 0 + u64::MAX is exact — the biggest deadline u64 can hold.
    let c = case();
    c.initialize_at(0, WINDOW, WINDOW, u64::MAX);
    assert_eq!(c.contract.get_commitment().delivery_deadline, u64::MAX);
}

#[test]
fn initialize_accepts_a_deadline_landing_exactly_on_u64_max() {
    let c = case();
    c.initialize_at(u64::MAX - 500, WINDOW, WINDOW, 500);
    assert_eq!(c.contract.get_commitment().delivery_deadline, u64::MAX);
}

#[test]
fn initialize_refuses_every_one_second_overflow() {
    // t = u64::MAX with a 1-second window, t = 1 with a u64::MAX window,
    // and both extremes at once — each addition is off by the smallest
    // possible margin (or the largest), and none may be recorded.
    for (t, w) in [(u64::MAX, 1), (1, u64::MAX), (u64::MAX, u64::MAX)] {
        let c = case();
        c.env.ledger().set_timestamp(t);
        let outcome = attempt(|| {
            classify(c.contract.try_initialize(
                &c.buyer,
                &c.cooperative,
                &c.warehouse,
                &c.token_address,
                &TOTAL_AMOUNT,
                &1_500,
                &2_000,
                &WINDOW,
                &WINDOW,
                &w,
                &1_000,
                &Vec::from_array(&c.env, GRADE_PRICE_BPS),
                &None,
            ))
        });
        assert!(
            matches!(outcome, Outcome::ContractRefusal | Outcome::Aborted),
            "t={t} w={w}: an overflowing delivery window must abort, got {outcome:?}"
        );
        assert!(
            matches!(
                c.contract.try_get_commitment(),
                Err(Ok(Error::NotInitialized))
            ),
            "t={t} w={w}: abort must leave no commitment behind"
        );
    }
}

#[test]
fn tranche_deadline_pinned_boundary() {
    // Exactly u64::MAX is stored; one second past it aborts atomically.
    let ok = case();
    ok.initialize_at(0, 1, WINDOW, DELIVERY_WINDOW);
    ok.contract.lock();
    ok.env.ledger().set_timestamp(u64::MAX - 1);
    ok.contract.release_advance_1();
    assert_eq!(ok.contract.get_commitment().advance1_deadline, u64::MAX);

    let bad = case();
    bad.initialize_at(0, 1, WINDOW, DELIVERY_WINDOW);
    bad.contract.lock();
    bad.env.ledger().set_timestamp(u64::MAX);
    assert!(matches!(
        attempt(|| classify(bad.contract.try_release_advance_1())),
        Outcome::ContractRefusal | Outcome::Aborted
    ));
    let st = bad.contract.get_commitment();
    assert_eq!(st.advance1_deadline, 0);
    assert_eq!(st.status, Status::Locked);
}

#[test]
fn remainder_deadline_pinned_boundary() {
    let ok = case();
    ok.initialize_at(0, WINDOW, 1, DELIVERY_WINDOW);
    ok.drive_to_advance2_released();
    ok.env.ledger().set_timestamp(u64::MAX - 1);
    ok.contract.ready_for_delivery();
    assert_eq!(ok.contract.get_commitment().remainder_deadline, u64::MAX);

    let bad = case();
    bad.initialize_at(0, WINDOW, 1, DELIVERY_WINDOW);
    bad.drive_to_advance2_released();
    bad.env.ledger().set_timestamp(u64::MAX);
    assert!(matches!(
        attempt(|| classify(bad.contract.try_ready_for_delivery())),
        Outcome::ContractRefusal | Outcome::Aborted
    ));
    let st = bad.contract.get_commitment();
    assert_eq!(st.remainder_deadline, 0);
    assert_eq!(st.status, Status::Advance2Released);
}

#[test]
fn dispute_deadline_pinned_boundary() {
    let ok = case();
    ok.initialize_at(0, WINDOW, 1, DELIVERY_WINDOW);
    ok.contract.lock();
    ok.env.ledger().set_timestamp(u64::MAX - 1);
    ok.contract.flag_dispute(&ok.buyer.clone());
    assert_eq!(ok.contract.get_commitment().dispute_deadline, u64::MAX);

    let bad = case();
    bad.initialize_at(0, WINDOW, 1, DELIVERY_WINDOW);
    bad.contract.lock();
    bad.env.ledger().set_timestamp(u64::MAX);
    assert!(matches!(
        attempt(|| classify(bad.contract.try_flag_dispute(&bad.buyer))),
        Outcome::ContractRefusal | Outcome::Aborted
    ));
    let st = bad.contract.get_commitment();
    assert_eq!(st.dispute_deadline, 0);
    assert_eq!(st.status, Status::Locked);
}
