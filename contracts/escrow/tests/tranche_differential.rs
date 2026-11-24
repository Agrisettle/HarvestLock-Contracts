//! #269 — differential property: tranche one and tranche two obey one rule.
//!
//! `open_tranche`, `claim_tranche` and `reclaim_tranche` are shared code,
//! parameterized by the private `Tranche` enum, "so the claim/reclaim rules
//! can't drift between the two tranches". That sharing is only as good as the
//! per-tranche accessors it dispatches through: `tranche_bps`,
//! `tranche_deadline`, `tranche_claimed`, `tranche_expired`,
//! `tranche_next_status`, `set_tranche_claimed`, `set_tranche_expired` and the
//! three `Method` rows. A copy-paste slip there — setting `advance1_expired`
//! for `Tranche::Two`, returning `advance1_bps` for `Tranche::Two`, writing
//! `Advance1Released` from tranche 2's open — is invisible to a suite where
//! every test exercises one tranche in isolation.
//!
//! **The property.** For random bps splits, claim windows, open timestamps and
//! attempt timings, the *same* sequence of open / claim / reclaim / no-op
//! operations runs against tranche 1 (from `Locked`, the only state
//! `release_advance_1` accepts) and tranche 2 (from `CheckpointPassed`, the
//! only state `release_advance_2` accepts). Both open at the same timestamp
//! with the same window, so a correct contract must produce the same
//! observations from each: the deadline the open wrote, whether the attempt
//! was accepted or which `Error` refused it — including which error wins when
//! several guards fail — the claimed/expired flags it set, and that the status
//! it left behind is the tranche's own released variant.
//!
//! The one intended difference is the *amount*, because the two tranches are
//! funded by different bps splits. Each run therefore asserts its own transfer
//! against its own split (`TOTAL * advance{1,2}_bps / 10_000`) and against the
//! party the action names, rather than comparing two equal numbers. A
//! `tranche_bps` (or `set_tranche_*`) slip shows up either as a wrong amount
//! or as a divergent rule outcome.
//!
//! Attempt timings are expressed relative to the deadline — `deadline + delta`
//! for `delta` in `-3..=3` — so the interesting cases (inside the cooperative's
//! window, the deadline second itself, the buyer's first second after it) are
//! generated often rather than only by luck. `tests/deadline_boundary.rs` pins
//! those same boundaries by hand for each tranche; this suite is what proves
//! the two tranches agree with each other.

use harvestlock_escrow::{CommitmentTerms, Error, EscrowContract, EscrowContractClient, Status};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
// `Vec` is the standard library's here: the observations collected per run
// are compared and formatted, and soroban's `Vec` only makes sense inside a
// contract call. The one on-chain vector in this file is named explicitly.
use soroban_sdk::{token, Address, Env, Vec as OnChainVec};

const TOTAL: i128 = 1_000_000;
const QUANTITY: u32 = 1_000;
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
const FULL_PRICE_BPS: [u32; 1] = [10_000];

// ---------- strategies ----------

/// Which of the contract's two advance tranches a run is driving. The
/// contract's own `Tranche` enum is private; this is the test's mirror of it,
/// and it is only ever used to pick the `_1`/`_2` entry points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tranche {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Claim,
    Reclaim,
    /// No call at all, so a plan can isolate the other attempt's effect.
    None,
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        Just(Action::Claim),
        Just(Action::Reclaim),
        Just(Action::None),
    ]
}

/// One random plan, applied unchanged to both tranches.
#[derive(Clone, Copy, Debug)]
struct Plan {
    advance1_bps: u32,
    advance2_bps: u32,
    claim_window_secs: u64,
    opened_at: u64,
    first: Action,
    /// `attempt_time - deadline`, in seconds: negative is still inside the
    /// cooperative's claim window, `0` is the deadline second itself, positive
    /// is the buyer's reclaim window.
    first_delta: i64,
    gap: u64,
    second: Action,
}

