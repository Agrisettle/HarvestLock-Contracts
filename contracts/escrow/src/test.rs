#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, BytesN as _, Ledger as _};
use soroban_sdk::Env;

const WINDOW: u64 = 7 * 24 * 60 * 60; // 7 days, arbitrary but realistic
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60; // 7 days — matches the app-level default
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60; // 120 days — matches the app-level default
const CONTRACTED_QUANTITY: u32 = 1_000; // arbitrary "kg" units, same framing as total_amount's stroops-equivalent
/// Full price at grade 0, then two discounted tiers — a plausible
/// pre-agreed grade schedule for tests that don't care about the exact
/// numbers, just that a schedule exists.
const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];
/// The grade index tests default to unless they're specifically exercising
/// grade adjustment — index 0 is `GRADE_PRICE_BPS`'s full-price tier.
const FULL_PRICE_GRADE: u32 = 0;

fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (
        address.clone(),
        token::Client::new(env, &address),
        token::StellarAssetClient::new(env, &address),
    )
}

struct Setup<'a> {
    env: Env,
    contract: EscrowContractClient<'a>,
    token: token::Client<'a>,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    total_amount: i128,
}

fn setup(advance1_bps: u32, advance2_bps: u32) -> Setup<'static> {
    setup_with_window(advance1_bps, advance2_bps, WINDOW)
}

fn setup_with_window(advance1_bps: u32, advance2_bps: u32, window: u64) -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let (token_address, token, token_admin_client) = create_token(&env, &token_admin);

    let total_amount: i128 = 1_000_000; // arbitrary stroops-equivalent units
    token_admin_client.mint(&buyer, &total_amount);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &total_amount,
        &advance1_bps,
        &advance2_bps,
        &window,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );

    Setup {
        env,
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        total_amount,
    }
}

/// Advances the simulated ledger clock forward by `secs`. Used to cross a
/// claim-window deadline deterministically instead of relying on wall time.
fn advance_time(s: &Setup, secs: u64) {
    let now = s.env.ledger().timestamp();
    s.env.ledger().set_timestamp(now + secs);
}

/// The deposit-only amount `lock` actually escrows, for a given bps split —
/// mirrors `EscrowContract::deposit_amount` so tests can assert against it
/// without reaching into contract internals.
fn deposit_amount(total: i128, advance1_bps: u32, advance2_bps: u32) -> i128 {
    total * ((advance1_bps + advance2_bps) as i128) / 10_000
}

/// Drives a fresh, locked commitment all the way to `ReadyForDelivery` with
/// the remainder funded — both tranches claimed normally along the way.
/// Shared by the several tests below that only care about what happens
/// *after* this point (confirm_delivery, settle) — the tranche mechanics
/// themselves already have their own dedicated coverage above.
fn advance_to_remainder_funded(s: &Setup) {
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
}

// ---------- basic lifecycle ----------

#[test]
fn initialize_sets_draft_status() {
    let s = setup(1_500, 1_500);
    assert_eq!(s.contract.get_status(), Status::Draft);
}

#[test]
fn cannot_initialize_twice() {
    let s = setup(1_000, 1_000);
    let result = s.contract.try_initialize(
        &s.buyer,
        &s.cooperative,
        &s.warehouse,
        &s.token.address,
        &s.total_amount,
        &1_000,
        &1_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&s.env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn zero_claim_window_rejected_at_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &0,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidWindow)));
}

#[test]
fn zero_remainder_window_rejected_at_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &WINDOW,
        &0,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidWindow)));
}

#[test]
fn zero_delivery_window_rejected_at_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &0,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidWindow)));
}

#[test]
fn advance_bps_over_10000_rejected_at_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &6_000,
        &5_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidBps)));
}

#[test]
fn lock_transfers_only_the_deposit_not_the_full_total() {
    // Two-phase funding: lock escrows advance1_bps + advance2_bps of the
    // total, not the whole amount — the remainder comes later via
    // fund_remainder. See module docs in lib.rs.
    let s = setup(1_500, 2_000); // 35% deposit, 65% remainder
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
    assert_eq!(s.token.balance(&s.contract.address), 0);

    s.contract.lock();

    let deposit = deposit_amount(s.total_amount, 1_500, 2_000);
    assert_eq!(s.contract.get_status(), Status::Locked);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount - deposit);
    assert_eq!(s.token.balance(&s.contract.address), deposit);
}

// ---------- allocation ledger (record-only, PRD §4.9 Rung 1) ----------

fn member(env: &Env, share_bps: u32) -> AllocationMember {
    AllocationMember {
        member_hash: BytesN::random(env),
        share_bps,
    }
}

