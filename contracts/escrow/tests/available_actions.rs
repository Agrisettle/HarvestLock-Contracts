//! Issue #272: the contract-provided `available_actions` view.
//!
//! This suite is the table-driven counterpart of the hand-written
//! `docs/griefing-and-inaction.md` §1 table. Every row builds one concrete
//! state (including both sides of every deadline), asserts the exact action
//! list the view advertises, then proves the contract agrees with itself:
//! each advertised call succeeds from a fresh copy of that state, and a
//! sample of non-advertised calls fails with the expected guard error.
//!
//! A fresh commitment is built per call because successful calls mutate
//! state; reusing one world would test the *next* row's state instead.

#![allow(dead_code)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use harvestlock_escrow::{
    Action, ActionDeadline, AllocationMember, AuthParty, CommitmentTerms, Error, EscrowContract,
    EscrowContractClient, OracleConfig, Status,
};
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::{token, Address, BytesN, ConversionError, Env, InvokeError, Symbol, Vec};

const DENOMINATED_NO_SHORTFALL: i128 = 500_000;
const DENOMINATED_WITH_SHORTFALL: i128 = 1_000_000;
/// Comfortably longer than every deadline in these fixtures, so a quote
/// written at t=0 stays fresh after the ledger clock has jumped forward.
const MAX_QUOTE_AGE: u64 = 10_000_000;

/// Wire-compatible stand-in for the Reflector oracle, as in `src/test.rs`.
mod mock_oracle {
    use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

    #[contracttype]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Asset {
        Stellar(Address),
        Other(Symbol),
    }

    #[contracttype]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct PriceData {
        pub price: i128,
        pub timestamp: u64,
    }

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn set_price(env: Env, price: i128, timestamp: u64) {
            env.storage()
                .instance()
                .set(&symbol_short!("price"), &(price, timestamp));
        }

        pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
            env.storage()
                .instance()
                .get::<_, (i128, u64)>(&symbol_short!("price"))
                .map(|(price, timestamp)| PriceData { price, timestamp })
        }

        pub fn decimals(_env: Env) -> u32 {
            14
        }
    }
}

/// This suite builds hundreds of short-lived Envs; recording a snapshot for
/// each one would add noise without testing anything the assertions don't
/// already cover. The snapshot-replay suite remains the home of recorded
/// fixtures.
fn test_env() -> Env {
    let mut env = Env::default();
    env.set_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
        ..Default::default()
    });
    // The shared fixture uses `mint` to fund the buyer; that SAC call needs
    // its admin's authorization just like every other suite's Env does.
    env.mock_all_auths();
    env
}

fn plain_world() -> World {
    let env = test_env();
    world_in(
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        1_500,
        1_500,
        WINDOW,
    )
}

fn oracle_world(denominated_amount: i128) -> World {
    let env = test_env();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    let token = token::Client::new(&env, &token_address);
    // FX scenarios need a third payment beyond deposit + remainder.
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &(TOTAL_AMOUNT * 3));

    let oracle_id = env.register(mock_oracle::MockOracle, ());
    let oracle = mock_oracle::MockOracleClient::new(&env, &oracle_id);
    oracle.set_price(&175_000_000_000_000, &0);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &CommitmentTerms {
            total_amount: TOTAL_AMOUNT,
            advance1_bps: 1_500,
            advance2_bps: 1_500,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &Some(OracleConfig {
            oracle_contract: oracle_id,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: MAX_QUOTE_AGE,
            denominated_amount,
        }),
    );

    World {
        env,
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        total_amount: TOTAL_AMOUNT,
    }
}

// -- state builders -------------------------------------------------------

fn draft_with_allocation() -> World {
    let w = plain_world();
    invoke(Call::SetAllocation, &w).unwrap();
    w
}

/// The other side of #277's boundary: still `Draft`, but the delivery window
/// `initialize` started has strictly passed, so `lock` is no longer possible.
fn draft_after_delivery_deadline() -> World {
    let w = plain_world();
    let deadline = w.contract.get_commitment().delivery_deadline;
    w.set_time(deadline + 1);
    w
}

fn locked() -> World {
    let w = plain_world();
    w.contract.lock();
    w
}

fn locked_at_delivery_deadline() -> World {
    let w = locked();
    let deadline = w.contract.get_commitment().delivery_deadline;
    w.set_time(deadline);
    w
}

fn locked_after_delivery_deadline() -> World {
    let w = locked();
    let deadline = w.contract.get_commitment().delivery_deadline;
    w.set_time(deadline + 1);
    w
}

