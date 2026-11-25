//! #259 — pin where the deposit's rounding dust ends up.
//!
//! `deposit_amount` escrows `floor(T * (a1 + a2) / 10_000)` once, while the
//! two tranche payouts are `floor(T * a1 / 10_000)` and
//! `floor(T * a2 / 10_000)` separately. With an odd, small total those can
//! differ by one unit:
//!
//!   T = 199, a1 = a2 = 1_500 bps
//!   deposit   = floor(199 * 3_000 / 10_000) = 59
//!   adv1      = floor(199 * 1_500 / 10_000) = 29
//!   adv2      = floor(199 * 1_500 / 10_000) = 29
//!   dust      = 59 - (29 + 29)          = 1
//!
//! That unit is not paid out by a tranche, so it stays in the contract's
//! balance and is allocated by whichever terminal path runs. Each test
//! withdraws the resulting credit and asserts which party receives the dust.

use harvestlock_escrow::{
    CommitmentTerms, Error, EscrowContract, EscrowContractClient, OracleConfig, Status,
};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, Env, Symbol, Vec};

const WINDOW: u64 = 7 * 24 * 60 * 60;
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
const CONTRACTED_QUANTITY: u32 = 1_000;
const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];

/// The odd total this suite is built around; see the module comment.
const TOTAL: i128 = 199;
const ADVANCE_BPS: u32 = 1_500;

struct World {
    env: Env,
    contract: EscrowContractClient<'static>,
    token: token::Client<'static>,
    buyer: Address,
    cooperative: Address,
}

fn world() -> World {
    let env = Env::default();
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
            advance1_bps: ADVANCE_BPS,
            advance2_bps: ADVANCE_BPS,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &None,
    );

    World {
        env,
        contract,
        token,
        buyer,
        cooperative,
    }
}

impl World {
    /// ReadyForDelivery with both advances claimed and the remainder
    /// unfunded — contract balance is exactly the one-unit dust.
    fn ready_with_both_advances_claimed(&self) {
        self.contract.lock();
        self.contract.release_advance_1();
        self.contract.claim_advance_1();
        self.contract.mark_checkpoint();
        self.contract.release_advance_2();
        self.contract.claim_advance_2();
        self.contract.ready_for_delivery();
    }

    fn advance(&self, secs: u64) {
        let now = self.env.ledger().timestamp();
        self.env.ledger().set_timestamp(now + secs);
    }
}

#[test]
fn the_fixture_really_leaves_one_unit_of_dust() {
    let w = world();
    w.ready_with_both_advances_claimed();

    // The cooperative has 29 + 29 and the contract still holds 1: the
    // deposit floors 59 once, the tranches floor to 29 each.
    assert_eq!(w.token.balance(&w.cooperative), 58);
    assert_eq!(w.token.balance(&w.contract.address), 1);
    assert_eq!(w.token.balance(&w.buyer), 140);
}

#[test]
fn cancel_refunds_the_dust_to_the_buyer() {
    let w = world();
    w.ready_with_both_advances_claimed();
    assert_eq!(w.token.balance(&w.contract.address), 1);

    w.contract.cancel();
    w.contract.withdraw_payout(&w.buyer);

    assert_eq!(w.contract.get_status(), Status::Cancelled);
    assert_eq!(w.token.balance(&w.contract.address), 0);
    // 140 unspent + the 1-unit dust, all refunded.
    assert_eq!(w.token.balance(&w.buyer), 141, "the dust is refunded");
    assert_eq!(w.token.balance(&w.cooperative), 58);
}

#[test]
fn expire_remainder_window_sweeps_the_dust_to_the_cooperative() {
    let w = world();
    w.ready_with_both_advances_claimed();
    w.advance(REMAINDER_WINDOW + 1);

    w.contract.expire_remainder_window();
    w.contract.withdraw_payout(&w.cooperative);

    assert_eq!(w.contract.get_status(), Status::Defaulted);
    assert_eq!(w.token.balance(&w.contract.address), 0);
    assert_eq!(w.token.balance(&w.cooperative), 59, "58 claimed + 1 dust");
    assert_eq!(w.token.balance(&w.buyer), 140);
}

#[test]
fn reclaim_on_nondelivery_refunds_the_dust_to_the_buyer() {
    let w = world();
    w.ready_with_both_advances_claimed();
    w.advance(DELIVERY_WINDOW + 1);

    w.contract.reclaim_on_nondelivery();
    w.contract.withdraw_payout(&w.buyer);

    assert_eq!(w.contract.get_status(), Status::Forfeited);
    assert_eq!(w.token.balance(&w.contract.address), 0);
    assert_eq!(w.token.balance(&w.buyer), 141, "the dust is refunded");
    assert_eq!(w.token.balance(&w.cooperative), 58);
}