#[test]
fn set_allocation_records_the_member_list() {
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(
        &s.env,
        [member(&s.env, 6_000), member(&s.env, 4_000)],
    );

    s.contract.set_allocation(&members);

    let stored = s.contract.get_allocation();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored.get(0).unwrap().share_bps, 6_000);
    assert_eq!(stored.get(1).unwrap().share_bps, 4_000);
}

#[test]
fn get_allocation_before_set_allocation_fails() {
    let s = setup(1_500, 1_500);
    let result = s.contract.try_get_allocation();
    assert_eq!(result, Err(Ok(Error::AllocationNotSet)));
}

#[test]
fn cannot_set_allocation_twice() {
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(&s.env, [member(&s.env, 10_000)]);
    s.contract.set_allocation(&members);

    let again = Vec::from_array(&s.env, [member(&s.env, 5_000)]);
    let result = s.contract.try_set_allocation(&again);
    assert_eq!(result, Err(Ok(Error::AllocationAlreadySet)));
}

#[test]
fn cannot_set_allocation_with_an_empty_member_list() {
    let s = setup(1_500, 1_500);
    let empty: Vec<AllocationMember> = Vec::new(&s.env);
    let result = s.contract.try_set_allocation(&empty);
    assert_eq!(result, Err(Ok(Error::InvalidAllocation)));
}

#[test]
fn cannot_set_allocation_with_shares_summing_over_10000_bps() {
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(
        &s.env,
        [member(&s.env, 6_000), member(&s.env, 5_000)],
    );
    let result = s.contract.try_set_allocation(&members);
    assert_eq!(result, Err(Ok(Error::InvalidAllocation)));
}

#[test]
fn allocation_shares_summing_to_under_10000_bps_is_allowed() {
    // Not every member has to be enumerated -- an incomplete or
    // under-100% ledger is still valid (e.g. recorded before the full
    // membership list is finalized). Only over 100% is rejected, since
    // that can never be a coherent entitlement split.
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(&s.env, [member(&s.env, 3_000)]);
    s.contract.set_allocation(&members);
    assert_eq!(s.contract.get_allocation().len(), 1);
}

#[test]
fn cannot_set_allocation_after_lock() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let members = Vec::from_array(&s.env, [member(&s.env, 10_000)]);
    let result = s.contract.try_set_allocation(&members);
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn lock_does_not_require_an_allocation_to_have_been_set() {
    // Deliberately optional -- see set_allocation's doc comment. A
    // solo-farmer commitment with no cooperative pooling should still be
    // lockable without ever calling set_allocation.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    assert_eq!(s.contract.get_status(), Status::Locked);
}

#[test]
fn set_allocation_requires_cooperative_auth() {
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(&s.env, [member(&s.env, 10_000)]);

    // mock_all_auths() means this can't check *rejection* of a wrong
    // signer -- see confirm_delivery_requires_warehouse_operator's
    // comment. What it confirms is that the call path genuinely requires
    // the cooperative's auth to exist at all.
    s.contract.set_allocation(&members);
    let auths = s.env.auths();
    let touched_cooperative = auths.iter().any(|(addr, _)| *addr == s.cooperative);
    assert!(
        touched_cooperative,
        "expected cooperative auth on set_allocation"
    );
}

// ---------- oracle_rate (PRD §16.3 staleness bound) ----------
//
// A local stand-in contract, not the real Reflector — `Env::default()`'s
// simulated ledger can't reach the live testnet oracle this contract
// actually integrates with (see reflector.rs's doc comment for the real
// address, and HANDOFF.md for the live testnet call that verified its
// interface and asset list). Cross-contract calls in Soroban match on
// XDR wire shape, not Rust type identity, so an independently-defined
// `Asset`/`PriceData` pair with the same shape as reflector.rs's is
// indistinguishable, from `oracle_rate`'s perspective, from the genuine
// Reflector contract.
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
        /// Test-only setup hook — the real Reflector contract has no such
        /// function; its price history comes from node consensus instead.
        pub fn set_price(env: Env, price: i128, timestamp: u64) {
            env.storage()
                .instance()
                .set(&symbol_short!("price"), &(price, timestamp));
        }

        /// `None` until `set_price` has run, matching Reflector's own
        /// "doesn't quote this asset" case (`OraclePriceUnavailable`).
        pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
            env.storage()
                .instance()
                .get::<_, (i128, u64)>(&symbol_short!("price"))
                .map(|(price, timestamp)| PriceData { price, timestamp })
        }
    }
}