fn advance1_open() -> World {
    let w = locked();
    w.contract.release_advance_1();
    w
}

/// The tranche clock starts at 0 and the window is `WINDOW`, so this is
/// the "before the deadline" side of the boundary.
fn advance1_before_deadline() -> World {
    advance1_open()
}

fn advance1_at_deadline() -> World {
    let w = advance1_open();
    let deadline = w.contract.get_commitment().advance1_deadline;
    w.set_time(deadline);
    w
}

fn advance1_after_deadline() -> World {
    let w = advance1_open();
    let deadline = w.contract.get_commitment().advance1_deadline;
    w.set_time(deadline + 1);
    w
}

fn checkpoint() -> World {
    let w = advance1_open();
    w.contract.mark_checkpoint();
    w
}

fn advance2_open() -> World {
    let w = checkpoint();
    w.contract.release_advance_2();
    let deadline = w.contract.get_commitment().advance2_deadline;
    w.set_time(deadline - 1);
    w
}

fn advance2_at_deadline() -> World {
    let w = checkpoint();
    w.contract.release_advance_2();
    let deadline = w.contract.get_commitment().advance2_deadline;
    w.set_time(deadline);
    w
}

fn advance2_after_deadline() -> World {
    let w = checkpoint();
    w.contract.release_advance_2();
    let deadline = w.contract.get_commitment().advance2_deadline;
    w.set_time(deadline + 1);
    w
}

fn ready_unfunded() -> World {
    // Same state as `drive_to_remainder_funded`, minus the payment itself,
    // so the funding window is genuinely open rather than already consumed.
    let w = plain_world();
    w.contract.lock();
    w.contract.release_advance_1();
    w.contract.claim_advance_1();
    w.contract.mark_checkpoint();
    w.contract.release_advance_2();
    w.contract.claim_advance_2();
    w.contract.ready_for_delivery();
    w
}

fn ready_before_remainder_deadline() -> World {
    ready_unfunded()
}

fn ready_at_remainder_deadline() -> World {
    let w = ready_unfunded();
    let deadline = w.contract.get_commitment().remainder_deadline;
    w.set_time(deadline);
    w
}

fn ready_after_remainder_deadline() -> World {
    let w = ready_unfunded();
    let deadline = w.contract.get_commitment().remainder_deadline;
    w.set_time(deadline + 1);
    w
}

fn ready_funded() -> World {
    let w = ready_unfunded();
    w.contract.fund_remainder();
    w
}

fn delivered() -> World {
    let w = plain_world();
    w.drive_to_delivered();
    w
}

fn delivered_with_unresolved_tranches() -> World {
    let w = plain_world();
    w.contract.lock();
    w.contract.release_advance_1();
    w.contract.mark_checkpoint();
    w.contract.release_advance_2();
    let tranche_deadline = w.contract.get_commitment().advance1_deadline;
    w.set_time(tranche_deadline + 1);
    w.contract.ready_for_delivery();
    w.contract.fund_remainder();
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    w
}

fn settled() -> World {
    let w = delivered();
    w.contract.settle();
    w
}

fn cancelled() -> World {
    let w = plain_world();
    w.contract.cancel();
    w
}

fn defaulted() -> World {
    let w = ready_after_remainder_deadline();
    w.contract.expire_remainder_window();
    w
}

fn forfeited() -> World {
    let w = locked();
    let deadline = w.contract.get_commitment().delivery_deadline;
    w.set_time(deadline + 1);
    w.contract.reclaim_on_nondelivery();
    w
}

fn disputed() -> World {
    let w = locked();
    w.contract.flag_dispute(&w.buyer.clone());
    w
}

fn disputed_before_deadline() -> World {
    disputed()
}

fn disputed_at_deadline() -> World {
    let w = disputed();
    let deadline = w.contract.get_commitment().dispute_deadline;
    w.set_time(deadline);
    w
}

fn disputed_after_deadline() -> World {
    let w = disputed();
    let deadline = w.contract.get_commitment().dispute_deadline;
    w.set_time(deadline + 1);
    w
}

fn oracle_delivered(denominated_amount: i128) -> World {
    let w = oracle_world(denominated_amount);
    w.drive_to_delivered();
    w
}

fn oracle_shortfall_before_deadline() -> World {
    let w = oracle_delivered(DENOMINATED_WITH_SHORTFALL);
    w.contract.resolve_fx_shortfall();
    let deadline = w.contract.get_commitment().fx_shortfall_deadline;
    assert!(deadline > 0);
    w.set_time(deadline - 1);
    w
}

