//! Issue #213 — the no-side-effect law: a call that fails changes nothing.
//!
//! For a randomly chosen *reachable* commitment state, every public method is
//! called once with random arguments and a random signer set. Before the call
//! the suite snapshots the three instance-storage reads the issue names
//! (`get_commitment`, `get_allocation`, `get_oracle_config`), all four token
//! balances (buyer, cooperative, warehouse operator, escrow), and the event
//! buffer. If the call returns an `Err` or aborts at the host level, all of
//! those snapshots must be byte-for-byte identical afterwards.
//!
//! Why this is worth pinning even though Soroban already rolls back a failed
//! invocation: several paths deliberately write before they can still fail.
//! `lock`, `fund_remainder`, `claim_tranche` and the sweeps save the state
//! effect *before* the token transfer (checks-effects-interactions) and rely on
//! that rollback if the transfer then traps. `oracle_fault_injection.rs`
//! proves this for the oracle path alone; this suite covers every public
//! method and both failure shapes (clean `Err`, and host abort/panic) at once.
//!
//! # Reachable states
//!
//! The driver below walks the real contract through 20 states covering each
//! lifecycle branch: draft, both tranches open/claimed/reclaimed, remainder
//! funded/unfunded/expired, delivered with and without an FX resolution,
//! disputed (before and after its deadline), and all four terminal statuses.
//! No state is fabricated by writing storage directly.
//!
//! # Signer set
//!
//! Each case draws "all named parties authorized" or "no signer" — the two
//! ends of the auth spectrum. The `Err` shape of the all-authorized case is
//! the method's own guard; the no-signer case additionally covers a failing
//! `require_auth()` (which the host reports as an abort). The law is the same
//! for both.
//!
//! # Event snapshot
//!
//! `env.events().all()` reports the last invocation only. A read (`get_status`)
//! is made immediately before the candidate call, so the buffer starts empty;
//! a failed candidate must then leave it empty. Events from successful setup
//! calls are therefore not part of the comparison, which is exactly the
//! property "a failing call emits nothing".
//!
//! # Findings
//!
//! None — no public method violated the law. See the per-method note at the
//! bottom of this file for the shapes exercised.

#![allow(dead_code)]

#[path = "common/mod.rs"]
mod common;

use common::{commitment_terms, mock_oracle, Terms};
use harvestlock_escrow::{AllocationMember, EscrowContract, EscrowContractClient, OracleConfig};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Events as _, Ledger as _};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{token, Address, Bytes, BytesN, Env, Symbol, Vec};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

struct Fixture {
    env: Env,
    escrow: EscrowContractClient<'static>,
    escrow_id: Address,
    token: token::Client<'static>,
    token_id: Address,
    oracle_id: Address,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    total: i128,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        env.mock_all_auths();

        let terms = Terms::default();
        let buyer = Address::generate(&env);
        let cooperative = Address::generate(&env);
        let warehouse = Address::generate(&env);

        let token_admin = Address::generate(&env);
        let (token_id, token, token_admin_client) = common::create_token(&env, &token_admin);
        token_admin_client.mint(&buyer, &(terms.total_amount * 3));

        let oracle_id = env.register(mock_oracle::MockOracle, ());
        mock_oracle::MockOracleClient::new(&env, &oracle_id).set_price(&66_700_000_000, &0);
        let oracle = OracleConfig {
            oracle_contract: oracle_id.clone(),
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 3_600,
            // Big enough to create an FX shortfall after a full delivery, so
            // the fund/expire-shortfall states are reachable.
            denominated_amount: 2_000_000_000,
        };

        let escrow_id = env.register(EscrowContract, ());
        let escrow = EscrowContractClient::new(&env, &escrow_id);
        escrow.initialize(
            &buyer,
            &cooperative,
            &warehouse,
            &token_id,
            &commitment_terms(&env, &terms),
            &Some(oracle),
        );