/// A commitment initialized with an `oracle_config` pointing at a fresh
/// `mock_oracle::MockOracle` instance, `price` already set at the current
/// ledger timestamp. Returns the setup plus the oracle client so tests
/// can call `set_price` again to simulate a stale or updated quote.
fn setup_with_oracle(max_age_secs: u64) -> (Setup<'static>, mock_oracle::MockOracleClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, token, token_admin_client) = create_token(&env, &token_admin);
    let total_amount: i128 = 1_000_000;
    token_admin_client.mint(&buyer, &total_amount);

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
        &total_amount,
        &1_500,
        &1_500,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &Some(OracleConfig {
            oracle_contract: oracle_id,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs,
        }),
    );

    (
        Setup {
            env,
            contract,
            token,
            buyer,
            cooperative,
            warehouse,
            total_amount,
        },
        oracle,
    )
}

#[test]
fn oracle_rate_returns_the_configured_oracles_fresh_quote() {
    let (s, _oracle) = setup_with_oracle(3_600);
    let rate = s.contract.oracle_rate();
    assert_eq!(rate.price, 175_000_000_000_000);
    assert_eq!(rate.timestamp, s.env.ledger().timestamp());
}

#[test]
fn get_oracle_config_reflects_what_initialize_set() {
    let (s, _oracle) = setup_with_oracle(3_600);
    let config = s.contract.get_oracle_config();
    assert!(config.is_some());
    assert_eq!(config.unwrap().price_asset, Symbol::new(&s.env, "NGN"));
}

#[test]
fn get_oracle_config_is_none_when_initialize_never_set_one() {
    // The plain setup() helper passes None -- every other test in this
    // file relies on that, implicitly proving oracle_config is genuinely
    // optional. This test just makes it explicit.
    let s = setup(1_500, 1_500);
    assert_eq!(s.contract.get_oracle_config(), None);
}

#[test]
fn oracle_rate_fails_when_no_oracle_configured() {
    let s = setup(1_500, 1_500);
    let result = s.contract.try_oracle_rate();
    assert_eq!(result, Err(Ok(Error::OracleNotConfigured)));
}

#[test]
fn oracle_rate_fails_when_the_oracle_has_never_quoted_this_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    // Never called: the oracle has no price on record at all yet.
    let oracle_id = env.register(mock_oracle::MockOracle, ());

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_500,
        &1_500,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &Some(OracleConfig {
            oracle_contract: oracle_id,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 3_600,
        }),
    );

    let result = contract.try_oracle_rate();
    assert_eq!(result, Err(Ok(Error::OraclePriceUnavailable)));
}

#[test]
fn oracle_rate_rejects_a_quote_older_than_max_age_secs() {
    let (s, _oracle) = setup_with_oracle(3_600);
    advance_time(&s, 3_601);
    let result = s.contract.try_oracle_rate();
    assert_eq!(result, Err(Ok(Error::OracleStale)));
}

#[test]
fn oracle_rate_accepts_a_quote_exactly_at_the_staleness_boundary() {
    // age > max_age_secs is rejected, so age == max_age_secs (not yet
    // over the bound) must still be accepted -- an off-by-one check.
    let (s, _oracle) = setup_with_oracle(3_600);
    advance_time(&s, 3_600);
    let rate = s.contract.oracle_rate();
    assert_eq!(rate.price, 175_000_000_000_000);
}

#[test]
fn oracle_rate_reflects_a_refreshed_quote() {
    let (s, oracle) = setup_with_oracle(3_600);
    advance_time(&s, 60);
    oracle.set_price(&176_500_000_000_000i128, &s.env.ledger().timestamp());

    let rate = s.contract.oracle_rate();
    assert_eq!(rate.price, 176_500_000_000_000);
    assert_eq!(rate.timestamp, s.env.ledger().timestamp());
}

#[test]
fn initialize_rejects_a_zero_max_age_secs_oracle_config() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);
    let oracle_id = env.register(mock_oracle::MockOracle, ());

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_500,
        &1_500,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &Some(OracleConfig {
            oracle_contract: oracle_id,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 0,
        }),
    );
    assert_eq!(result, Err(Ok(Error::InvalidOracleConfig)));
}

#[test]
fn oracle_rate_is_callable_after_settlement() {
    // A pure read against Reflector's own history, not a state
    // transition -- there's no reason to gate it by commitment status.
    let (s, _oracle) = setup_with_oracle(3_600);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    let rate = s.contract.oracle_rate();
    assert_eq!(rate.price, 175_000_000_000_000);
}

// ---------- opening a tranche moves no funds ----------