fn oracle_shortfall_at_deadline() -> World {
    let w = oracle_delivered(DENOMINATED_WITH_SHORTFALL);
    w.contract.resolve_fx_shortfall();
    let deadline = w.contract.get_commitment().fx_shortfall_deadline;
    assert!(deadline > 0);
    w.set_time(deadline);
    w
}

fn oracle_shortfall_after_deadline() -> World {
    let w = oracle_delivered(DENOMINATED_WITH_SHORTFALL);
    w.contract.resolve_fx_shortfall();
    let deadline = w.contract.get_commitment().fx_shortfall_deadline;
    w.set_time(deadline + 1);
    w
}

fn oracle_no_shortfall_resolved() -> World {
    let w = oracle_delivered(DENOMINATED_NO_SHORTFALL);
    w.contract.resolve_fx_shortfall();
    assert_eq!(w.contract.get_commitment().fx_shortfall_amount, 0);
    w
}

// -- expected action lists ------------------------------------------------

fn act(env: &Env, function: &str, auth: AuthParty, deadline: ActionDeadline) -> Action {
    Action {
        function: Symbol::new(env, function),
        auth,
        deadline,
    }
}

fn list(w: &World, actions: &[Action]) -> Vec<Action> {
    let mut out = Vec::new(&w.env);
    for action in actions {
        out.push_back(action.clone());
    }
    out
}

fn none() -> ActionDeadline {
    ActionDeadline::None
}

fn at(t: u64) -> ActionDeadline {
    ActionDeadline::Inclusive(t)
}

fn after(t: u64) -> ActionDeadline {
    ActionDeadline::Exclusive(t)
}

/// `cancel` and `reassign_buyer` are advertised in every pre-delivery state,
/// with the same auth and no deadline, so the table doesn't repeat them.
fn push_pre_delivery_common(w: &World, v: &mut Vec<Action>) {
    let c = w.contract.get_commitment();
    if c.status != Status::Draft && w.env.ledger().timestamp() <= c.delivery_deadline {
        v.push_back(act(
            &w.env,
            "extend_delivery_deadline",
            AuthParty::BuyerAndCooperative,
            at(c.delivery_deadline),
        ));
    }
    v.push_back(act(
        &w.env,
        "cancel",
        AuthParty::BuyerAndCooperative,
        none(),
    ));
    v.push_back(act(
        &w.env,
        "reassign_buyer",
        AuthParty::BuyerCooperativeAndNewBuyer,
        none(),
    ));
}

// -- call dispatch --------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    SetAllocation,
    Lock,
    Release1,
    Claim1,
    Reclaim1,
    MarkCheckpoint,
    Release2,
    Claim2,
    Reclaim2,
    Ready,
    FundRemainder,
    ExpireRemainder,
    ReclaimNondelivery,
    ConfirmDelivery,
    Settle,
    ResolveFx,
    RefreshFx,
    FundFx,
    ExpireFx,
    Cancel,
    Reassign,
    FlagDispute,
    ResolveDispute,
    ExpireDispute,
}

impl Call {
    fn name(self) -> &'static str {
        match self {
            Call::SetAllocation => "set_allocation",
            Call::Lock => "lock",
            Call::Release1 => "release_advance_1",
            Call::Claim1 => "claim_advance_1",
            Call::Reclaim1 => "reclaim_advance_1",
            Call::MarkCheckpoint => "mark_checkpoint",
            Call::Release2 => "release_advance_2",
            Call::Claim2 => "claim_advance_2",
            Call::Reclaim2 => "reclaim_advance_2",
            Call::Ready => "ready_for_delivery",
            Call::FundRemainder => "fund_remainder",
            Call::ExpireRemainder => "expire_remainder_window",
            Call::ReclaimNondelivery => "reclaim_on_nondelivery",
            Call::ConfirmDelivery => "confirm_delivery",
            Call::Settle => "settle",
            Call::ResolveFx => "resolve_fx_shortfall",
            Call::RefreshFx => "refresh_fx_quote_status",
            Call::FundFx => "fund_fx_shortfall",
            Call::ExpireFx => "expire_fx_shortfall_window",
            Call::Cancel => "cancel",
            Call::Reassign => "reassign_buyer",
            Call::FlagDispute => "flag_dispute",
            Call::ResolveDispute => "resolve_dispute",
            Call::ExpireDispute => "expire_dispute_window",
        }
    }
}