        Fixture {
            env,
            escrow,
            escrow_id,
            token,
            token_id,
            oracle_id,
            buyer,
            cooperative,
            warehouse,
            total: terms.total_amount,
        }
    }

    fn bps(&self, bps: u32) -> i128 {
        self.total * i128::from(bps) / 10_000
    }

    fn deposit(&self) -> i128 {
        self.bps(3_000)
    }

    fn drive_to_remainder_funded(&self) {
        self.escrow.lock();
        self.escrow.release_advance_1();
        self.escrow.claim_advance_1();
        self.escrow.mark_checkpoint();
        self.escrow.release_advance_2();
        self.escrow.claim_advance_2();
        self.escrow.ready_for_delivery();
    }

    fn drive_to_delivered(&self) {
        self.drive_to_remainder_funded();
        self.escrow.fund_remainder();
        self.escrow.confirm_delivery(&1_000, &0);
    }

    fn drive_to_fx_resolved(&self) {
        self.drive_to_delivered();
        self.escrow.resolve_fx_shortfall();
    }

    fn drive_to_shortfall_funded(&self) {
        self.drive_to_fx_resolved();
        self.escrow.fund_fx_shortfall();
    }

    /// Walk the contract to one of the 20 reachable states enumerated in the
    /// module docs. Every step is a real public call; nothing writes storage
    /// directly.
    fn drive_to(&self, state: u8) {
        match state {
            0 => {}
            1 => {
                self.escrow.lock();
            }
            2 => {
                self.escrow.lock();
                self.escrow.release_advance_1();
            }
            3 => {
                self.escrow.lock();
                self.escrow.release_advance_1();
                let deadline = self.escrow.get_commitment().advance1_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
            }
            4 => {
                self.escrow.lock();
                self.escrow.release_advance_1();
                self.escrow.claim_advance_1();
                self.escrow.mark_checkpoint();
            }
            5 => {
                self.escrow.lock();
                self.escrow.release_advance_1();
                self.escrow.claim_advance_1();
                self.escrow.mark_checkpoint();
                self.escrow.release_advance_2();
            }
            6 => {
                self.escrow.lock();
                self.escrow.release_advance_1();
                self.escrow.claim_advance_1();
                self.escrow.mark_checkpoint();
                self.escrow.release_advance_2();
                let deadline = self.escrow.get_commitment().advance2_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
            }
            7 => self.drive_to_remainder_funded(),
            8 => {
                self.drive_to_remainder_funded();
                let deadline = self.escrow.get_commitment().remainder_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
            }
            9 => self.drive_to_delivered(),
            10 => self.drive_to_fx_resolved(),
            11 => self.drive_to_shortfall_funded(),
            12 => {
                self.drive_to_fx_resolved();
                let deadline = self.escrow.get_commitment().fx_shortfall_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
            }
            13 => {
                self.escrow.lock();
                let deadline = self.escrow.get_commitment().delivery_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
            }
            14 => {
                self.escrow.lock();
                self.escrow.flag_dispute(&self.buyer);
            }
            15 => {
                self.escrow.lock();
                self.escrow.flag_dispute(&self.buyer);
                let deadline = self.escrow.get_commitment().dispute_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
            }
            16 => {
                self.escrow.lock();
                self.escrow.cancel();
            }
            17 => {
                self.drive_to_shortfall_funded();
                self.escrow.settle();
            }
            18 => {
                self.drive_to_remainder_funded();
                let deadline = self.escrow.get_commitment().remainder_deadline;
                self.env.ledger().set_timestamp(deadline + 1);
                self.escrow.expire_remainder_window();
            }
            19 => {
                self.escrow.lock();
                let c = self.escrow.get_commitment();
                self.env
                    .ledger()
                    .set_timestamp(c.delivery_deadline + c.remainder_window_secs + 1);
                self.escrow.expire_delivery_window();
            }
            other => panic!("unreachable state id {other}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot of everything a failed call must leave untouched
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    // `Commitment`/`AllocationMember` derive `PartialEq` only under
    // `cfg(test)` (the deployed WASM does not pay for it), and integration
    // tests do not compile the crate with that cfg. XDR bytes are the same
    // comparison without widening the ABI.
    commitment: Option<Bytes>,
    allocation: Option<Bytes>,
    oracle_config: Option<Bytes>,
    buyer_balance: i128,
    cooperative_balance: i128,
    warehouse_balance: i128,
    escrow_balance: i128,
    events: usize,
}

impl Snapshot {
    fn take(f: &Fixture) -> Self {
        let commitment = f
            .escrow
            .try_get_commitment()
            .ok()
            .and_then(|r| r.ok())
            .map(|c| c.to_xdr(&f.env));
        let allocation = f
            .escrow
            .try_get_allocation()
            .ok()
            .and_then(|r| r.ok())
            .map(|a| a.to_xdr(&f.env));
        let oracle_config = f
            .escrow
            .try_get_oracle_config()
            .ok()
            .and_then(|r| r.ok())
            .flatten()
            .map(|o| o.to_xdr(&f.env));
        let events = f
            .env
            .events()
            .all()
            .filter_by_contract(&f.escrow_id)
            .events()
            .len();
        Snapshot {
            commitment,
            allocation,
            oracle_config,
            buyer_balance: f.token.balance(&f.buyer),
            cooperative_balance: f.token.balance(&f.cooperative),
            warehouse_balance: f.token.balance(&f.warehouse),
            escrow_balance: f.token.balance(&f.escrow_id),
            events,
        }
    }
}

// ---------------------------------------------------------------------------
// Candidate methods
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Method {
    Initialize,
    GetOracleConfig,
    ReplaceOracleConfig,
    OracleRate,
    SetAllocation,
    GetAllocation,
    Lock,
    ReleaseAdvance1,
    ClaimAdvance1,
    ReclaimAdvance1,
    MarkCheckpoint,
    ReleaseAdvance2,
    ClaimAdvance2,
    ReclaimAdvance2,
    ReadyForDelivery,
    FundRemainder,
    ExpireRemainderWindow,
    ReclaimOnNondelivery,
    ExpireDeliveryWindow,
    ConfirmDelivery,
    Settle,
    ResolveFxShortfall,
    RefreshFxQuoteStatus,
    FundFxShortfall,
    ExpireFxShortfallWindow,
    Cancel,
    ReassignBuyer,
    FlagDispute,
    ResolveDispute,
    ExpireDisputeWindow,
    GetStatus,
    GetCommitment,
    AvailableActions,
    GetCommitmentView,
}

fn arb_method() -> impl Strategy<Value = Method> {
    prop_oneof![
        Just(Method::Initialize),
        Just(Method::GetOracleConfig),
        Just(Method::ReplaceOracleConfig),
        Just(Method::OracleRate),
        Just(Method::SetAllocation),
        Just(Method::GetAllocation),
        Just(Method::Lock),
        Just(Method::ReleaseAdvance1),
        Just(Method::ClaimAdvance1),
        Just(Method::ReclaimAdvance1),
        Just(Method::MarkCheckpoint),
        Just(Method::ReleaseAdvance2),
        Just(Method::ClaimAdvance2),
        Just(Method::ReclaimAdvance2),
        Just(Method::ReadyForDelivery),
        Just(Method::FundRemainder),
        Just(Method::ExpireRemainderWindow),
        Just(Method::ReclaimOnNondelivery),
        Just(Method::ExpireDeliveryWindow),
        Just(Method::ConfirmDelivery),
        Just(Method::Settle),
        Just(Method::ResolveFxShortfall),
        Just(Method::RefreshFxQuoteStatus),
        Just(Method::FundFxShortfall),
        Just(Method::ExpireFxShortfallWindow),
        Just(Method::Cancel),
        Just(Method::ReassignBuyer),
        Just(Method::FlagDispute),
        Just(Method::ResolveDispute),
        Just(Method::ExpireDisputeWindow),
        Just(Method::GetStatus),
        Just(Method::GetCommitment),
        Just(Method::AvailableActions),
        Just(Method::GetCommitmentView),
    ]
}

/// Random, env-independent arguments for one candidate call.
#[derive(Clone, Debug)]
struct Args {
    delivered_quantity: u32,
    grade_index: u32,
    member_count: u8,
    share_bps: u32,
    flagger_choice: u8,
    max_age_secs: u64,
    denominated_amount: i128,
}

impl Args {
    fn members(&self, env: &Env) -> Vec<AllocationMember> {
        let mut members = Vec::new(env);
        for i in 0..self.member_count {
            members.push_back(AllocationMember {
                member_hash: BytesN::from_array(env, &[i; 32]),
                share_bps: self.share_bps,
            });
        }
        members
    }

    fn flagger(&self, f: &Fixture) -> Address {
        match self.flagger_choice {
            0 => f.buyer.clone(),
            1 => f.cooperative.clone(),
            2 => f.warehouse.clone(),
            _ => Address::generate(&f.env),
        }
    }
}

/// One candidate invocation. Returns `true` when the call succeeded, `false`
/// for a clean contract `Err` or a host abort (both are failure shapes the law
/// covers). The return value is otherwise discarded.
fn invoke(f: &Fixture, method: Method, args: &Args) -> bool {
    let result = catch_unwind(AssertUnwindSafe(|| -> bool {
        match method {
            Method::Initialize => f
                .escrow
                .try_initialize(
                    &f.buyer,
                    &f.cooperative,
                    &f.warehouse,
                    &f.token_id,
                    &commitment_terms(&f.env, &Terms::default()),
                    &None,
                )
                .is_ok_and(|inner| inner.is_ok()),
            Method::GetOracleConfig => f
                .escrow
                .try_get_oracle_config()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ReplaceOracleConfig => {
                let new_config = OracleConfig {
                    oracle_contract: f.oracle_id.clone(),
                    price_asset: Symbol::new(&f.env, "NGN"),
                    max_age_secs: args.max_age_secs,
                    denominated_amount: args.denominated_amount,
                };
                f.escrow
                    .try_replace_oracle_config(&new_config)
                    .is_ok_and(|inner| inner.is_ok())
            }
            Method::OracleRate => f.escrow.try_oracle_rate().is_ok_and(|inner| inner.is_ok()),
            Method::SetAllocation => {
                let members = args.members(&f.env);
                f.escrow
                    .try_set_allocation(&members)
                    .is_ok_and(|inner| inner.is_ok())
            }
            Method::GetAllocation => f
                .escrow
                .try_get_allocation()
                .is_ok_and(|inner| inner.is_ok()),
            Method::Lock => f.escrow.try_lock().is_ok_and(|inner| inner.is_ok()),
            Method::ReleaseAdvance1 => f
                .escrow
                .try_release_advance_1()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ClaimAdvance1 => f
                .escrow
                .try_claim_advance_1()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ReclaimAdvance1 => f
                .escrow
                .try_reclaim_advance_1()
                .is_ok_and(|inner| inner.is_ok()),
            Method::MarkCheckpoint => f
                .escrow
                .try_mark_checkpoint()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ReleaseAdvance2 => f
                .escrow
                .try_release_advance_2()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ClaimAdvance2 => f
                .escrow
                .try_claim_advance_2()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ReclaimAdvance2 => f
                .escrow
                .try_reclaim_advance_2()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ReadyForDelivery => f
                .escrow
                .try_ready_for_delivery()
                .is_ok_and(|inner| inner.is_ok()),
            Method::FundRemainder => f
                .escrow
                .try_fund_remainder()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ExpireRemainderWindow => f
                .escrow
                .try_expire_remainder_window()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ReclaimOnNondelivery => f
                .escrow
                .try_reclaim_on_nondelivery()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ExpireDeliveryWindow => f
                .escrow
                .try_expire_delivery_window()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ConfirmDelivery => f
                .escrow
                .try_confirm_delivery(&args.delivered_quantity, &args.grade_index)
                .is_ok_and(|inner| inner.is_ok()),
            Method::Settle => f.escrow.try_settle().is_ok_and(|inner| inner.is_ok()),
            Method::ResolveFxShortfall => f
                .escrow
                .try_resolve_fx_shortfall()
                .is_ok_and(|inner| inner.is_ok()),
            Method::RefreshFxQuoteStatus => f
                .escrow
                .try_refresh_fx_quote_status()
                .is_ok_and(|inner| inner.is_ok()),
            Method::FundFxShortfall => f
                .escrow
                .try_fund_fx_shortfall()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ExpireFxShortfallWindow => f
                .escrow
                .try_expire_fx_shortfall_window()
                .is_ok_and(|inner| inner.is_ok()),
            Method::Cancel => f.escrow.try_cancel().is_ok_and(|inner| inner.is_ok()),
            Method::ReassignBuyer => {
                let new_buyer = Address::generate(&f.env);
                f.escrow
                    .try_reassign_buyer(&new_buyer)
                    .is_ok_and(|inner| inner.is_ok())
            }
            Method::FlagDispute => {
                let flagger = args.flagger(f);
                f.escrow
                    .try_flag_dispute(&flagger)
                    .is_ok_and(|inner| inner.is_ok())
            }
            Method::ResolveDispute => f
                .escrow
                .try_resolve_dispute()
                .is_ok_and(|inner| inner.is_ok()),
            Method::ExpireDisputeWindow => f
                .escrow
                .try_expire_dispute_window()
                .is_ok_and(|inner| inner.is_ok()),
            Method::GetStatus => f.escrow.try_get_status().is_ok_and(|inner| inner.is_ok()),
            Method::GetCommitment => f
                .escrow
                .try_get_commitment()
                .is_ok_and(|inner| inner.is_ok()),
            Method::AvailableActions => {
                let _ = f.escrow.available_actions();
                true
            }
            Method::GetCommitmentView => f
                .escrow
                .try_get_commitment_view()
                .is_ok_and(|inner| inner.is_ok()),
        }
    }));
    // A raw unwind is the same failure shape as a host abort for this law.
    result.unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Property
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 600,
        ..Default::default()
    })]

    #[test]
    fn failed_calls_leave_storage_balances_and_events_untouched(
        state in 0u8..20,
        method in arb_method(),
        delivered_quantity in any::<u32>(),
        grade_index in any::<u32>(),
        member_count in 0u8..5,
        share_bps in 0u32..12_000,
        flagger_choice in 0u8..4,
        max_age_secs in any::<u64>(),
        denominated_amount in -10i128..5_000_000,
        all_parties_authorized in any::<bool>(),
    ) {
        let f = Fixture::new();
        f.drive_to(state);

        // The two ends of the signer spectrum. Setup used `mock_all_auths`;
        // an empty mock list turns every subsequent `require_auth()` into a
        // failure, while recording mode keeps the all-authorized case.
        if !all_parties_authorized {
            f.env.mock_auths(&[]);
        }

        // Clear the event buffer with a read so the next `all()` describes
        // the candidate call only.
        let _ = f.escrow.get_status();
        let before = Snapshot::take(&f);
        prop_assert_eq!(before.events, 0, "event buffer did not start empty");

        let args = Args {
            delivered_quantity,
            grade_index,
            member_count,
            share_bps,
            flagger_choice,
            max_age_secs,
            denominated_amount,
        };
        let succeeded = invoke(&f, method, &args);

        if !succeeded {
            let after = Snapshot::take(&f);
            prop_assert_eq!(
                &before,
                &after,
                "failed call {:?} in state {} changed state",
                method,
                state
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Per-method coverage note
// ---------------------------------------------------------------------------
//
// Every exported state-changing function is in `Method`, including the ones
// with arguments (`confirm_delivery`, `replace_oracle_config`,
// `set_allocation`, `reassign_buyer`, `flag_dispute`, `initialize`). The
// reachable-state driver reaches each status-transition guard at least once,
// and the random argument draw covers the argument-validation guards
// (`InvalidGradeIndex`, `InvalidAllocation`, `InvalidOracleConfig`, ...).
// Read-only methods are included because the issue says "every public method";
// they trivially satisfy the law, and including them keeps the suite total
// rather than a hand-picked subset.
//
// The one shape outside this suite is a *successful* oracle cross-contract
// read that then traps inside the escrow's own arithmetic: those paths are
// already covered by `oracle_fault_injection.rs` (including the host-abort
// budget case), and they are success-then-abort rather than a clean contract
// `Err`. `window_overflow.rs` pins the deadline-arithmetic aborts
// separately.
//
// The law only needs "failed", not "failed for this reason" — the specific
// error codes are pinned elsewhere (`error_discriminants.snapshot` and the
// per-guard suites), so this file deliberately does not assert them.