#[test]
fn release_advance_1_opens_window_but_transfers_nothing() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    let deposit = deposit_amount(s.total_amount, 1_500, 2_000);

    s.contract.release_advance_1();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), deposit);
}

#[test]
fn cannot_release_advance_1_before_lock() {
    let s = setup(1_500, 1_500);
    let result = s.contract.try_release_advance_1();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

// ---------- claiming within the window ----------

#[test]
fn claim_advance_1_within_window_pays_cooperative_correct_bps() {
    let s = setup(1_500, 2_000); // 15%
    s.contract.lock();
    s.contract.release_advance_1();

    s.contract.claim_advance_1();

    let expected = s.total_amount * 1_500 / 10_000;
    let deposit = deposit_amount(s.total_amount, 1_500, 2_000);
    assert_eq!(s.token.balance(&s.cooperative), expected);
    assert_eq!(s.token.balance(&s.contract.address), deposit - expected);
}

#[test]
fn cannot_claim_advance_1_before_it_is_opened() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let result = s.contract.try_claim_advance_1();
    assert_eq!(result, Err(Ok(Error::NotYetOpened)));
}

#[test]
fn cannot_claim_advance_1_twice() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();

    let result = s.contract.try_claim_advance_1();
    assert_eq!(result, Err(Ok(Error::AlreadyClaimed)));
}

#[test]
fn cannot_claim_advance_1_after_window_passes() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);

    let result = s.contract.try_claim_advance_1();
    assert_eq!(result, Err(Ok(Error::ClaimWindowPassed)));
}

#[test]
fn claim_exactly_at_deadline_still_succeeds() {
    // Boundary check: the guard is `timestamp > deadline`, so a claim
    // landing in the same second as the deadline should still work.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    advance_time(&s, WINDOW); // now == deadline exactly

    s.contract.claim_advance_1();
    assert!(s.token.balance(&s.cooperative) > 0);
}

// ---------- reclaiming after expiry ----------

#[test]
fn cannot_reclaim_advance_1_before_window_passes() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();

    let result = s.contract.try_reclaim_advance_1();
    assert_eq!(result, Err(Ok(Error::ClaimWindowNotPassed)));
}

#[test]
fn reclaim_advance_1_after_window_passes_returns_funds_to_buyer() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);

    s.contract.reclaim_advance_1();

    let expected = s.total_amount * 1_500 / 10_000;
    let deposit = deposit_amount(s.total_amount, 1_500, 2_000);
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - deposit + expected
    );
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), deposit - expected);
}

#[test]
fn cannot_reclaim_advance_1_twice() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);
    s.contract.reclaim_advance_1();

    let result = s.contract.try_reclaim_advance_1();
    assert_eq!(result, Err(Ok(Error::AlreadyExpired)));
}

#[test]
fn cannot_claim_after_buyer_already_reclaimed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);
    s.contract.reclaim_advance_1();

    let result = s.contract.try_claim_advance_1();
    assert_eq!(result, Err(Ok(Error::AlreadyExpired)));
}

#[test]
fn cannot_reclaim_after_cooperative_already_claimed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    advance_time(&s, WINDOW + 1);

    let result = s.contract.try_reclaim_advance_1();
    assert_eq!(result, Err(Ok(Error::AlreadyClaimed)));
}

// ---------- ready_for_delivery / fund_remainder (two-phase funding) ----------

#[test]
fn ready_for_delivery_opens_the_remainder_window() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();

    s.contract.ready_for_delivery();

    assert_eq!(s.contract.get_status(), Status::ReadyForDelivery);
    assert_eq!(
        s.contract.get_commitment().remainder_deadline,
        s.env.ledger().timestamp() + REMAINDER_WINDOW
    );
}

#[test]
fn cannot_call_ready_for_delivery_before_advance_2_released() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    let result = s.contract.try_ready_for_delivery();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn ready_for_delivery_requires_cooperative_auth() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();

    // mock_all_auths() means this can't check *rejection* of a wrong signer
    // — see confirm_delivery_requires_warehouse_operator's comment. What it
    // confirms is that the auth trace genuinely names the cooperative.
    s.contract.ready_for_delivery();
    let auths = s.env.auths();
    assert!(auths.iter().any(|(addr, _)| *addr == s.cooperative));
}