fn arb_plan() -> impl Strategy<Value = Plan> {
    (
        0u32..=5_000,
        0u32..=5_000,
        1u64..=10_000,
        0u64..=1_000_000,
        arb_action(),
        -3i64..=3,
        0u64..=10_000,
        arb_action(),
    )
        .prop_filter("advance1_bps + advance2_bps <= 10_000", |(a1, a2, ..)| {
            *a1 + *a2 <= 10_000
        })
        .prop_map(
            |(
                advance1_bps,
                advance2_bps,
                claim_window_secs,
                opened_at,
                first,
                first_delta,
                gap,
                second,
            )| {
                Plan {
                    advance1_bps,
                    advance2_bps,
                    claim_window_secs,
                    opened_at,
                    first,
                    first_delta,
                    gap,
                    second,
                }
            },
        )
}

// ---------- one tranche's run ----------

/// What one attempt did, recorded so the two runs can be compared field by
/// field. Nothing in here is tranche-specific except `owns_release_status`,
/// which is true for either tranche when the status is that tranche's own
/// `Advance*Released` value.
#[derive(Debug, PartialEq, Eq)]
struct Step {
    action: Action,
    /// `None` when the call succeeded, `Some(error)` when the contract refused
    /// it. The error is part of the observation: guard ordering is behavior.
    refusal: Option<Error>,
    /// The tranche's own deadline after the attempt.
    deadline: u64,
    claimed: bool,
    expired: bool,
    owns_release_status: bool,
}

struct Scene {
    env: Env,
    contract: EscrowContractClient<'static>,
    token: token::Client<'static>,
    buyer: Address,
    cooperative: Address,
}

/// A fresh commitment with `advance1_bps`/`advance2_bps` and the generated
/// claim window, at ledger timestamp 0, with the buyer funded for the whole
/// deal so a deposit can move.
fn scene(advance1_bps: u32, advance2_bps: u32, claim_window_secs: u64) -> Scene {
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
    let token = token::Client::new(&env, &token_address);
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &TOTAL);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &CommitmentTerms {
            total_amount: TOTAL,
            advance1_bps,
            advance2_bps,
            claim_window_secs,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: QUANTITY,
            grade_price_bps: OnChainVec::from_array(&env, FULL_PRICE_BPS),
        },
        &None,
    );

    Scene {
        env,
        contract,
        token,
        buyer,
        cooperative,
    }
}

impl Scene {
    fn set_time(&self, timestamp: u64) {
        self.env.ledger().set_timestamp(timestamp);
    }

    fn deadline(&self, tranche: Tranche) -> u64 {
        let c = self.contract.get_commitment();
        match tranche {
            Tranche::One => c.advance1_deadline,
            Tranche::Two => c.advance2_deadline,
        }
    }

    fn flags(&self, tranche: Tranche) -> (bool, bool) {
        let c = self.contract.get_commitment();
        match tranche {
            Tranche::One => (c.advance1_claimed, c.advance1_expired),
            Tranche::Two => (c.advance2_claimed, c.advance2_expired),
        }
    }

    fn owns_release_status(&self, tranche: Tranche) -> bool {
        let expected = match tranche {
            Tranche::One => Status::Advance1Released,
            Tranche::Two => Status::Advance2Released,
        };
        self.contract.get_status() == expected
    }

    /// Opens the tranche under test at `opened_at`, driving through the
    /// tranche's own status precondition first — tranche 2 cannot open before
    /// the warehouse operator's checkpoint attestation.
    fn open(&self, tranche: Tranche, opened_at: u64) {
        self.contract.lock();
        match tranche {
            Tranche::One => {
                self.set_time(opened_at);
                self.contract.release_advance_1();
            }
            Tranche::Two => {
                // Tranche 1's window is opened and left untouched; only
                // tranche 2 is under test in this run.
                self.contract.release_advance_1();
                self.contract.mark_checkpoint();
                self.set_time(opened_at);
                self.contract.release_advance_2();
            }
        }
    }

    fn claim(&self, tranche: Tranche) -> Result<(), Error> {
        match tranche {
            Tranche::One => match self.contract.try_claim_advance_1() {
                Ok(Ok(())) => Ok(()),
                Err(Ok(error)) => Err(error),
                other => panic!("unexpected host result from claim_advance_1: {other:?}"),
            },
            Tranche::Two => match self.contract.try_claim_advance_2() {
                Ok(Ok(())) => Ok(()),
                Err(Ok(error)) => Err(error),
                other => panic!("unexpected host result from claim_advance_2: {other:?}"),
            },
        }
    }

