//! #229 — generated checks that opposing terminal calls do not both succeed.
//!
//! Pair list: `fund_remainder` / `expire_remainder_window`,
//! `fund_fx_shortfall` / `expire_fx_shortfall_window`,
//! `claim_advance_N` / `reclaim_advance_N`,
//! `confirm_delivery` / `reclaim_on_nondelivery`,
//! `expire_remainder_window` / `reclaim_on_nondelivery`, and
//! `resolve_dispute` / `expire_dispute_window`.
//!
//! Each side runs against a separately-built copy of the same generated
//! pre-state, so the first action cannot make the second appear unavailable.
//! Window lengths, start timestamps, and lifecycle prefixes are generated.
//! The delivery pair also pins the deadline guard added for #199.
//! The two known overlapping pairs remain ignored until their precedence is
//! decided in #348.

use harvestlock_escrow::{
    CommitmentTerms, EscrowContract, EscrowContractClient, OracleConfig, Status,
};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol, Vec};

const TOTAL_AMOUNT: i128 = 1_000_000;
const CLAIMED_ADVANCE_BPS: u32 = 1_500;
const CONTRACTED_QUANTITY: u32 = 1_000;
const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];
const FX_DENOMINATED_AMOUNT: i128 = 500_000;
const FX_PRICE: i128 = 300_000_000_000_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceData {
    price: i128,
    timestamp: u64,
}

#[contract]
pub struct TestOracle;

#[contractimpl]
impl TestOracle {
    pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
        Some(PriceData {
            price: FX_PRICE,
            timestamp: env.ledger().timestamp(),
        })
    }

    pub fn decimals(_env: Env) -> u32 {
        14
    }
}

struct Scene {
    env: Env,
    contract: EscrowContractClient<'static>,
    buyer: Address,
    start: u64,
}

fn scene(
    claim_window: u64,
    remainder_window: u64,
    delivery_window: u64,
    start: u64,
    with_oracle: bool,
) -> Scene {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(start);

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &(TOTAL_AMOUNT * 3));

    let oracle_config = if with_oracle {
        let oracle_id = env.register(TestOracle, ());
        Some(OracleConfig {
            oracle_contract: oracle_id,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 3_600,
            denominated_amount: FX_DENOMINATED_AMOUNT,
        })
    } else {
        None
    };

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &CommitmentTerms {
            total_amount: TOTAL_AMOUNT,
            advance1_bps: CLAIMED_ADVANCE_BPS,
            advance2_bps: CLAIMED_ADVANCE_BPS,
            claim_window_secs: claim_window,
            remainder_window_secs: remainder_window,
            delivery_window_secs: delivery_window,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &oracle_config,
    );

    Scene {
        env,
        contract,
        buyer,
        start,
    }
}

fn ready_for_delivery(s: &Scene, prefix_claims_advances: bool, elapsed: u64) {
    s.contract.lock();
    s.contract.release_advance_1();
    if prefix_claims_advances {
        s.contract.claim_advance_1();
    }
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    if prefix_claims_advances {
        s.contract.claim_advance_2();
    }
    s.env.ledger().set_timestamp(s.start + elapsed);
    s.contract.ready_for_delivery();
}

fn ready_and_funded(s: &Scene, prefix_claims_advances: bool, elapsed: u64) {
    ready_for_delivery(s, prefix_claims_advances, elapsed);
    s.contract.fund_remainder();
}

fn around(deadline: u64, offset: i8) -> u64 {
    match offset {
        -1 => deadline - 1,
        0 => deadline,
        _ => deadline + 1,
    }
}

fn arb_window() -> impl Strategy<Value = u64> {
    1u64..100_000
}

fn arb_start() -> impl Strategy<Value = u64> {
    1_000_000u64..1_000_000_000_000
}

fn arb_offset() -> impl Strategy<Value = i8> {
    -1i8..=1
}