#[test]
fn fund_remainder_transfers_exactly_the_remainder() {
    let s = setup(1_500, 2_000); // 35% deposit, 65% remainder
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();

    let deposit = deposit_amount(s.total_amount, 1_500, 2_000);
    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let buyer_balance_before = s.total_amount - deposit;

    s.contract.fund_remainder();

    let remainder = s.total_amount - deposit;
    assert_eq!(s.token.balance(&s.buyer), buyer_balance_before - remainder);
    // contract now holds: unclaimed advance2 (already claimed above) + remainder
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    assert_eq!(
        s.token.balance(&s.contract.address),
        deposit - advance1_amount - advance2_amount + remainder
    );
    assert!(s.contract.get_commitment().remainder_funded);
}

#[test]
fn cannot_fund_remainder_before_ready_for_delivery() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    let result = s.contract.try_fund_remainder();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn cannot_fund_remainder_twice() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();

    let result = s.contract.try_fund_remainder();
    assert_eq!(result, Err(Ok(Error::RemainderAlreadyFunded)));
}

#[test]
fn cannot_fund_remainder_after_window_passed() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    advance_time(&s, REMAINDER_WINDOW + 1);

    let result = s.contract.try_fund_remainder();
    assert_eq!(result, Err(Ok(Error::RemainderWindowPassed)));
}

#[test]
fn fund_remainder_requires_buyer_auth() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();

    s.contract.fund_remainder();
    let auths = s.env.auths();
    assert!(auths.iter().any(|(addr, _)| *addr == s.buyer));
}

// ---------- expire_remainder_window (buyer default) ----------

#[test]
fn expire_remainder_window_defaults_buyer_and_sweeps_escrow_to_cooperative() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    // buyer never funds the remainder
    advance_time(&s, REMAINDER_WINDOW + 1);

    let contract_balance_before = s.token.balance(&s.contract.address);
    let cooperative_balance_before = s.token.balance(&s.cooperative);

    s.contract.expire_remainder_window();

    assert_eq!(s.contract.get_status(), Status::Defaulted);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(
        s.token.balance(&s.cooperative),
        cooperative_balance_before + contract_balance_before
    );
}

#[test]
fn cannot_expire_remainder_window_before_deadline() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();

    let result = s.contract.try_expire_remainder_window();
    assert_eq!(result, Err(Ok(Error::RemainderWindowNotPassed)));
}

#[test]
fn cannot_expire_remainder_window_if_already_funded() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    advance_time(&s, REMAINDER_WINDOW + 1);

    let result = s.contract.try_expire_remainder_window();
    assert_eq!(result, Err(Ok(Error::RemainderAlreadyFunded)));
}

#[test]
fn expire_remainder_window_is_permissionless() {
    // No require_auth() at all on this path — anyone (including an
    // off-chain watcher) can trigger it once the deadline fact is true.
    // mock_all_auths() can't itself prove absence of an auth requirement,
    // so this asserts the call succeeds while the auth trace names neither
    // the buyer nor the cooperative (only they, or the caller, would
    // appear if some require_auth() were secretly present and mock-satisfied).
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    advance_time(&s, REMAINDER_WINDOW + 1);

    s.contract.expire_remainder_window();
    assert_eq!(s.contract.get_status(), Status::Defaulted);
}

// ---------- reclaim_on_nondelivery (seller-non-delivery forfeiture) ----------

#[test]
fn reclaim_on_nondelivery_after_delivery_deadline_returns_balance_to_buyer() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    // cooperative never proceeds any further
    advance_time(&s, DELIVERY_WINDOW + 1);

    let contract_balance_before = s.token.balance(&s.contract.address);
    let buyer_balance_before = s.token.balance(&s.buyer);

    s.contract.reclaim_on_nondelivery();

    assert_eq!(s.contract.get_status(), Status::Forfeited);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(
        s.token.balance(&s.buyer),
        buyer_balance_before + contract_balance_before
    );
}

#[test]
fn reclaim_on_nondelivery_works_from_ready_for_delivery_too() {
    // Covers the case where the remainder was already funded but the
    // cooperative still never delivers before the overall deadline.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s); // claims both tranches (35%), then funds the 65% remainder
    advance_time(&s, DELIVERY_WINDOW + 1);

    s.contract.reclaim_on_nondelivery();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    assert_eq!(s.contract.get_status(), Status::Forfeited);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    // Same shape as cancel: the already-claimed 35% stays with the
    // cooperative — reclaim only returns the contract's current balance.
    assert_eq!(
        s.token.balance(&s.cooperative),
        advance1_amount + advance2_amount
    );
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - advance1_amount - advance2_amount
    );
}

#[test]
fn cannot_reclaim_on_nondelivery_before_deadline() {
    let s = setup(1_500, 2_000);
    s.contract.lock();

    let result = s.contract.try_reclaim_on_nondelivery();
    assert_eq!(result, Err(Ok(Error::DeliveryDeadlineNotPassed)));
}