/// `try_*` on a `Result<_, Error>` contract method returns
/// `Ok(Ok(value))` on success and `Err(Ok(Error::X))` on a contract error;
/// the two other arms are SDK conversion/host failures, not state-machine
/// outcomes.
fn outcome<T>(
    result: Result<Result<T, ConversionError>, Result<Error, InvokeError>>,
) -> Result<T, Error> {
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => panic!("unexpected SDK conversion error: {e:?}"),
        Err(Ok(e)) => Err(e),
        Err(Err(e)) => panic!("unexpected host-level invocation error: {e:?}"),
    }
}

fn invoke(call: Call, w: &World) -> Result<(), Error> {
    match call {
        Call::SetAllocation => {
            let member = AllocationMember {
                member_hash: BytesN::from_array(&w.env, &[7u8; 32]),
                share_bps: 10_000,
            };
            outcome(
                w.contract
                    .try_set_allocation(&Vec::from_array(&w.env, [member])),
            )
        }
        Call::Lock => outcome(w.contract.try_lock()),
        Call::Release1 => outcome(w.contract.try_release_advance_1()),
        Call::Claim1 => outcome(w.contract.try_claim_advance_1()),
        Call::Reclaim1 => outcome(w.contract.try_reclaim_advance_1()),
        Call::MarkCheckpoint => outcome(w.contract.try_mark_checkpoint()),
        Call::Release2 => outcome(w.contract.try_release_advance_2()),
        Call::Claim2 => outcome(w.contract.try_claim_advance_2()),
        Call::Reclaim2 => outcome(w.contract.try_reclaim_advance_2()),
        Call::Ready => outcome(w.contract.try_ready_for_delivery()),
        Call::FundRemainder => outcome(w.contract.try_fund_remainder()),
        Call::ExpireRemainder => outcome(w.contract.try_expire_remainder_window()),
        Call::ReclaimNondelivery => outcome(w.contract.try_reclaim_on_nondelivery()),
        Call::ConfirmDelivery => outcome(w.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &0)),
        Call::Settle => outcome(w.contract.try_settle()),
        Call::ResolveFx => outcome(w.contract.try_resolve_fx_shortfall()),
        Call::RefreshFx => outcome(w.contract.try_refresh_fx_quote_status()),
        Call::FundFx => outcome(w.contract.try_fund_fx_shortfall()),
        Call::ExpireFx => outcome(w.contract.try_expire_fx_shortfall_window()),
        Call::Cancel => outcome(w.contract.try_cancel()),
        Call::Reassign => {
            let new_buyer = Address::generate(&w.env);
            outcome(w.contract.try_reassign_buyer(&new_buyer))
        }
        Call::FlagDispute => outcome(w.contract.try_flag_dispute(&w.buyer.clone())),
        Call::ResolveDispute => outcome(w.contract.try_resolve_dispute()),
        Call::ExpireDispute => outcome(w.contract.try_expire_dispute_window()),
    }
}

#[derive(Clone, Copy, Debug)]
struct Absent {
    call: Call,
    error: Error,
}

const fn absent(call: Call, error: Error) -> Absent {
    Absent { call, error }
}

// -- the table ------------------------------------------------------------

type Setup = fn() -> World;
type Expected = fn(&World) -> Vec<Action>;

struct Scenario {
    name: &'static str,
    setup: Setup,
    expected: Expected,
    advertised: std::vec::Vec<Call>,
    absent: std::vec::Vec<Absent>,
}