    fn reclaim(&self, tranche: Tranche) -> Result<(), Error> {
        match tranche {
            Tranche::One => match self.contract.try_reclaim_advance_1() {
                Ok(Ok(())) => Ok(()),
                Err(Ok(error)) => Err(error),
                other => panic!("unexpected host result from reclaim_advance_1: {other:?}"),
            },
            Tranche::Two => match self.contract.try_reclaim_advance_2() {
                Ok(Ok(())) => Ok(()),
                Err(Ok(error)) => Err(error),
                other => panic!("unexpected host result from reclaim_advance_2: {other:?}"),
            },
        }
    }

    /// Runs one claim/reclaim/no-op at `at`, asserting the money moved is
    /// exactly `expected` and went to the party the action names — and that a
    /// refused attempt moves nothing at all.
    fn attempt(&self, tranche: Tranche, action: Action, at: u64, expected: i128) -> Step {
        self.set_time(at);
        let cooperative_before = self.token.balance(&self.cooperative);
        let buyer_before = self.token.balance(&self.buyer);

        let refusal = match action {
            Action::None => None,
            Action::Claim => self.claim(tranche).err(),
            Action::Reclaim => self.reclaim(tranche).err(),
        };

        let cooperative_delta = self.token.balance(&self.cooperative) - cooperative_before;
        let buyer_delta = self.token.balance(&self.buyer) - buyer_before;
        match (action, refusal) {
            (Action::Claim, None) => {
                assert_eq!(
                    cooperative_delta, expected,
                    "a claim must pay the cooperative exactly this tranche's bps amount"
                );
                assert_eq!(buyer_delta, 0);
            }
            (Action::Reclaim, None) => {
                assert_eq!(
                    buyer_delta, expected,
                    "a reclaim must refund the buyer exactly this tranche's bps amount"
                );
                assert_eq!(cooperative_delta, 0);
            }
            _ => {
                assert_eq!(cooperative_delta, 0, "a refused/no-op attempt moved funds");
                assert_eq!(buyer_delta, 0, "a refused/no-op attempt moved funds");
            }
        }

        let (claimed, expired) = self.flags(tranche);
        Step {
            action,
            refusal,
            deadline: self.deadline(tranche),
            claimed,
            expired,
            owns_release_status: self.owns_release_status(tranche),
        }
    }
}

/// Runs `plan` against `tranche` and returns what each attempt observed.
fn run(tranche: Tranche, plan: &Plan) -> Vec<Step> {
    let s = scene(plan.advance1_bps, plan.advance2_bps, plan.claim_window_secs);
    s.open(tranche, plan.opened_at);

    let deadline = s.deadline(tranche);
    assert_eq!(
        deadline,
        plan.opened_at + plan.claim_window_secs,
        "an open must record exactly opened_at + claim_window_secs"
    );

    let expected = match tranche {
        Tranche::One => TOTAL * i128::from(plan.advance1_bps) / 10_000,
        Tranche::Two => TOTAL * i128::from(plan.advance2_bps) / 10_000,
    };
    let first_at = (deadline as i64 + plan.first_delta).max(0) as u64;

    vec![
        s.attempt(tranche, plan.first, first_at, expected),
        s.attempt(tranche, plan.second, first_at + plan.gap, expected),
    ]
}

// ---------- the differential property ----------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The same plan against both tranches must produce the same rule-level
    /// observations. `ProptestConfig::with_cases(64)` keeps the run to a few
    /// seconds: each case builds two host environments and a commitment.
    #[test]
    fn tranche_one_and_tranche_two_apply_the_same_claim_rules(plan in arb_plan()) {
        let one = run(Tranche::One, &plan);
        let two = run(Tranche::Two, &plan);
        prop_assert_eq!(
            &one,
            &two,
            "the two advance tranches diverged for {:?}",
            plan
        );
    }
}