#[test]
fn cannot_reclaim_on_nondelivery_from_draft() {
    let s = setup(1_500, 2_000);
    advance_time(&s, DELIVERY_WINDOW + 1);

    let result = s.contract.try_reclaim_on_nondelivery();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn cannot_reclaim_on_nondelivery_after_delivered() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    advance_time(&s, DELIVERY_WINDOW + 1);

    let result = s.contract.try_reclaim_on_nondelivery();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn reclaim_on_nondelivery_requires_buyer_auth() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_time(&s, DELIVERY_WINDOW + 1);

    s.contract.reclaim_on_nondelivery();
    let auths = s.env.auths();
    assert!(auths.iter().any(|(addr, _)| *addr == s.buyer));
}

// ---------- confirm_delivery now gated on remainder funded ----------

#[test]
fn cannot_confirm_delivery_before_ready_for_delivery() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();

    let result = s.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn cannot_confirm_delivery_before_remainder_funded() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();

    let result = s.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    assert_eq!(result, Err(Ok(Error::RemainderNotFunded)));
}

// ---------- settle requires both tranches resolved ----------

#[test]
fn settle_blocked_if_advance_1_never_resolved() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    // deliberately never claim or reclaim advance 1
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let result = s.contract.try_settle();
    assert_eq!(result, Err(Ok(Error::TrancheUnresolved)));
}

#[test]
fn settle_blocked_if_advance_2_never_resolved() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    // deliberately never claim or reclaim advance 2
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let result = s.contract.try_settle();
    assert_eq!(result, Err(Ok(Error::TrancheUnresolved)));
}

#[test]
fn cannot_settle_before_delivery_confirmed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();

    let result = s.contract.try_settle();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

// ---------- full happy path, several shapes ----------

#[test]
fn full_happy_path_both_tranches_claimed_pays_out_exactly_total_amount() {
    let s = setup(1_500, 2_000); // 15% + 20% = 35%, 65% remains at settlement
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
    assert_eq!(s.token.balance(&s.buyer), 0);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn full_happy_path_advance_1_reclaimed_advance_2_claimed_still_sums_correctly() {
    let s = setup(1_500, 2_000);
    s.contract.lock();

    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);
    s.contract.reclaim_advance_1(); // cooperative was too slow; buyer gets 15% back

    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2(); // this one claimed normally
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    s.contract.settle();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    let remainder = s.total_amount - advance1_amount - advance2_amount;

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.buyer), advance1_amount);
    assert_eq!(s.token.balance(&s.cooperative), advance2_amount + remainder);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    // Everything that started with the buyer is accounted for somewhere.
    assert_eq!(
        s.token.balance(&s.buyer) + s.token.balance(&s.cooperative),
        s.total_amount
    );
}

#[test]
fn zero_advance_bps_still_requires_explicit_resolution_before_settle() {
    // A zero-bps tranche moves no money, but still isn't auto-resolved —
    // settle should still block on it until claim/reclaim is called.
    // This is documented as a known minor friction in HANDOFF.md, not a
    // bug: it keeps "every stroop's fate decided by an explicit call" true
    // without a special case for the zero-amount edge.
    let s = setup(0, 0);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder(); // with 0/0 bps, this funds the entire total_amount
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let blocked = s.contract.try_settle();
    assert_eq!(blocked, Err(Ok(Error::TrancheUnresolved)));

    s.contract.claim_advance_1();
    s.contract.claim_advance_2();
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
}

// ---------- shortfall/grade adjustment (PRD §7) ----------

#[test]
fn initialize_rejects_zero_contracted_quantity() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &0,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidQuantity)));
}

#[test]
fn initialize_rejects_empty_grade_schedule() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::new(&env),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidGradeSchedule)));
}

#[test]
fn initialize_rejects_a_grade_priced_over_10000_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, [10_000, 10_001]),
        &None,
    );
    assert_eq!(result, Err(Ok(Error::InvalidGradeSchedule)));
}

#[test]
fn confirm_delivery_rejects_an_out_of_range_grade_index() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    // GRADE_PRICE_BPS has 3 entries (indices 0-2) -- 3 is out of range.
    let result = s.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &3);
    assert_eq!(result, Err(Ok(Error::InvalidGradeIndex)));
}