fn scenarios() -> std::vec::Vec<Scenario> {
    vec![
        Scenario {
            name: "Draft",
            setup: plain_world,
            expected: |w| {
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "set_allocation",
                    AuthParty::Cooperative,
                    none(),
                ));
                // #277: `lock` stops being legal once the delivery window
                // that `initialize` started is over, so the advertised bound
                // is that window's end, not "no deadline".
                v.push_back(act(
                    &w.env,
                    "lock",
                    AuthParty::Buyer,
                    at(w.contract.get_commitment().delivery_deadline),
                ));
                push_pre_delivery_common(w, &mut v);
                v
            },
            advertised: vec![
                Call::SetAllocation,
                Call::Lock,
                Call::Cancel,
                Call::Reassign,
            ],
            absent: vec![
                absent(Call::Claim1, Error::NotYetOpened),
                absent(Call::FlagDispute, Error::InvalidState),
                absent(Call::FundRemainder, Error::InvalidState),
                absent(Call::ReclaimNondelivery, Error::InvalidState),
                absent(Call::Settle, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Draft with allocation already recorded",
            setup: draft_with_allocation,
            expected: |w| {
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "lock",
                    AuthParty::Buyer,
                    at(w.contract.get_commitment().delivery_deadline),
                ));
                push_pre_delivery_common(w, &mut v);
                v
            },
            advertised: vec![Call::Lock, Call::Cancel, Call::Reassign],
            absent: vec![
                absent(Call::SetAllocation, Error::AllocationAlreadySet),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Draft, after the overall delivery deadline",
            setup: draft_after_delivery_deadline,
            expected: |w| {
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "set_allocation",
                    AuthParty::Cooperative,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v
            },
            advertised: vec![Call::SetAllocation, Call::Cancel, Call::Reassign],
            absent: vec![
                absent(Call::Lock, Error::LockAfterDeliveryDeadline),
                absent(Call::ReclaimNondelivery, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Locked",
            setup: locked,
            expected: |w| {
                let mut v = Vec::new(&w.env);
                v.push_back(act(&w.env, "release_advance_1", AuthParty::Anyone, none()));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Release1,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::SetAllocation, Error::InvalidState),
                absent(Call::Claim1, Error::NotYetOpened),
                absent(Call::Release2, Error::InvalidState),
                absent(Call::ReclaimNondelivery, Error::DeliveryDeadlineNotPassed),
                absent(Call::FundRemainder, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Locked, at the overall delivery deadline",
            setup: locked_at_delivery_deadline,
            expected: |w| {
                let mut v = Vec::new(&w.env);
                v.push_back(act(&w.env, "release_advance_1", AuthParty::Anyone, none()));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Release1,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::ReclaimNondelivery, Error::DeliveryDeadlineNotPassed),
                absent(Call::SetAllocation, Error::InvalidState),
                absent(Call::Claim1, Error::NotYetOpened),
            ],
        },
        Scenario {
            name: "Locked, one tick past the overall delivery deadline",
            setup: locked_after_delivery_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(&w.env, "release_advance_1", AuthParty::Anyone, none()));
                v.push_back(act(
                    &w.env,
                    "reclaim_on_nondelivery",
                    AuthParty::Buyer,
                    after(c.delivery_deadline),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Release1,
                Call::ReclaimNondelivery,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Lock, Error::InvalidState),
                absent(Call::Claim1, Error::NotYetOpened),
                absent(Call::Settle, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Advance1Released, before the claim deadline",
            setup: advance1_before_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "claim_advance_1",
                    AuthParty::Cooperative,
                    at(c.advance1_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "mark_checkpoint",
                    AuthParty::WarehouseOperator,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Claim1,
                Call::MarkCheckpoint,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Reclaim1, Error::ClaimWindowNotPassed),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::ReclaimNondelivery, Error::DeliveryDeadlineNotPassed),
            ],
        },
        Scenario {
            name: "Advance1Released, at the claim deadline",
            setup: advance1_at_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "claim_advance_1",
                    AuthParty::Cooperative,
                    at(c.advance1_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "mark_checkpoint",
                    AuthParty::WarehouseOperator,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Claim1,
                Call::MarkCheckpoint,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Reclaim1, Error::ClaimWindowNotPassed),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::Claim2, Error::NotYetOpened),
                absent(Call::FundRemainder, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Advance1Released, one tick past the claim deadline",
            setup: advance1_after_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "reclaim_advance_1",
                    AuthParty::Buyer,
                    after(c.advance1_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "mark_checkpoint",
                    AuthParty::WarehouseOperator,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Reclaim1,
                Call::MarkCheckpoint,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Claim1, Error::ClaimWindowPassed),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::Reclaim2, Error::NotYetOpened),
                absent(Call::FundRemainder, Error::InvalidState),
            ],
        },
        Scenario {
            name: "CheckpointPassed, tranche 1 still claimable",
            setup: checkpoint,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "claim_advance_1",
                    AuthParty::Cooperative,
                    at(c.advance1_deadline),
                ));
                v.push_back(act(&w.env, "release_advance_2", AuthParty::Anyone, none()));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Claim1,
                Call::Release2,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::MarkCheckpoint, Error::InvalidState),
                absent(Call::Reclaim1, Error::ClaimWindowNotPassed),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::Claim2, Error::NotYetOpened),
            ],
        },
        Scenario {
            name: "Advance2Released, both claim windows open",
            setup: advance2_open,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "claim_advance_1",
                    AuthParty::Cooperative,
                    at(c.advance1_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "claim_advance_2",
                    AuthParty::Cooperative,
                    at(c.advance2_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "ready_for_delivery",
                    AuthParty::Cooperative,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Claim1,
                Call::Claim2,
                Call::Ready,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Reclaim1, Error::ClaimWindowNotPassed),
                absent(Call::Reclaim2, Error::ClaimWindowNotPassed),
                absent(Call::MarkCheckpoint, Error::InvalidState),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::Release2, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Advance2Released, at the tranche-2 claim deadline",
            setup: advance2_at_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "claim_advance_1",
                    AuthParty::Cooperative,
                    at(c.advance1_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "claim_advance_2",
                    AuthParty::Cooperative,
                    at(c.advance2_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "ready_for_delivery",
                    AuthParty::Cooperative,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Claim1,
                Call::Claim2,
                Call::Ready,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Reclaim1, Error::ClaimWindowNotPassed),
                absent(Call::Reclaim2, Error::ClaimWindowNotPassed),
                absent(Call::Release2, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Advance2Released, one tick past the tranche-2 claim deadline",
            setup: advance2_after_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "reclaim_advance_1",
                    AuthParty::Buyer,
                    after(c.advance1_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "reclaim_advance_2",
                    AuthParty::Buyer,
                    after(c.advance2_deadline),
                ));
                v.push_back(act(
                    &w.env,
                    "ready_for_delivery",
                    AuthParty::Cooperative,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::Reclaim1,
                Call::Reclaim2,
                Call::Ready,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::Claim1, Error::ClaimWindowPassed),
                absent(Call::Claim2, Error::ClaimWindowPassed),
                absent(Call::MarkCheckpoint, Error::InvalidState),
            ],
        },
        Scenario {
            name: "ReadyForDelivery, unfunded, before the remainder deadline",
            setup: ready_before_remainder_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "fund_remainder",
                    AuthParty::Buyer,
                    at(c.remainder_deadline),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::FundRemainder,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::ExpireRemainder, Error::RemainderWindowNotPassed),
                absent(Call::ConfirmDelivery, Error::RemainderNotFunded),
                absent(Call::Settle, Error::InvalidState),
            ],
        },
        Scenario {
            name: "ReadyForDelivery, unfunded, at the remainder deadline",
            setup: ready_at_remainder_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "fund_remainder",
                    AuthParty::Buyer,
                    at(c.remainder_deadline),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::FundRemainder,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::ExpireRemainder, Error::RemainderWindowNotPassed),
                absent(Call::ConfirmDelivery, Error::RemainderNotFunded),
                absent(Call::Settle, Error::InvalidState),
                absent(Call::ReclaimNondelivery, Error::DeliveryDeadlineNotPassed),
            ],
        },
        Scenario {
            name: "ReadyForDelivery, unfunded, one tick past the remainder deadline",
            setup: ready_after_remainder_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "expire_remainder_window",
                    AuthParty::Anyone,
                    after(c.remainder_deadline),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::ExpireRemainder,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::FundRemainder, Error::RemainderWindowPassed),
                absent(Call::ConfirmDelivery, Error::RemainderNotFunded),
                absent(Call::Settle, Error::InvalidState),
            ],
        },
        Scenario {
            name: "ReadyForDelivery, remainder funded",
            setup: ready_funded,
            expected: |w| {
                let mut v = Vec::new(&w.env);
                v.push_back(act(
                    &w.env,
                    "confirm_delivery",
                    AuthParty::WarehouseOperator,
                    none(),
                ));
                push_pre_delivery_common(w, &mut v);
                v.push_back(act(
                    &w.env,
                    "flag_dispute",
                    AuthParty::AnyNamedParty,
                    none(),
                ));
                v
            },
            advertised: vec![
                Call::ConfirmDelivery,
                Call::Cancel,
                Call::Reassign,
                Call::FlagDispute,
            ],
            absent: vec![
                absent(Call::FundRemainder, Error::RemainderAlreadyFunded),
                absent(Call::ExpireRemainder, Error::RemainderAlreadyFunded),
                absent(Call::Settle, Error::InvalidState),
                absent(Call::ReclaimNondelivery, Error::DeliveryDeadlineNotPassed),
            ],
        },
        Scenario {
            name: "Delivered, no oracle, tranches resolved",
            setup: delivered,
            expected: |w| {
                list(
                    w,
                    &[
                        act(&w.env, "settle", AuthParty::Anyone, none()),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::Settle, Call::FlagDispute],
            absent: vec![
                absent(Call::ResolveFx, Error::OracleNotConfigured),
                absent(Call::RefreshFx, Error::OracleNotConfigured),
                absent(Call::FundFx, Error::FxNotResolved),
                absent(Call::ExpireFx, Error::FxNotResolved),
                absent(Call::Cancel, Error::InvalidState),
                absent(Call::Reassign, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Delivered with an unresolved tranche",
            setup: delivered_with_unresolved_tranches,
            expected: |w| {
                let c = w.contract.get_commitment();
                list(
                    w,
                    &[
                        act(
                            &w.env,
                            "reclaim_advance_1",
                            AuthParty::Buyer,
                            after(c.advance1_deadline),
                        ),
                        act(
                            &w.env,
                            "reclaim_advance_2",
                            AuthParty::Buyer,
                            after(c.advance2_deadline),
                        ),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::Reclaim1, Call::Reclaim2, Call::FlagDispute],
            absent: vec![
                absent(Call::Settle, Error::TrancheUnresolved),
                absent(Call::ResolveFx, Error::TrancheUnresolved),
                absent(Call::RefreshFx, Error::OracleNotConfigured),
                absent(Call::FundFx, Error::FxNotResolved),
            ],
        },
        Scenario {
            name: "Settled (terminal)",
            setup: settled,
            expected: |w| list(w, &[]),
            advertised: vec![],
            absent: vec![
                absent(Call::Settle, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
                absent(Call::Release1, Error::InvalidState),
                absent(Call::FundRemainder, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Cancelled (terminal)",
            setup: cancelled,
            expected: |w| list(w, &[]),
            advertised: vec![],
            absent: vec![
                absent(Call::Lock, Error::InvalidState),
                absent(Call::Cancel, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Defaulted (terminal)",
            setup: defaulted,
            expected: |w| list(w, &[]),
            advertised: vec![],
            absent: vec![
                absent(Call::FundRemainder, Error::InvalidState),
                absent(Call::ExpireRemainder, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Forfeited (terminal)",
            setup: forfeited,
            expected: |w| list(w, &[]),
            advertised: vec![],
            absent: vec![
                absent(Call::ReclaimNondelivery, Error::InvalidState),
                absent(Call::Settle, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Disputed, before the dispute deadline",
            setup: disputed_before_deadline,
            expected: |w| {
                list(
                    w,
                    &[act(
                        &w.env,
                        "resolve_dispute",
                        AuthParty::AllParties,
                        none(),
                    )],
                )
            },
            advertised: vec![Call::ResolveDispute],
            absent: vec![
                absent(Call::ExpireDispute, Error::DisputeWindowNotPassed),
                absent(Call::Lock, Error::InvalidState),
                absent(Call::Settle, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Disputed, at the dispute deadline",
            setup: disputed_at_deadline,
            expected: |w| {
                list(
                    w,
                    &[act(
                        &w.env,
                        "resolve_dispute",
                        AuthParty::AllParties,
                        none(),
                    )],
                )
            },
            advertised: vec![Call::ResolveDispute],
            absent: vec![
                absent(Call::ExpireDispute, Error::DisputeWindowNotPassed),
                absent(Call::Lock, Error::InvalidState),
                absent(Call::Settle, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Disputed, one tick past the dispute deadline",
            setup: disputed_after_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                list(
                    w,
                    &[
                        act(&w.env, "resolve_dispute", AuthParty::AllParties, none()),
                        act(
                            &w.env,
                            "expire_dispute_window",
                            AuthParty::Anyone,
                            after(c.dispute_deadline),
                        ),
                    ],
                )
            },
            advertised: vec![Call::ResolveDispute, Call::ExpireDispute],
            absent: vec![
                absent(Call::Lock, Error::InvalidState),
                absent(Call::Settle, Error::InvalidState),
                absent(Call::FlagDispute, Error::InvalidState),
            ],
        },
        Scenario {
            name: "Delivered with an oracle, FX unresolved",
            setup: || oracle_delivered(DENOMINATED_NO_SHORTFALL),
            expected: |w| {
                list(
                    w,
                    &[
                        act(&w.env, "resolve_fx_shortfall", AuthParty::Anyone, none()),
                        act(
                            &w.env,
                            "replace_oracle_config",
                            AuthParty::BuyerAndCooperative,
                            none(),
                        ),
                        act(&w.env, "refresh_fx_quote_status", AuthParty::Anyone, none()),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::ResolveFx, Call::RefreshFx, Call::FlagDispute],
            absent: vec![
                absent(Call::Settle, Error::FxNotResolved),
                absent(Call::FundFx, Error::FxNotResolved),
                absent(Call::ExpireFx, Error::FxNotResolved),
            ],
        },
        Scenario {
            name: "Delivered with a resolved FX shortfall, before the funding deadline",
            setup: oracle_shortfall_before_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                list(
                    w,
                    &[
                        act(
                            &w.env,
                            "fund_fx_shortfall",
                            AuthParty::Buyer,
                            at(c.fx_shortfall_deadline),
                        ),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::FundFx, Call::FlagDispute],
            absent: vec![
                absent(Call::Settle, Error::FxShortfallUnfunded),
                absent(Call::ExpireFx, Error::FxShortfallWindowNotPassed),
                absent(Call::ResolveFx, Error::FxAlreadyResolved),
            ],
        },
        Scenario {
            name: "Delivered with a resolved FX shortfall, at the funding deadline",
            setup: oracle_shortfall_at_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                list(
                    w,
                    &[
                        act(
                            &w.env,
                            "fund_fx_shortfall",
                            AuthParty::Buyer,
                            at(c.fx_shortfall_deadline),
                        ),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::FundFx, Call::FlagDispute],
            absent: vec![
                absent(Call::Settle, Error::FxShortfallUnfunded),
                absent(Call::ResolveFx, Error::FxAlreadyResolved),
                absent(Call::RefreshFx, Error::FxAlreadyResolved),
                absent(Call::ExpireFx, Error::FxShortfallWindowNotPassed),
            ],
        },
        Scenario {
            name: "Delivered with a resolved FX shortfall, one tick past the deadline",
            setup: oracle_shortfall_after_deadline,
            expected: |w| {
                let c = w.contract.get_commitment();
                list(
                    w,
                    &[
                        act(
                            &w.env,
                            "expire_fx_shortfall_window",
                            AuthParty::Anyone,
                            after(c.fx_shortfall_deadline),
                        ),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::ExpireFx, Call::FlagDispute],
            absent: vec![
                absent(Call::Settle, Error::FxShortfallUnfunded),
                absent(Call::FundFx, Error::FxShortfallWindowPassed),
                absent(Call::ResolveFx, Error::FxAlreadyResolved),
            ],
        },
        Scenario {
            name: "Delivered with FX resolved and no shortfall",
            setup: oracle_no_shortfall_resolved,
            expected: |w| {
                list(
                    w,
                    &[
                        act(&w.env, "settle", AuthParty::Anyone, none()),
                        act(&w.env, "flag_dispute", AuthParty::AnyNamedParty, none()),
                    ],
                )
            },
            advertised: vec![Call::Settle, Call::FlagDispute],
            absent: vec![
                absent(Call::FundFx, Error::NoFxShortfall),
                absent(Call::ExpireFx, Error::NoFxShortfall),
                absent(Call::ResolveFx, Error::FxAlreadyResolved),
                absent(Call::RefreshFx, Error::FxAlreadyResolved),
            ],
        },
    ]
}

#[test]
fn available_actions_table_walks_every_status_and_deadline() {
    for scenario in scenarios() {
        let w = (scenario.setup)();
        assert_eq!(
            w.contract.available_actions(),
            (scenario.expected)(&w),
            "wrong action list for {}",
            scenario.name
        );

        for call in scenario.advertised {
            let w = (scenario.setup)();
            assert_eq!(
                invoke(call, &w),
                Ok(()),
                "{} advertised {}, but the call failed",
                scenario.name,
                call.name()
            );
        }

        for missing in scenario.absent {
            let w = (scenario.setup)();
            assert_eq!(
                invoke(missing.call, &w),
                Err(missing.error),
                "{} did not advertise {}, which should fail with {:?}",
                scenario.name,
                missing.call.name(),
                missing.error
            );
        }
    }
}

/// A future `Status` variant must fail this test until the table grows a
/// row for it — otherwise "every status" quietly stops being true.
#[test]
fn table_covers_every_status() {
    let covered: std::vec::Vec<Status> = scenarios()
        .into_iter()
        .map(|scenario| (scenario.setup)().status())
        .collect();
    for status in [
        Status::Draft,
        Status::Locked,
        Status::Advance1Released,
        Status::CheckpointPassed,
        Status::Advance2Released,
        Status::ReadyForDelivery,
        Status::Delivered,
        Status::Settled,
        Status::Cancelled,
        Status::Defaulted,
        Status::Forfeited,
        Status::Disputed,
    ] {
        assert!(
            covered.contains(&status),
            "no table row builds a {status:?} commitment"
        );
    }
}

#[test]
fn available_actions_is_empty_before_initialize() {
    let env = test_env();
    let id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &id);
    assert_eq!(contract.available_actions(), Vec::<Action>::new(&env));
}