fn arb_prefix_elapsed() -> impl Strategy<Value = u64> {
    0u64..100_000
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    #[test]
    fn remainder_funding_and_expiry_are_exclusive(
        claim_window in arb_window(),
        remainder_window in arb_window(),
        start in arb_start(),
        elapsed in arb_prefix_elapsed(),
        offset in arb_offset(),
        prefix_claims in any::<bool>(),
    ) {
        let delivery_window = 200_000;
        let funded = scene(claim_window, remainder_window, delivery_window, start, false);
        ready_for_delivery(&funded, prefix_claims, elapsed);
        let deadline = funded.contract.get_commitment().remainder_deadline;
        funded.env.ledger().set_timestamp(around(deadline, offset));
        let funding_succeeds = funded.contract.try_fund_remainder().is_ok();

        let expired = scene(claim_window, remainder_window, delivery_window, start, false);
        ready_for_delivery(&expired, prefix_claims, elapsed);
        expired.env.ledger().set_timestamp(around(deadline, offset));
        let expiry_succeeds = expired.contract.try_expire_remainder_window().is_ok();

        prop_assert_ne!(funding_succeeds, expiry_succeeds);
    }

    #[test]
    fn fx_funding_and_expiry_are_exclusive(
        claim_window in arb_window(),
        remainder_window in arb_window(),
        start in arb_start(),
        elapsed in arb_prefix_elapsed(),
        offset in arb_offset(),
    ) {
        let delivery_window = 200_000;
        // This pair's pre-state always claims both advances. `resolve_fx_shortfall`
        // shares `settle`'s `tranches_resolved` guard -- it needs final,
        // settled figures, pinned by
        // `cannot_resolve_fx_shortfall_before_tranches_are_resolved` -- so with
        // `prefix_claims = false` the prefix leaves a tranche open and the call
        // is legitimately `TrancheUnresolved`: the pair would never become
        // comparable. `prefix_claims` still varies in the pairs whose pre-state
        // does not depend on it.
        let funded = scene(claim_window, remainder_window, delivery_window, start, true);
        ready_and_funded(&funded, true, elapsed);
        funded.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
        funded.contract.resolve_fx_shortfall();
        let deadline = funded.contract.get_commitment().fx_shortfall_deadline;
        prop_assert!(funded.contract.get_commitment().fx_shortfall_amount > 0);
        funded.env.ledger().set_timestamp(around(deadline, offset));
        let funding_succeeds = funded.contract.try_fund_fx_shortfall().is_ok();

        let expired = scene(claim_window, remainder_window, delivery_window, start, true);
        ready_and_funded(&expired, true, elapsed);
        expired.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
        expired.contract.resolve_fx_shortfall();
        expired.env.ledger().set_timestamp(around(deadline, offset));
        let expiry_succeeds = expired.contract.try_expire_fx_shortfall_window().is_ok();

        prop_assert_ne!(funding_succeeds, expiry_succeeds);
    }

    #[test]
    fn first_advance_claim_and_reclaim_are_exclusive(
        claim_window in arb_window(),
        remainder_window in arb_window(),
        start in arb_start(),
        elapsed in arb_prefix_elapsed(),
        offset in arb_offset(),
    ) {
        let claim = scene(claim_window, remainder_window, 200_000, start, false);
        claim.contract.lock();
        claim.env.ledger().set_timestamp(start + elapsed);
        claim.contract.release_advance_1();
        let deadline = claim.contract.get_commitment().advance1_deadline;
        claim.env.ledger().set_timestamp(around(deadline, offset));
        let claim_succeeds = claim.contract.try_claim_advance_1().is_ok();

        let reclaim = scene(claim_window, remainder_window, 200_000, start, false);
        reclaim.contract.lock();
        reclaim.env.ledger().set_timestamp(start + elapsed);
        reclaim.contract.release_advance_1();
        reclaim.env.ledger().set_timestamp(around(deadline, offset));
        let reclaim_succeeds = reclaim.contract.try_reclaim_advance_1().is_ok();

        prop_assert_ne!(claim_succeeds, reclaim_succeeds);
    }

    #[test]
    fn second_advance_claim_and_reclaim_are_exclusive(
        claim_window in arb_window(),
        remainder_window in arb_window(),
        start in arb_start(),
        elapsed in arb_prefix_elapsed(),
        offset in arb_offset(),
        prefix_claims in any::<bool>(),
    ) {
        let claim = scene(claim_window, remainder_window, 200_000, start, false);
        claim.contract.lock();
        claim.env.ledger().set_timestamp(start + elapsed);
        claim.contract.release_advance_1();
        if prefix_claims {
            claim.contract.claim_advance_1();
        }
        claim.contract.mark_checkpoint();
        claim.contract.release_advance_2();
        let deadline = claim.contract.get_commitment().advance2_deadline;
        claim.env.ledger().set_timestamp(around(deadline, offset));
        let claim_succeeds = claim.contract.try_claim_advance_2().is_ok();

        let reclaim = scene(claim_window, remainder_window, 200_000, start, false);
        reclaim.contract.lock();
        reclaim.env.ledger().set_timestamp(start + elapsed);
        reclaim.contract.release_advance_1();
        if prefix_claims {
            reclaim.contract.claim_advance_1();
        }
        reclaim.contract.mark_checkpoint();
        reclaim.contract.release_advance_2();
        reclaim.env.ledger().set_timestamp(around(deadline, offset));
        let reclaim_succeeds = reclaim.contract.try_reclaim_advance_2().is_ok();

        prop_assert_ne!(claim_succeeds, reclaim_succeeds);
    }

    #[test]
    fn delivery_confirmation_and_nondelivery_reclaim_are_exclusive(
        claim_window in arb_window(),
        remainder_window in arb_window(),
        delivery_window in 200_000u64..1_000_000,
        start in arb_start(),
        elapsed in arb_prefix_elapsed(),
        offset in arb_offset(),
        prefix_claims in any::<bool>(),
    ) {
        let confirmed = scene(claim_window, remainder_window, delivery_window, start, false);
        ready_and_funded(&confirmed, prefix_claims, elapsed);
        let deadline = confirmed.contract.get_commitment().delivery_deadline;
        confirmed.env.ledger().set_timestamp(around(deadline, offset));
        let confirm_succeeds = confirmed.contract
            .try_confirm_delivery(&CONTRACTED_QUANTITY, &0).is_ok();

        let reclaimed = scene(claim_window, remainder_window, delivery_window, start, false);
        ready_and_funded(&reclaimed, prefix_claims, elapsed);
        reclaimed.env.ledger().set_timestamp(around(deadline, offset));
        let reclaim_succeeds = reclaimed.contract.try_reclaim_on_nondelivery().is_ok();

        prop_assert_ne!(confirm_succeeds, reclaim_succeeds);
    }
}