#[test]
fn settle_when_the_obligation_exceeds_the_balance_pays_the_dust_to_the_cooperative() {
    let w = world();
    w.ready_with_both_advances_claimed();
    w.contract.fund_remainder();
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    w.contract.settle();
    w.contract.withdraw_payout(&w.cooperative);

    assert_eq!(w.contract.get_status(), Status::Settled);
    assert_eq!(w.token.balance(&w.contract.address), 0);
    assert_eq!(
        w.token.balance(&w.cooperative),
        TOTAL,
        "the full obligation swallows the balance, dust included"
    );
    assert_eq!(w.token.balance(&w.buyer), 0);
}

#[test]
fn settle_after_a_partial_delivery_refunds_the_dust_to_the_buyer() {
    let w = world();
    w.ready_with_both_advances_claimed();
    w.contract.fund_remainder();
    // Half the contracted quantity at full grade -> settlement_bps 5_000.
    w.contract.confirm_delivery(&(CONTRACTED_QUANTITY / 2), &0);
    w.contract.settle();
    w.contract.withdraw_payout(&w.buyer);
    w.contract.withdraw_payout(&w.cooperative);

    assert_eq!(w.contract.get_status(), Status::Settled);
    assert_eq!(w.token.balance(&w.contract.address), 0);
    // floor(199 * 5_000 / 10_000) = 99 to the cooperative in total
    // (58 advances + 41 from the balance); everything else, dust
    // included, returns to the buyer.
    assert_eq!(w.token.balance(&w.cooperative), 99);
    assert_eq!(w.token.balance(&w.buyer), 100);
}

#[test]
fn expire_fx_shortfall_window_sweeps_the_dust_to_the_cooperative() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    let token = token::Client::new(&env, &token_address);
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &TOTAL);

    let oracle_id = env.register(mock_oracle::MockOracle, ());
    let oracle = mock_oracle::MockOracleClient::new(&env, &oracle_id);
    oracle.set_price(&175_000_000_000_000i128, &env.ledger().timestamp());

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &CommitmentTerms {
            total_amount: TOTAL,
            advance1_bps: ADVANCE_BPS,
            advance2_bps: ADVANCE_BPS,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &Some(OracleConfig {
            oracle_contract: oracle_id,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 3_600,
            // 200 fiat units x 1.75 = 350 settlement units, well over the
            // 199 escrowed, so resolution finds a real shortfall.
            denominated_amount: 200,
        }),
    );

    contract.lock();
    contract.release_advance_1();
    contract.claim_advance_1();
    contract.mark_checkpoint();
    contract.release_advance_2();
    contract.claim_advance_2();
    contract.ready_for_delivery();
    contract.fund_remainder();
    contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    contract.resolve_fx_shortfall();
    assert!(
        contract.get_commitment().fx_shortfall_amount > 0,
        "fixture must produce a shortfall for this path to be reachable"
    );
    assert_eq!(contract.try_settle(), Err(Ok(Error::FxShortfallUnfunded)));

    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + REMAINDER_WINDOW + 1);
    contract.expire_fx_shortfall_window();
    contract.withdraw_payout(&cooperative);

    assert_eq!(contract.get_status(), Status::Defaulted);
    assert_eq!(token.balance(&contract.address), 0);
    assert_eq!(
        token.balance(&cooperative),
        TOTAL,
        "58 claimed + 1 dust + 140 remainder"
    );
    assert_eq!(token.balance(&buyer), 0);
}

/// The mock oracle used by the FX-expiry path. Kept local to this file so the
/// suite doesn't depend on the in-crate test module.
mod mock_oracle {
    use soroban_sdk::{contract, contractimpl, contracttype, Env, Symbol};

    #[contracttype]
    #[derive(Clone)]
    pub struct PriceData {
        pub price: i128,
        pub timestamp: u64,
    }

    #[contracttype]
    #[derive(Clone)]
    pub enum Asset {
        Stellar(soroban_sdk::Address),
        Other(Symbol),
    }

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn set_price(env: Env, price: i128, timestamp: u64) {
            env.storage()
                .instance()
                .set(&soroban_sdk::symbol_short!("price"), &(price, timestamp));
        }

        pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
            env.storage()
                .instance()
                .get::<_, (i128, u64)>(&soroban_sdk::symbol_short!("price"))
                .map(|(price, timestamp)| PriceData { price, timestamp })
        }

        pub fn decimals(_env: Env) -> u32 {
            14
        }
    }
}