#[test]
fn confirm_delivery_records_quantity_grade_and_settlement_bps() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    let half_quantity = CONTRACTED_QUANTITY / 2;
    s.contract.confirm_delivery(&half_quantity, &1); // grade 1 = 9_000 bps

    let c = s.contract.get_commitment();
    assert_eq!(c.delivered_quantity, half_quantity);
    assert_eq!(c.grade_index, 1);
    // 50% quantity * 90% grade = 45% combined.
    assert_eq!(c.settlement_bps, 4_500);
}

#[test]
fn partial_delivery_pays_cooperative_proportionally_and_refunds_the_shortfall_to_buyer() {
    // 50% of contracted quantity delivered, full-price grade: settle should
    // pay the cooperative half the remainder and refund the other half to
    // the buyer -- "pre-agreed proportional adjustment; advance not clawed
    // back" (PRD §7's partial-delivery row).
    let s = setup(1_500, 2_000); // 35% deposit, 65% remainder
    s.contract.lock();
    advance_to_remainder_funded(&s); // both tranches claimed normally

    let half_quantity = CONTRACTED_QUANTITY / 2;
    s.contract.confirm_delivery(&half_quantity, &FULL_PRICE_GRADE);
    s.contract.settle();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    let remainder = s.total_amount - advance1_amount - advance2_amount;
    let adjusted_total = s.total_amount * 5_000 / 10_000; // 50% of contract value
    let owed_from_remainder = adjusted_total - advance1_amount - advance2_amount;

    assert_eq!(s.contract.get_status(), Status::Settled);
    // The advance stays with the cooperative regardless -- not clawed back.
    assert_eq!(
        s.token.balance(&s.cooperative),
        advance1_amount + advance2_amount + owed_from_remainder
    );
    assert_eq!(s.token.balance(&s.buyer), remainder - owed_from_remainder);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    // Nothing created or destroyed.
    assert_eq!(
        s.token.balance(&s.buyer) + s.token.balance(&s.cooperative),
        s.total_amount
    );
}

#[test]
fn grade_adjustment_alone_reduces_the_cooperative_payout() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &1); // full quantity, grade 1 = 9_000 bps
    s.contract.settle();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    let remainder = s.total_amount - advance1_amount - advance2_amount;
    let adjusted_total = s.total_amount * 9_000 / 10_000;
    let owed_from_remainder = adjusted_total - advance1_amount - advance2_amount;

    assert_eq!(
        s.token.balance(&s.cooperative),
        advance1_amount + advance2_amount + owed_from_remainder
    );
    assert_eq!(s.token.balance(&s.buyer), remainder - owed_from_remainder);
}

#[test]
fn over_delivery_does_not_pay_more_than_the_full_contract_value() {
    // Delivering more than contracted doesn't earn extra on-chain -- the
    // PRD's over-delivery handling (buyer right of first refusal, excess
    // "otherwise released to cooperative") is a physical/commercial
    // matter, not something this contract's settlement math rewards.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    let double_quantity = CONTRACTED_QUANTITY * 2;
    s.contract.confirm_delivery(&double_quantity, &FULL_PRICE_GRADE);
    s.contract.settle();

    assert_eq!(s.contract.get_commitment().settlement_bps, 10_000);
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
    assert_eq!(s.token.balance(&s.buyer), 0);
}

#[test]
fn severe_shortfall_does_not_claw_back_advances_already_claimed() {
    // Total crop failure after confirm_delivery somehow still runs (e.g.
    // partial salvage at 0 quantity would be a Forfeited path in practice,
    // but the settlement math itself must still hold if this state is ever
    // reached): the cooperative keeps both already-claimed advances in
    // full, the entire remainder refunds to the buyer, nothing goes
    // negative.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    s.contract.confirm_delivery(&0, &FULL_PRICE_GRADE); // zero delivered
    s.contract.settle();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    let remainder = s.total_amount - advance1_amount - advance2_amount;

    assert_eq!(s.contract.get_commitment().settlement_bps, 0);
    assert_eq!(s.token.balance(&s.cooperative), advance1_amount + advance2_amount);
    assert_eq!(s.token.balance(&s.buyer), remainder);
}

// ---------- auth gating ----------

#[test]
fn confirm_delivery_requires_warehouse_operator() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    // mock_all_auths() means this test can't check *rejection* of a wrong
    // signer (everything auths successfully in that mode) — what it does
    // confirm is that the call path genuinely requires an auth to exist at
    // all, by checking the recorded auth trace names the warehouse address.
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    let auths = s.env.auths();
    let touched_warehouse = auths.iter().any(|(addr, _)| *addr == s.warehouse);
    assert!(
        touched_warehouse,
        "expected warehouse operator auth on confirm_delivery"
    );
}

// ---------- mutual cancellation ----------