#[test]
#[ignore = "Known overlap; precedence is tracked in https://github.com/Agrisettle/HarvestLock-Contracts/issues/348"]
fn remainder_expiry_and_nondelivery_reclaim_are_exclusive() {
    let defaulted = scene(10, 20, 10, 1_000, false);
    ready_for_delivery(&defaulted, false, 11);
    let remainder_deadline = defaulted.contract.get_commitment().remainder_deadline;
    let delivery_deadline = defaulted.contract.get_commitment().delivery_deadline;
    let now = remainder_deadline.max(delivery_deadline) + 1;
    defaulted.env.ledger().set_timestamp(now);
    let default_succeeds = defaulted.contract.try_expire_remainder_window().is_ok();

    let forfeited = scene(10, 20, 10, 1_000, false);
    ready_for_delivery(&forfeited, false, 11);
    forfeited.env.ledger().set_timestamp(now);
    let forfeiture_succeeds = forfeited.contract.try_reclaim_on_nondelivery().is_ok();

    assert_ne!(default_succeeds, forfeiture_succeeds);
}

#[test]
#[ignore = "Known overlap; precedence is tracked in https://github.com/Agrisettle/HarvestLock-Contracts/issues/348"]
fn dispute_resolution_and_expiry_are_exclusive() {
    let resolved = scene(10, 20, 100, 1_000, false);
    ready_for_delivery(&resolved, false, 0);
    resolved.contract.flag_dispute(&resolved.buyer);
    let deadline = resolved.contract.get_commitment().dispute_deadline;
    resolved.env.ledger().set_timestamp(deadline + 1);
    let resolution_succeeds = resolved.contract.try_resolve_dispute().is_ok();

    let expired = scene(10, 20, 100, 1_000, false);
    ready_for_delivery(&expired, false, 0);
    expired.contract.flag_dispute(&expired.buyer);
    expired.env.ledger().set_timestamp(deadline + 1);
    let expiry_succeeds = expired.contract.try_expire_dispute_window().is_ok();

    assert_ne!(resolution_succeeds, expiry_succeeds);
}

#[test]
fn delivery_boundary_fix_keeps_the_late_attestation_exclusive() {
    let confirmed = scene(10, 20, 100, 1_000, false);
    ready_and_funded(&confirmed, false, 0);
    let deadline = confirmed.contract.get_commitment().delivery_deadline;
    confirmed.env.ledger().set_timestamp(deadline + 1);
    assert!(confirmed
        .contract
        .try_confirm_delivery(&CONTRACTED_QUANTITY, &0)
        .is_err());
    assert!(confirmed.contract.try_reclaim_on_nondelivery().is_ok());
    assert_eq!(confirmed.contract.get_status(), Status::Forfeited);
}