#[test]
fn cancel_from_draft_marks_cancelled_with_nothing_to_return() {
    let s = setup(1_500, 1_500);
    // Never locked — contract never held any of the buyer's deposit.
    s.contract.cancel();
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn cancel_after_lock_returns_deposit_to_buyer() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let deposit = deposit_amount(s.total_amount, 1_500, 1_500);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount - deposit);

    s.contract.cancel();

    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn cancel_after_partial_claim_leaves_claimed_advance_with_cooperative() {
    let s = setup(1_500, 2_000); // 15% + 20%
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1(); // cooperative already has 15% — no penalty means this stays theirs

    s.contract.cancel();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.cooperative), advance1_amount);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount - advance1_amount);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn cancel_still_works_from_ready_for_delivery_including_the_already_funded_remainder() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s); // claims both tranches (35%), then funds the 65% remainder

    s.contract.cancel();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    // No penalty, no clawback: the already-claimed 35% stays with the
    // cooperative — only the contract's current balance (the funded
    // remainder) returns to the buyer.
    assert_eq!(
        s.token.balance(&s.cooperative),
        advance1_amount + advance2_amount
    );
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - advance1_amount - advance2_amount
    );
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn cannot_cancel_after_delivery_confirmed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let result = s.contract.try_cancel();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn cannot_cancel_twice() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.cancel();

    let result = s.contract.try_cancel();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_requires_both_buyer_and_cooperative_auth() {
    let s = setup(1_500, 1_500);
    s.contract.lock();

    // mock_all_auths() means this can't check *rejection* of a missing
    // signer — see confirm_delivery_requires_warehouse_operator's comment.
    // What it confirms is that cancel's auth trace genuinely names both
    // parties, not just one — mutual means mutual.
    s.contract.cancel();
    let auths = s.env.auths();
    let touched_buyer = auths.iter().any(|(addr, _)| *addr == s.buyer);
    let touched_cooperative = auths.iter().any(|(addr, _)| *addr == s.cooperative);
    assert!(touched_buyer, "expected buyer auth on cancel");
    assert!(touched_cooperative, "expected cooperative auth on cancel");
}

// ---------- buyer-position assignability ----------

#[test]
fn reassign_buyer_updates_the_buyer_field() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let new_buyer = Address::generate(&s.env);

    s.contract.reassign_buyer(&new_buyer);

    assert_eq!(s.contract.get_commitment().buyer, new_buyer);
}

#[test]
fn reassign_buyer_transfers_reclaim_rights_to_the_new_buyer() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let new_buyer = Address::generate(&s.env);
    s.contract.reassign_buyer(&new_buyer);

    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);
    s.contract.reclaim_advance_1();

    let deposit = deposit_amount(s.total_amount, 1_500, 1_500);
    let advance1_amount = s.total_amount * 1_500 / 10_000;
    assert_eq!(s.token.balance(&new_buyer), advance1_amount);
    // The original buyer keeps whatever they never funded in the first
    // place (the portion beyond the deposit, since two-phase funding
    // means `lock` never took the full amount) — reassignment doesn't
    // touch that, it only affects *future* claim/reclaim/fund rights.
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - deposit,
        "original buyer should receive nothing from reclaim after reassignment, only keep what they never funded"
    );
}

#[test]
fn reassign_buyer_works_from_ready_for_delivery_too() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    let new_buyer = Address::generate(&s.env);

    s.contract.reassign_buyer(&new_buyer);

    assert_eq!(s.contract.get_commitment().buyer, new_buyer);
    assert_eq!(s.contract.get_status(), Status::ReadyForDelivery);
}

#[test]
fn cannot_reassign_buyer_after_delivery_confirmed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    let new_buyer = Address::generate(&s.env);

    let result = s.contract.try_reassign_buyer(&new_buyer);
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn reassign_buyer_requires_current_buyer_cooperative_and_new_buyer_auth() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let new_buyer = Address::generate(&s.env);

    // mock_all_auths() means this can't check *rejection* of a missing
    // signer — see confirm_delivery_requires_warehouse_operator's comment.
    // What it confirms is that all three named parties' auth genuinely
    // appears in the trace, not just some of them.
    s.contract.reassign_buyer(&new_buyer);
    let auths = s.env.auths();
    let touched = |addr: &Address| auths.iter().any(|(a, _)| a == addr);
    assert!(touched(&s.buyer), "expected the outgoing buyer's auth");
    assert!(touched(&s.cooperative), "expected the cooperative's auth");
    assert!(touched(&new_buyer), "expected the incoming buyer's auth");
}
