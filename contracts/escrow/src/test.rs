#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{
    Address as _, BytesN as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke,
};
use soroban_sdk::{xdr, Env, Event as _, IntoVal, Val};

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
    setup_in(
        &env,
        buyer,
        cooperative,
        warehouse,
        advance1_bps,
        advance2_bps,
        window,
    )
}

/// The identity-agnostic half of `setup_with_window` — generates nothing, so
/// tests that need a specific (including degenerate) set of named parties can
/// pass their own addresses in. See `setup_shaped` and the auth-model tests.
fn setup_in(
    env: &Env,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    advance1_bps: u32,
    advance2_bps: u32,
    window: u64,
) -> Setup<'static> {
    let token_admin = Address::generate(env);

    let (token_address, token, token_admin_client) = create_token(env, &token_admin);

    let total_amount: i128 = 1_000_000; // arbitrary stroops-equivalent units
    token_admin_client.mint(&buyer, &total_amount);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(env, &contract_id);

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
        &Vec::from_array(env, GRADE_PRICE_BPS),
        &None,
    );

    Setup {
        env: env.clone(),
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        total_amount,
    }
}

/// Which of the three named parties deliberately share an address, for the
/// auth-model tests. `initialize` does not reject any of these shapes (a
/// cooperative that also runs the warehouse is a realistic single operator;
/// the all-identical case is covered by its own companion issue) — these
/// exist so the auth checks' behavior under them is proven, not assumed.
#[derive(Clone, Copy)]
enum PartyShape {
    /// Three genuinely distinct addresses.
    Distinct,
    /// One address is both buyer and warehouse operator.
    BuyerIsWarehouse,
    /// One address is both buyer and cooperative.
    BuyerIsCooperative,
    /// One address is all three named parties.
    AllSame,
}

fn setup_shaped(shape: PartyShape) -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let (buyer, cooperative, warehouse) = match shape {
        PartyShape::Distinct => (a, b, c),
        PartyShape::BuyerIsWarehouse => (a.clone(), b, a),
        PartyShape::BuyerIsCooperative => (a.clone(), a, c),
        PartyShape::AllSame => (a.clone(), a.clone(), a),
    };
    setup_in(&env, buyer, cooperative, warehouse, 1_500, 1_500, WINDOW)
}

/// A `MockAuth`-compatible invocation descriptor: the exact contract, method,
/// and serialized arguments that an authorization entry is scoped to. Soroban
/// binds each auth entry to a specific invocation tree (contract + fn_name +
/// args, plus sub-invocations), which is what makes replaying a signature
/// against a different method or different arguments impossible (#141).
fn mock_invoke<'a>(contract: &'a Address, fn_name: &'a str, args: Vec<Val>) -> MockAuthInvoke<'a> {
    MockAuthInvoke {
        contract,
        fn_name,
        args,
        sub_invokes: &[],
    }
}

/// Advances the simulated ledger clock forward by `secs`. Used to cross a
/// claim-window deadline deterministically instead of relying on wall time.
fn advance_time(s: &Setup, secs: u64) {
    let now = s.env.ledger().timestamp();
    s.env.ledger().set_timestamp(now + secs);
}

/// Sets the simulated ledger clock to an absolute timestamp. The
/// deadline-boundary tests below read the deadline straight out of contract
/// state and jump to exactly that value: `advance_time`'s `now + secs`
/// expresses "clearly before" / "clearly after" fine, but re-deriving the
/// deadline by hand to land on it exactly is the same hand-arithmetic those
/// tests exist to avoid (and would let a wrong window-length constant hide).
fn set_time(s: &Setup, timestamp: u64) {
    s.env.ledger().set_timestamp(timestamp);
}

/// The deposit-only amount `lock` actually escrows, for a given bps split —
/// mirrors `EscrowContract::deposit_amount` so tests can assert against it
/// without reaching into contract internals.
fn deposit_amount(total: i128, advance1_bps: u32, advance2_bps: u32) -> i128 {
    total * i128::from(advance1_bps + advance2_bps) / 10_000
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

/// The same drive, stopping at `ReadyForDelivery` with the remainder
/// deliberately left **unfunded** — the state every `remainder_deadline`
/// boundary assertion starts from.
fn advance_to_ready_for_delivery(s: &Setup) {
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
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

/// #94 — `initialize()` deliberately does not require the three named parties
/// (`buyer`, `cooperative`, `warehouse_operator`) to hold distinct addresses.
/// Its guard clauses validate the economic parameters and the buyer's
/// signature; none of them compare the party addresses, so a single principal
/// holding all three roles initializes successfully and lands in `Draft`.
///
/// This is intentional, not an unconsidered gap. Role separation in this
/// contract is enforced where it matters — at each state transition, by that
/// role's own `require_auth()` — rather than by a distinctness check at
/// creation. Collapsing the three roles onto one principal is a deployment
/// choice (a cooperative that also runs the warehouse, or a minimal test
/// deployment); the contract records the parties it is handed and does not
/// second-guess them. The consequence is documented rather than hidden: with
/// one principal in all three roles, every multi-party consent gate reduces to
/// that single signer, which the companion auth-model test
/// `all_three_identical_parties_accept_only_that_principals_signature` pins
/// down. This test asserts the creation half of that same fact directly.
#[test]
fn initialize_allows_buyer_cooperative_and_warehouse_to_be_one_address() {
    let env = Env::default();
    env.mock_all_auths();
    let one_principal = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let result = contract.try_initialize(
        &one_principal,
        &one_principal,
        &one_principal,
        &token_address,
        &1_000_000,
        &1_000,
        &1_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
    assert_eq!(
        result,
        Ok(Ok(())),
        "initialize must accept the all-identical party shape (no distinctness guard)"
    );
    assert_eq!(contract.get_status(), Status::Draft);

    // The three stored roles are the same address the caller passed — the
    // contract faithfully records the degenerate shape rather than rewriting or
    // rejecting it.
    let commitment = contract.get_commitment();
    assert_eq!(commitment.buyer, one_principal);
    assert_eq!(commitment.cooperative, one_principal);
    assert_eq!(commitment.warehouse_operator, one_principal);
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
    let members = Vec::from_array(&s.env, [member(&s.env, 6_000), member(&s.env, 4_000)]);

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
    let members = Vec::from_array(&s.env, [member(&s.env, 6_000), member(&s.env, 5_000)]);
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
fn cannot_set_allocation_with_more_than_max_members() {
    // The member-count cap is anchored to the PRD §16.3 scale (200
    // members). The share-sum check alone can't catch an oversized list
    // -- any long list can still sum to under 10_000 bps -- so the
    // length itself has to be rejected.
    let s = setup(1_500, 1_500);
    let mut members = Vec::new(&s.env);
    for _ in 0..=MAX_ALLOCATION_MEMBERS {
        members.push_back(member(&s.env, 1));
    }
    let result = s.contract.try_set_allocation(&members);
    assert_eq!(result, Err(Ok(Error::AllocationTooLarge)));
}

#[test]
fn allocation_with_exactly_max_members_is_allowed() {
    // Boundary: `MAX_ALLOCATION_MEMBERS` members at 50 bps each sums to
    // exactly 10_000 -- at both the member cap and the share-sum
    // ceiling, and still valid.
    let s = setup(1_500, 1_500);
    let mut members = Vec::new(&s.env);
    for _ in 0..MAX_ALLOCATION_MEMBERS {
        members.push_back(member(&s.env, 50));
    }
    s.contract.set_allocation(&members);
    assert_eq!(s.contract.get_allocation().len(), MAX_ALLOCATION_MEMBERS);
}

#[test]
/// Covers issue #54: `set_allocation()` attempted after `lock()` has already
/// run. The allocation ledger is Draft-only — one-time and immutable once
/// the commitment leaves `Draft` — so the call must bounce with
/// `InvalidState` rather than rewrite a ledger the cooperative may already
/// be acting on.
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

        /// Simulates "this oracle does not quote that asset" for the
        /// freshness probe — same test-only shape as the fault-injection
        /// suite's `clear_quote`; the real oracle has no such hook (its
        /// feed simply doesn't list the symbol).
        pub fn clear_price(env: Env) {
            env.storage().instance().remove(&symbol_short!("price"));
        }

        /// Hardcoded to match Reflector's real fiat-exchange-rate oracle
        /// (verified live, see `reflector.rs`) -- `resolve_fx_shortfall`
        /// calls this to scale `lastprice`'s raw integer back into an
        /// actual rate, same as the real oracle requires.
        pub fn decimals(_env: Env) -> u32 {
            14
        }
    }
}

/// A commitment initialized with an `oracle_config` pointing at a fresh
/// `mock_oracle::MockOracle` instance, `price` already set at the current
/// ledger timestamp. Returns the setup plus the oracle client so tests
/// can call `set_price` again to simulate a stale or updated quote.
/// `denominated_amount` of 500_000 at this fixture's 1.75 rate comes to
/// an `fx_adjusted_total` of 875_000 -- comfortably under `total_amount`
/// (1_000_000), so tests using the plain `setup_with_oracle` wrapper
/// never hit a shortfall; `setup_with_oracle_denomination` lets the FX
/// shortfall tests below choose a `denominated_amount` that does.
fn setup_with_oracle(
    max_age_secs: u64,
) -> (Setup<'static>, mock_oracle::MockOracleClient<'static>) {
    setup_with_oracle_denomination(max_age_secs, 500_000)
}

fn setup_with_oracle_denomination(
    max_age_secs: u64,
    denominated_amount: i128,
) -> (Setup<'static>, mock_oracle::MockOracleClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, token, token_admin_client) = create_token(&env, &token_admin);
    let total_amount: i128 = 1_000_000;
    // Extra headroom beyond total_amount, deliberately -- a shortfall
    // test needs the buyer able to fund a *second* payment on top of
    // the deposit + remainder they've already sent.
    token_admin_client.mint(&buyer, &(total_amount * 3));

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
            denominated_amount,
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
            denominated_amount: 500_000,
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
            denominated_amount: 500_000,
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    // Oracle-configured, so settle() now requires resolve_fx_shortfall()
    // first -- denominated_amount (500_000) at this fixture's price
    // (1.75) comes to 875_000, comfortably under total_amount (1_000_000),
    // so this specific fixture never actually has a shortfall to fund.
    s.contract.resolve_fx_shortfall();
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    let rate = s.contract.oracle_rate();
    assert_eq!(rate.price, 175_000_000_000_000);
}

// ---------- FX shortfall (PRD §4.2, option (b): buyer tops up or is
// refunded at settlement) ----------
//
// denominated_amount: 1_000_000 at this fixture's 1.75 rate ->
// fx_adjusted_total = 1_750_000 at full settlement, comfortably over
// total_amount (1_000_000) -- a genuine shortfall every test in this
// section can rely on unless it says otherwise.
const SHORTFALL_DENOMINATION: i128 = 1_000_000;
const EXPECTED_FX_ADJUSTED_TOTAL: i128 = 1_750_000;
const EXPECTED_SHORTFALL: i128 = 750_000; // 1_750_000 - 300_000 claimed - 700_000 remainder

fn advance_to_delivered_with_shortfall(s: &Setup) {
    s.contract.lock();
    advance_to_remainder_funded(s);
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
}

#[test]
fn resolve_fx_shortfall_computes_a_real_shortfall_when_the_currency_strengthened() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);

    s.contract.resolve_fx_shortfall();

    let c = s.contract.get_commitment();
    assert!(c.fx_resolved);
    assert_eq!(c.fx_adjusted_total, EXPECTED_FX_ADJUSTED_TOTAL);
    assert_eq!(c.fx_shortfall_amount, EXPECTED_SHORTFALL);
    assert!(!c.fx_shortfall_funded);
    assert!(c.fx_shortfall_deadline > 0);
}

#[test]
fn resolve_fx_shortfall_finds_nothing_owed_when_already_covered() {
    // The default setup_with_oracle fixture (denominated_amount 500_000)
    // -- fx_adjusted_total 875_000 is under total_amount 1_000_000.
    let (s, _oracle) = setup_with_oracle(3_600);
    advance_to_delivered_with_shortfall(&s);

    s.contract.resolve_fx_shortfall();

    let c = s.contract.get_commitment();
    assert!(c.fx_resolved);
    assert_eq!(c.fx_shortfall_amount, 0);
    // Trivially "funded" -- nothing to pay in, settle() shouldn't wait
    // on fund_fx_shortfall for a shortfall that doesn't exist.
    assert!(c.fx_shortfall_funded);
    assert_eq!(c.fx_shortfall_deadline, 0);
}

#[test]
fn cannot_resolve_fx_shortfall_twice() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    let result = s.contract.try_resolve_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::FxAlreadyResolved)));
}

#[test]
fn cannot_resolve_fx_shortfall_before_tranches_are_resolved() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    s.contract.lock();
    // Deliberately not advance_to_remainder_funded's full claim/reclaim
    // dance -- open both tranches but never resolve either, matching
    // settle()'s own TrancheUnresolved guard, which resolve_fx_shortfall
    // shares for the same reason (needs final, settled figures).
    s.contract.release_advance_1();
    advance_time(&s, WINDOW + 1);
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let result = s.contract.try_resolve_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::TrancheUnresolved)));
}

#[test]
/// Covers issue #55: `resolve_fx_shortfall()` called when no `oracle_config`
/// was ever set at `initialize`. The plain `setup()` fixture never configures
/// one, so the call must refuse with `OracleNotConfigured` — the documented
/// degradation path (#124) — instead of reading an oracle that doesn't exist.
fn resolve_fx_shortfall_requires_oracle_configured() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let result = s.contract.try_resolve_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::OracleNotConfigured)));
}

#[test]
fn settle_blocked_until_fx_resolved() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);

    let result = s.contract.try_settle();
    assert_eq!(result, Err(Ok(Error::FxNotResolved)));
}

#[test]
fn settle_blocked_while_fx_shortfall_unfunded() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    let result = s.contract.try_settle();
    assert_eq!(result, Err(Ok(Error::FxShortfallUnfunded)));
}

#[test]
fn settle_proceeds_immediately_when_fx_resolution_found_no_shortfall() {
    let (s, _oracle) = setup_with_oracle(3_600); // no-shortfall fixture
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    s.contract.settle();
    assert_eq!(s.contract.get_status(), Status::Settled);
}

#[test]
fn fund_fx_shortfall_pays_in_the_shortfall_and_unblocks_settle() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();
    let buyer_balance_before = s.token.balance(&s.buyer);

    s.contract.fund_fx_shortfall();

    assert_eq!(
        s.token.balance(&s.buyer),
        buyer_balance_before - EXPECTED_SHORTFALL
    );
    assert!(s.contract.get_commitment().fx_shortfall_funded);

    // No longer blocked.
    s.contract.settle();
    assert_eq!(s.contract.get_status(), Status::Settled);
    // fx_adjusted_total (1_750_000), fully covered now: claimed_by_coop
    // (300_000, already paid) + this settle's payout should sum to
    // exactly 1_750_000 -- the whole point of the top-up.
    assert_eq!(s.token.balance(&s.cooperative), 300_000 + 1_450_000);
}

#[test]
fn fund_fx_shortfall_requires_buyer_auth() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    // mock_all_auths() means this can't check *rejection* of a wrong
    // signer -- see confirm_delivery_requires_warehouse_operator's
    // comment. What it confirms is the call path genuinely requires the
    // buyer's auth to exist at all.
    s.contract.fund_fx_shortfall();
    let auths = s.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == s.buyer),
        "expected buyer auth on fund_fx_shortfall"
    );
}

#[test]
fn cannot_fund_fx_shortfall_twice() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();
    s.contract.fund_fx_shortfall();

    let result = s.contract.try_fund_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::FxShortfallAlreadyFunded)));
}

#[test]
fn cannot_fund_fx_shortfall_when_none_is_owed() {
    let (s, _oracle) = setup_with_oracle(3_600); // no-shortfall fixture
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    let result = s.contract.try_fund_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::NoFxShortfall)));
}

#[test]
fn cannot_fund_fx_shortfall_before_resolution() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);

    let result = s.contract.try_fund_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::FxNotResolved)));
}

#[test]
fn cannot_fund_fx_shortfall_after_the_window_passes() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();
    advance_time(&s, REMAINDER_WINDOW + 1);

    let result = s.contract.try_fund_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::FxShortfallWindowPassed)));
}

#[test]
fn expire_fx_shortfall_window_sweeps_escrow_to_cooperative_and_defaults_the_buyer() {
    // No require_auth() at all on this path, same reasoning as
    // expire_remainder_window_is_permissionless's own test -- the call
    // succeeding with no buyer/cooperative auth required is the proof.
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();
    let contract_balance_before = s.token.balance(&s.contract.address);
    let cooperative_balance_before = s.token.balance(&s.cooperative); // already has the 300_000 claimed pre-delivery
    advance_time(&s, REMAINDER_WINDOW + 1);

    s.contract.expire_fx_shortfall_window();

    assert_eq!(s.contract.get_status(), Status::Defaulted);
    assert_eq!(
        s.token.balance(&s.cooperative),
        cooperative_balance_before + contract_balance_before
    );
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn cannot_expire_fx_shortfall_window_before_the_deadline() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    let result = s.contract.try_expire_fx_shortfall_window();
    assert_eq!(result, Err(Ok(Error::FxShortfallWindowNotPassed)));
}

/// #114 — the exact `fx_shortfall_deadline` boundary.
///
/// Same shape as #112's `remainder_deadline`: `fund_fx_shortfall` rejects
/// only `now > fx_shortfall_deadline` (the deadline second is still the
/// buyer's window), while `expire_fx_shortfall_window` requires strictly
/// more than the deadline.
#[test]
fn fx_shortfall_deadline_boundary_is_inclusive_for_funding_and_exclusive_for_expiry() {
    // At exactly the deadline: expiry refused, funding accepted.
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();
    let deadline = s.contract.get_commitment().fx_shortfall_deadline;
    assert!(deadline > 0, "a real shortfall must open a deadline");
    set_time(&s, deadline);

    assert_eq!(
        s.contract.try_expire_fx_shortfall_window(),
        Err(Ok(Error::FxShortfallWindowNotPassed))
    );
    s.contract.fund_fx_shortfall();
    assert!(s.contract.get_commitment().fx_shortfall_funded);

    // One second later both halves flip: funding is refused and the very
    // same expiry call that was refused above now succeeds.
    let (s2, _oracle2) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s2);
    s2.contract.resolve_fx_shortfall();
    let deadline2 = s2.contract.get_commitment().fx_shortfall_deadline;
    set_time(&s2, deadline2 + 1);

    assert_eq!(
        s2.contract.try_fund_fx_shortfall(),
        Err(Ok(Error::FxShortfallWindowPassed))
    );
    s2.contract.expire_fx_shortfall_window();
    assert_eq!(s2.contract.get_status(), Status::Defaulted);
}

#[test]
fn cannot_expire_fx_shortfall_window_once_funded() {
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();
    s.contract.fund_fx_shortfall();
    advance_time(&s, REMAINDER_WINDOW + 1);

    let result = s.contract.try_expire_fx_shortfall_window();
    assert_eq!(result, Err(Ok(Error::FxShortfallAlreadyFunded)));
}

// ---------- refresh_fx_quote_status (#1, "waiting on a fresh quote") ----------

#[test]
fn stale_quote_leaves_no_flag_but_refresh_fx_quote_status_records_one() {
    // The whole reason #1 needed its own probe: `resolve_fx_shortfall`'s
    // OracleStale refusal returns `Err`, and a Soroban invocation that
    // errors rolls back every write it made — so the refusal itself can
    // never record why it refused. Proven here, not assumed: after the
    // failed resolve the flag is still false, and only the separate
    // (successful) probe persists it.
    let (s, _oracle) = setup_with_oracle(3_600);
    advance_to_delivered_with_shortfall(&s);
    advance_time(&s, 3_601); // the configured quote is now stale

    assert_eq!(
        s.contract.try_resolve_fx_shortfall(),
        Err(Ok(Error::OracleStale))
    );
    assert!(
        !s.contract.get_commitment().fx_quote_stale,
        "the failed call must have written nothing"
    );

    s.contract.refresh_fx_quote_status();
    assert!(s.contract.get_commitment().fx_quote_stale);
}

#[test]
fn refresh_fx_quote_status_clears_the_flag_once_a_fresh_quote_lands() {
    let (s, oracle) = setup_with_oracle(3_600);
    advance_to_delivered_with_shortfall(&s);
    advance_time(&s, 3_601);
    s.contract.refresh_fx_quote_status();
    assert!(s.contract.get_commitment().fx_quote_stale);

    // Reflector publishes a fresh quote; the next probe self-corrects —
    // no reverse transition to learn, the flag tracks reality.
    oracle.set_price(&175_000_000_000_000i128, &s.env.ledger().timestamp());
    s.contract.refresh_fx_quote_status();
    assert!(!s.contract.get_commitment().fx_quote_stale);
}

#[test]
fn refresh_fx_quote_status_is_informational_and_gates_nothing() {
    // The flag must not change settlement behavior in any direction. It
    // doesn't unblock anything by itself (`settle` is still gated on
    // `FxNotResolved`), and a normal fresh-quote resolve -> fund ->
    // settle path runs identically with the flag having been set and
    // cleared in between.
    let (s, oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    advance_time(&s, 3_601);
    s.contract.refresh_fx_quote_status();
    assert!(s.contract.get_commitment().fx_quote_stale);
    assert_eq!(s.contract.try_settle(), Err(Ok(Error::FxNotResolved)));

    // Fresh quote: the probe clears, the shortfall path works exactly as
    // it did before the flag existed (750_000 top-up, per the fixture's
    // recorded figures), and settle completes.
    oracle.set_price(&175_000_000_000_000i128, &s.env.ledger().timestamp());
    s.contract.refresh_fx_quote_status();
    assert!(!s.contract.get_commitment().fx_quote_stale);
    s.contract.resolve_fx_shortfall();
    assert_eq!(
        s.contract.get_commitment().fx_shortfall_amount,
        EXPECTED_SHORTFALL
    );
    s.contract.fund_fx_shortfall();
    s.contract.settle();
    assert_eq!(s.contract.get_status(), Status::Settled);
}

#[test]
fn resolve_fx_shortfall_clears_the_flag_so_it_never_lingers_past_resolution() {
    let (s, oracle) = setup_with_oracle(3_600);
    advance_to_delivered_with_shortfall(&s);
    advance_time(&s, 3_601);
    s.contract.refresh_fx_quote_status();
    assert!(s.contract.get_commitment().fx_quote_stale);

    // Fresh quote, then resolve *without* another probe — resolution
    // itself must clear the flag, or a commitment could sit `Settled`
    // still advertising "waiting on a fresh quote".
    oracle.set_price(&175_000_000_000_000i128, &s.env.ledger().timestamp());
    s.contract.resolve_fx_shortfall();
    assert!(!s.contract.get_commitment().fx_quote_stale);

    // And there's nothing left to wait for once resolved.
    assert_eq!(
        s.contract.try_refresh_fx_quote_status(),
        Err(Ok(Error::FxAlreadyResolved))
    );
}

#[test]
fn refresh_fx_quote_status_refuses_outside_delivered_and_without_an_oracle() {
    // Not yet Delivered — the flag can't matter before settlement is
    // actually pending.
    let (s2, _oracle) = setup_with_oracle(3_600);
    s2.contract.lock();
    assert_eq!(
        s2.contract.try_refresh_fx_quote_status(),
        Err(Ok(Error::InvalidState))
    );

    // Delivered, but this commitment never needed a conversion at all.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    assert_eq!(
        s.contract.try_refresh_fx_quote_status(),
        Err(Ok(Error::InvalidState))
    );
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    assert_eq!(
        s.contract.try_refresh_fx_quote_status(),
        Err(Ok(Error::OracleNotConfigured))
    );
    assert!(
        !s.contract.get_commitment().fx_quote_stale,
        "a refused call must have written nothing"
    );
}

#[test]
fn refresh_fx_quote_status_does_not_conflate_a_missing_quote_with_a_stale_one() {
    // "The oracle doesn't quote this asset at all" is a permanent
    // configuration gap, not "come back later" — waiting will never fix
    // it, so the error propagates and the waiting flag is not set.
    let (s, oracle) = setup_with_oracle(3_600);
    advance_to_delivered_with_shortfall(&s);
    oracle.clear_price();

    assert_eq!(
        s.contract.try_refresh_fx_quote_status(),
        Err(Ok(Error::OraclePriceUnavailable))
    );
    assert!(!s.contract.get_commitment().fx_quote_stale);
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

#[test]
/// Covers issue #57: `claim_advance_1()` attempted by the buyer instead of
/// the cooperative. `claim_tranche` gates on `cooperative.require_auth()`
/// before any state check, so under enforcing auth the buyer's own signature
/// cannot satisfy it. This uses `mock_auths` — the same technique as the
/// multi-party auth-scrutiny tests further below — because it mocks only the
/// listed invocation and fails every other `require_auth()`, which is what
/// makes the wrong-signer rejection observable; `mock_all_auths()` alone
/// (which the surrounding tests run under) can only prove an auth exists,
/// never that the wrong party is refused.
fn claim_advance_1_rejects_the_buyer_as_signer() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();

    let invoke = mock_invoke(&s.contract.address, "claim_advance_1", ().into_val(&s.env));

    // The buyer presents their own signature for the exact call — the
    // cooperative's required signature is absent, so the host refuses
    // before the contract body runs.
    s.env.mock_auths(&[MockAuth {
        address: &s.buyer,
        invoke: &invoke,
    }]);
    let result = s.contract.try_claim_advance_1();
    assert!(
        result.is_err(),
        "the buyer's signature must not satisfy claim_advance_1's cooperative auth: {result:?}"
    );

    // The refused attempt claimed nothing and moved nothing.
    assert!(!s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.cooperative), 0);

    // The cooperative's own signature is the one the call actually requires.
    s.env.mock_auths(&[MockAuth {
        address: &s.cooperative,
        invoke: &invoke,
    }]);
    s.contract.claim_advance_1();
    assert!(s.contract.get_commitment().advance1_claimed);
    assert_eq!(
        s.token.balance(&s.cooperative),
        s.total_amount * 1_500 / 10_000
    );
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

// ---------- mark_checkpoint ----------

#[test]
fn cannot_mark_checkpoint_twice() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();

    let result = s.contract.try_mark_checkpoint();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
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

/// #112 — the exact `remainder_deadline` boundary, at the deadline second
/// itself and not one second either side of it.
///
/// The two guards read in opposite directions on purpose: `fund_remainder`
/// rejects only `now > remainder_deadline` (inclusive — the deadline second
/// is still the buyer's to pay in), while `expire_remainder_window` requires
/// `now > remainder_deadline` (exclusive — the default path is not open in
/// that second). A payment and a default therefore can never both be legal
/// at the same timestamp. The clock is set to the deadline read back out of
/// contract state, not to a value re-derived from `REMAINDER_WINDOW` by hand.
#[test]
fn remainder_deadline_boundary_is_inclusive_for_funding_and_exclusive_for_expiry() {
    // At exactly the deadline: expiry refused, funding accepted.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_to_ready_for_delivery(&s);
    let deadline = s.contract.get_commitment().remainder_deadline;
    set_time(&s, deadline);

    assert_eq!(
        s.contract.try_expire_remainder_window(),
        Err(Ok(Error::RemainderWindowNotPassed))
    );
    s.contract.fund_remainder();
    assert!(s.contract.get_commitment().remainder_funded);

    // One second later both halves flip: funding is refused and the very
    // same expiry call that was refused above now succeeds, so the boundary
    // sits exactly between `deadline` and `deadline + 1`.
    let s2 = setup(1_500, 2_000);
    s2.contract.lock();
    advance_to_ready_for_delivery(&s2);
    let deadline2 = s2.contract.get_commitment().remainder_deadline;
    set_time(&s2, deadline2 + 1);

    assert_eq!(
        s2.contract.try_fund_remainder(),
        Err(Ok(Error::RemainderWindowPassed))
    );
    s2.contract.expire_remainder_window();
    assert_eq!(s2.contract.get_status(), Status::Defaulted);
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

#[test]
fn expire_remainder_window_works_when_only_a_third_party_signs() {
    // Issue #73: "called by a genuinely unrelated third-party address
    // (confirm it's still permissionless, not accidentally gated)". The
    // test above runs under mock_all_auths(), which satisfies any
    // require_auth() a future accidental gate would add — it cannot tell
    // "permissionless" from "every party's auth silently provided".
    // mock_auths(&[stranger]) can: every require_auth() except the
    // stranger's own exact invocation would fail, so this call succeeding
    // proves the path requires no signature at all — the stranger's entry
    // is never even consumed. The tranches are deliberately left
    // unclaimed, so the sweep has a non-zero balance to move and the
    // assertions cover the actual transfer, not just the status flip.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.ready_for_delivery();
    advance_time(&s, REMAINDER_WINDOW + 1);

    let stranger = Address::generate(&s.env);
    let invoke = mock_invoke(
        &s.contract.address,
        "expire_remainder_window",
        ().into_val(&s.env),
    );
    s.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &invoke,
    }]);

    let contract_balance_before = s.token.balance(&s.contract.address);
    let cooperative_balance_before = s.token.balance(&s.cooperative);
    assert!(contract_balance_before > 0); // the unclaimed deposit

    s.contract.expire_remainder_window();

    assert_eq!(s.contract.get_status(), Status::Defaulted);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(
        s.token.balance(&s.cooperative),
        cooperative_balance_before + contract_balance_before
    );
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

/// #113 — the exact `delivery_deadline` boundary.
///
/// `reclaim_on_nondelivery`'s guard is `now <= delivery_deadline`, so the
/// deadline second itself still belongs to the cooperative: the buyer's
/// forfeiture right opens strictly after it.
#[test]
fn delivery_deadline_boundary_is_exclusive_for_reclaim_on_nondelivery() {
    let s = setup(1_500, 2_000);
    s.contract.lock();
    let deadline = s.contract.get_commitment().delivery_deadline;
    set_time(&s, deadline);

    assert_eq!(
        s.contract.try_reclaim_on_nondelivery(),
        Err(Ok(Error::DeliveryDeadlineNotPassed))
    );
    // Refused, and nothing moved: the call is all-or-nothing at the boundary.
    assert_eq!(s.contract.get_status(), Status::Locked);
    assert!(s.token.balance(&s.contract.address) > 0);

    // One second later it opens — proving the refusal above is the boundary
    // itself, not a wider margin.
    set_time(&s, deadline + 1);
    s.contract.reclaim_on_nondelivery();
    assert_eq!(s.contract.get_status(), Status::Forfeited);
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
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

#[test]
fn reclaim_on_nondelivery_attempted_by_a_non_buyer_fails() {
    // Issue #72: the same call "attempted by someone other than the
    // buyer". The test above runs under mock_all_auths(), which can show
    // the buyer's require_auth() is reached, never that anyone else's
    // signature is insufficient. Under mock_auths(&[stranger]) the
    // stranger's entry is the only one in the authorization set, so
    // c.buyer.require_auth() matches nothing: the call must abort as a
    // host auth error and leave no partial state. The buyer's own entry
    // then succeeds from that exact same state, proving the gate is
    // precisely "the buyer's signature" — not some other condition that
    // would also have stopped the buyer.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    advance_time(&s, DELIVERY_WINDOW + 1);

    let stranger = Address::generate(&s.env);
    let buyer_balance_before = s.token.balance(&s.buyer);
    let contract_balance_before = s.token.balance(&s.contract.address);

    let invoke = mock_invoke(
        &s.contract.address,
        "reclaim_on_nondelivery",
        ().into_val(&s.env),
    );
    s.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &invoke,
    }]);
    let result = s.contract.try_reclaim_on_nondelivery();
    assert!(
        matches!(result, Err(Err(_))),
        "a non-buyer's signature must not reclaim: {result:?}"
    );
    // Atomicity: the rejected invocation rolled back every write.
    assert_eq!(s.contract.get_status(), Status::Locked);
    assert_eq!(
        s.token.balance(&s.contract.address),
        contract_balance_before
    );
    assert_eq!(s.token.balance(&s.buyer), buyer_balance_before);

    // Same call, the one signature it actually requires.
    s.env.mock_auths(&[MockAuth {
        address: &s.buyer,
        invoke: &invoke,
    }]);
    s.contract.reclaim_on_nondelivery();
    assert_eq!(s.contract.get_status(), Status::Forfeited);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
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

    let result = s
        .contract
        .try_confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
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

    let result = s
        .contract
        .try_confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    assert_eq!(result, Err(Ok(Error::RemainderNotFunded)));
}

#[test]
/// Covers issue #56: `confirm_delivery()` called twice on the same
/// commitment. The first call moves status to `Delivered`, so the second is
/// refused by the `ReadyForDelivery` guard with `InvalidState` before it can
/// touch anything. The retry deliberately carries *different* quantity/grade
/// arguments that would otherwise be valid — proving the refusal is the
/// status gate and that none of the first call's recorded delivery data is
/// rewritten by a replay.
fn cannot_confirm_delivery_twice() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let result = s.contract.try_confirm_delivery(&500, &2);
    assert_eq!(result, Err(Ok(Error::InvalidState)));

    // The refused second call left the first call's records exactly as they
    // were — no overwrite, no double count.
    let c = s.contract.get_commitment();
    assert_eq!(c.status, Status::Delivered);
    assert_eq!(c.delivered_quantity, CONTRACTED_QUANTITY);
    assert_eq!(c.grade_index, FULL_PRICE_GRADE);
    assert_eq!(c.settlement_bps, 10_000); // full quantity at the full-price grade
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

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
fn initialize_rejects_a_grade_schedule_over_max_entries() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let mut over_cap = Vec::new(&env);
    for _ in 0..=MAX_GRADE_ENTRIES {
        over_cap.push_back(10_000u32);
    }

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
        &over_cap,
        &None,
    );
    assert_eq!(result, Err(Ok(Error::GradeScheduleTooLong)));
}

#[test]
fn initialize_accepts_a_grade_schedule_at_max_entries() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    let mut at_cap = Vec::new(&env);
    for _ in 0..MAX_GRADE_ENTRIES {
        at_cap.push_back(10_000u32);
    }

    contract.initialize(
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
        &at_cap,
        &None,
    );
    assert_eq!(contract.get_status(), Status::Draft);
    assert_eq!(
        contract.get_commitment().grade_price_bps.len(),
        MAX_GRADE_ENTRIES
    );
}

#[test]
fn confirm_delivery_rejects_an_out_of_range_grade_index() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    // GRADE_PRICE_BPS has 3 entries (indices 0-2) -- 3 is out of range.
    let result = s.contract.try_confirm_delivery(&CONTRACTED_QUANTITY, &3);
    assert_eq!(result, Err(Ok(Error::InvalidGradeIndex)));

    // #95: the rejection also pins that nothing was persisted — the
    // bounds check runs before any mutation and before the sole `save`,
    // so the stored commitment is exactly as `advance_to_remainder_funded`
    // left it (see `confirm_delivery`'s doc comment for the full trace).
    assert_eq!(s.contract.get_status(), Status::ReadyForDelivery);
    let c = s.contract.get_commitment();
    assert_eq!(c.delivered_quantity, 0);
    assert_eq!(c.grade_index, 0);
    assert_eq!(c.settlement_bps, 0);
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
fn confirm_delivery_storage_profile_and_settlement_bps_invariants() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);

    let pre = s.contract.get_commitment();
    assert_eq!(pre.status, Status::ReadyForDelivery);

    // Profile confirm_delivery execution: exactly 1 load, 1 save, state transitions cleanly
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let post = s.contract.get_commitment();
    assert_eq!(post.status, Status::Delivered);
    assert_eq!(post.delivered_quantity, CONTRACTED_QUANTITY);
    assert_eq!(post.grade_index, FULL_PRICE_GRADE);
    assert_eq!(post.settlement_bps, 10_000);

    // Unmodified fields maintain exact equality (no accidental storage mutation/corruption)
    assert_eq!(post.contracted_quantity, pre.contracted_quantity);
    assert_eq!(post.buyer, pre.buyer);
    assert_eq!(post.cooperative, pre.cooperative);
    assert_eq!(post.warehouse_operator, pre.warehouse_operator);
    assert_eq!(post.token, pre.token);
    assert_eq!(post.total_amount, pre.total_amount);
    assert_eq!(post.advance1_claimed, pre.advance1_claimed);
    assert_eq!(post.advance2_claimed, pre.advance2_claimed);
    assert!(post.remainder_funded);
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
    s.contract
        .confirm_delivery(&half_quantity, &FULL_PRICE_GRADE);
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
    s.contract
        .confirm_delivery(&double_quantity, &FULL_PRICE_GRADE);
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
    assert_eq!(
        s.token.balance(&s.cooperative),
        advance1_amount + advance2_amount
    );
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
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
/// Covers issue #52: `cancel()` attempted after the remainder has been
/// funded but before `Delivered`. `advance_to_remainder_funded` stops at
/// `ReadyForDelivery` with the remainder fully funded — exactly the window
/// between `fund_remainder()` and `confirm_delivery()` — and `cancel()`
/// must still succeed from there, returning the funded remainder to the
/// buyer while the already-claimed tranches stay with the cooperative.
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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

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
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
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

// ---------- disputes ----------

#[test]
fn flag_dispute_by_buyer_moves_to_disputed_and_records_pre_status() {
    let s = setup(1_500, 1_500);
    s.contract.lock();

    s.contract.flag_dispute(&s.buyer);

    assert_eq!(s.contract.get_status(), Status::Disputed);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Locked
    );
}

#[test]
fn flag_dispute_by_cooperative_works() {
    let s = setup(1_500, 1_500);
    s.contract.lock();

    s.contract.flag_dispute(&s.cooperative);

    assert_eq!(s.contract.get_status(), Status::Disputed);
}

#[test]
fn flag_dispute_by_warehouse_operator_works() {
    let s = setup(1_500, 1_500);
    s.contract.lock();

    s.contract.flag_dispute(&s.warehouse);

    assert_eq!(s.contract.get_status(), Status::Disputed);
}

#[test]
fn flag_dispute_rejects_a_non_party() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let stranger = Address::generate(&s.env);

    let result = s.contract.try_flag_dispute(&stranger);
    assert_eq!(result, Err(Ok(Error::NotAParty)));
}

#[test]
fn flag_dispute_rejects_from_draft() {
    let s = setup(1_500, 1_500);

    let result = s.contract.try_flag_dispute(&s.buyer);
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn flag_dispute_rejects_after_settlement() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    s.contract.settle();

    let result = s.contract.try_flag_dispute(&s.buyer);
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
/// Covers issue #51: `flag_dispute()` called a second time while already `Disputed`
/// returns `Err(Error::InvalidState)`, whether called by another party or the same party.
fn flag_dispute_rejects_a_second_flag() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    // Calling a second time while already Disputed (by another party)
    let result = s.contract.try_flag_dispute(&s.cooperative);
    assert_eq!(result, Err(Ok(Error::InvalidState)));

    // Calling a second time while already Disputed (by the same party)
    let result_same = s.contract.try_flag_dispute(&s.buyer);
    assert_eq!(result_same, Err(Ok(Error::InvalidState)));
}

#[test]
/// Covers issue #53: a second, *different* dispute on the same commitment —
/// flagged by another party at a later lifecycle stage, after the first
/// dispute was resolved — is accepted, freezes from the new stage, and
/// resolves back to it. The one-dispute-at-a-time rule pinned by
/// `flag_dispute_rejects_a_second_flag` only guards while the first dispute
/// is still open: `resolve_dispute` restores the pre-dispute status and
/// clears the dispute fields, so the next dispute starts from a clean slate.
fn second_dispute_on_the_same_commitment_is_flagged_and_resolved_later_in_its_lifecycle() {
    let s = setup(1_500, 1_500);
    s.contract.lock();

    // Round one: the buyer disputes from Locked.
    s.contract.flag_dispute(&s.buyer);
    assert_eq!(s.contract.get_status(), Status::Disputed);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Locked
    );
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Locked);
    // Resolution clears the dispute fields — nothing lingers into round two.
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Draft
    );
    assert_eq!(s.contract.get_commitment().dispute_deadline, 0);

    // Push much further into the lifecycle so the second dispute is
    // genuinely a later one: ReadyForDelivery with the remainder funded.
    advance_to_remainder_funded(&s);
    assert_eq!(s.contract.get_status(), Status::ReadyForDelivery);

    // Round two: a different party disputes from a different stage, and the
    // freeze records the new pre-dispute status, not the old one.
    s.contract.flag_dispute(&s.warehouse);
    assert_eq!(s.contract.get_status(), Status::Disputed);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::ReadyForDelivery
    );
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::ReadyForDelivery);
    assert_eq!(s.contract.get_commitment().dispute_deadline, 0);

    // The second freeze genuinely lifted: the lifecycle continues past it,
    // all the way to Delivered.
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    assert_eq!(s.contract.get_status(), Status::Delivered);
}

#[test]
fn flag_dispute_reaches_from_delivered_too() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    advance_to_remainder_funded(&s);
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    s.contract.flag_dispute(&s.cooperative);

    assert_eq!(s.contract.get_status(), Status::Disputed);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Delivered
    );
}

#[test]
/// Covers issue #50: `reassign_buyer()` while the commitment is `Disputed`
/// is rejected via the existing `Error::InvalidState` guard (reassign_buyer's
/// status match only permits Draft..=ReadyForDelivery, rejecting Disputed).
/// Also checks other state-changing calls are frozen in Disputed state.
fn flag_dispute_freezes_every_other_state_changing_call() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    assert_eq!(
        s.contract.try_release_advance_1(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(s.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    assert_eq!(
        s.contract.try_reassign_buyer(&Address::generate(&s.env)),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn resolve_dispute_restores_the_exact_pre_dispute_status() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.flag_dispute(&s.cooperative);

    s.contract.resolve_dispute();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Draft
    );
}

#[test]
fn resolve_dispute_allows_normal_flow_to_continue_afterward() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);
    s.contract.resolve_dispute();

    // Proves the freeze genuinely lifted, not just that the status field
    // flipped back -- a real subsequent call succeeds.
    s.contract.release_advance_1();
    assert_eq!(s.contract.get_status(), Status::Advance1Released);
}

#[test]
fn resolve_dispute_requires_all_three_parties_auth() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    // mock_all_auths() means this can't check *rejection* of a missing
    // signer -- see reassign_buyer_requires_current_buyer_cooperative_and_new_buyer_auth's
    // comment. What it confirms is that all three parties' auth genuinely
    // appears in the trace, not just the flagger's.
    s.contract.resolve_dispute();
    let auths = s.env.auths();
    let touched = |addr: &Address| auths.iter().any(|(a, _)| a == addr);
    assert!(touched(&s.buyer), "expected the buyer's auth");
    assert!(touched(&s.cooperative), "expected the cooperative's auth");
    assert!(
        touched(&s.warehouse),
        "expected the warehouse operator's auth"
    );
}

#[test]
fn resolve_dispute_rejects_when_not_disputed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();

    let result = s.contract.try_resolve_dispute();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn expire_dispute_window_rejects_before_the_deadline() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    let result = s.contract.try_expire_dispute_window();
    assert_eq!(result, Err(Ok(Error::DisputeWindowNotPassed)));
}

/// #115 — the exact `dispute_deadline` boundary.
///
/// `expire_dispute_window`'s guard is `now <= dispute_deadline`, so the
/// deadline second itself is still inside the freeze. `resolve_dispute` has
/// no deadline guard at all — unanimous consent is its only gate — so it
/// still succeeds in that same second. That pairing is what keeps the
/// boundary second from being a dead zone where neither resume path works.
#[test]
fn dispute_deadline_boundary_keeps_resolve_open_and_expire_closed() {
    // At exactly the deadline: timeout refused, unanimous resolve accepted.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);
    let deadline = s.contract.get_commitment().dispute_deadline;
    assert!(deadline > 0, "flagging a dispute must open a deadline");
    set_time(&s, deadline);

    assert_eq!(
        s.contract.try_expire_dispute_window(),
        Err(Ok(Error::DisputeWindowNotPassed))
    );
    assert_eq!(s.contract.get_status(), Status::Disputed);
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Locked);

    // One second later the timeout path opens too, restoring the same
    // pre-dispute status.
    let s2 = setup(1_500, 1_500);
    s2.contract.lock();
    s2.contract.flag_dispute(&s2.buyer);
    let deadline2 = s2.contract.get_commitment().dispute_deadline;
    set_time(&s2, deadline2 + 1);

    s2.contract.expire_dispute_window();
    assert_eq!(s2.contract.get_status(), Status::Locked);
    assert_eq!(
        s2.contract.get_commitment().dispute_pre_status,
        Status::Draft
    );
}

#[test]
fn expire_dispute_window_restores_pre_dispute_status_after_the_deadline() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.flag_dispute(&s.warehouse);
    advance_time(&s, REMAINDER_WINDOW + 1);

    s.contract.expire_dispute_window();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Draft
    );
}

#[test]
fn expire_dispute_window_is_permissionless() {
    // No require_auth() at all on this path -- see
    // expire_remainder_window_is_permissionless's comment for why
    // mock_all_auths() still lets this assertion mean something: the
    // auth trace should name none of the three parties, since only they
    // (or the flagger, already recorded before this call) would appear
    // if some require_auth() were secretly present and mock-satisfied.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);
    advance_time(&s, REMAINDER_WINDOW + 1);

    s.contract.expire_dispute_window();
    assert_eq!(s.contract.get_status(), Status::Locked);

    let auths = s.env.auths();
    assert!(
        !auths
            .iter()
            .any(|(a, _)| *a == s.cooperative || *a == s.warehouse),
        "expire_dispute_window should not require any party's auth"
    );
}

#[test]
fn dispute_does_not_pause_an_already_ticking_deadline_known_limitation() {
    // Documents flag_dispute's own doc comment: freezing status doesn't
    // pause other absolute deadlines. Here the advance-1 claim window is
    // opened, then a dispute is flagged and outlives that window --
    // resolving the dispute immediately makes reclaim_advance_1 callable,
    // even though the claim window "passed" while nobody could act.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.flag_dispute(&s.buyer);
    advance_time(&s, WINDOW + 1); // outlives both the claim window and (since REMAINDER_WINDOW == WINDOW here) the dispute window

    s.contract.expire_dispute_window();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    // The claim window's own deadline already passed during the freeze --
    // reclaim is immediately available, not "reset" by having been frozen.
    s.contract.reclaim_advance_1();
    let deposit = deposit_amount(s.total_amount, 1_500, 1_500);
    let advance1_amount = s.total_amount * 1_500 / 10_000;
    assert_eq!(
        s.token.balance(&s.buyer),
        s.total_amount - deposit + advance1_amount
    );
}

#[test]
fn dispute_in_the_remainder_window_the_remainder_deadline_still_governs() {
    // Issue #75, the `remainder_deadline` half: extends the known-limitation
    // test above from the advance-1 claim window to `expire_remainder_window`'s
    // own deadline. The dispute is flagged with one second left on the
    // remainder window, so that deadline expires mid-freeze while the
    // dispute's own (equally long) window is still open. Which deadline
    // governs: the remainder deadline. The freeze controls who can act —
    // status-gated calls bounce for as long as it lasts — but it never
    // pauses, extends, or resets the deadline itself, so once the dispute
    // resolves, curing is already impossible and the expired deadline's
    // consequence is immediately available.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.ready_for_delivery();

    advance_time(&s, REMAINDER_WINDOW - 1);
    s.contract.flag_dispute(&s.buyer);
    advance_time(&s, 2); // past remainder_deadline (604_800), still inside dispute_deadline (1_209_599)

    // The dispute is genuinely still open — it's the other deadline that
    // expired.
    let result = s.contract.try_expire_dispute_window();
    assert_eq!(result, Err(Ok(Error::DisputeWindowNotPassed)));

    // While frozen, the past-due remainder deadline changes nothing: the
    // status gate bounces the call.
    let result = s.contract.try_expire_remainder_window();
    assert_eq!(result, Err(Ok(Error::InvalidState)));

    // The parties resolve — status restores to exactly the pre-dispute
    // ReadyForDelivery ...
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::ReadyForDelivery);

    // ... but the remainder deadline governs from here: the freeze didn't
    // extend it, so the buyer can no longer cure by funding ...
    let result = s.contract.try_fund_remainder();
    assert_eq!(result, Err(Ok(Error::RemainderWindowPassed)));

    // ... and the expired deadline's own consequence executes immediately.
    let deposit = deposit_amount(s.total_amount, 1_500, 1_500);
    s.contract.expire_remainder_window();
    assert_eq!(s.contract.get_status(), Status::Defaulted);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(s.token.balance(&s.cooperative), deposit);
}

#[test]
fn dispute_in_the_fx_shortfall_window_the_fx_shortfall_deadline_still_governs() {
    // Issue #75, the `fx_shortfall_deadline` half: same shape as the
    // remainder-window test above, on the window `resolve_fx_shortfall`
    // opens once a genuine shortfall exists. The top-up deadline expires
    // mid-freeze; the shortfall deadline still governs.
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    advance_time(&s, REMAINDER_WINDOW - 1);
    s.contract.flag_dispute(&s.buyer);
    advance_time(&s, 2); // past fx_shortfall_deadline, still inside dispute_deadline

    let result = s.contract.try_expire_dispute_window();
    assert_eq!(result, Err(Ok(Error::DisputeWindowNotPassed)));

    // While frozen, the past-due FX deadline changes nothing.
    let result = s.contract.try_expire_fx_shortfall_window();
    assert_eq!(result, Err(Ok(Error::InvalidState)));

    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Delivered);

    // The deadline governs: the top-up can no longer be paid in ...
    let result = s.contract.try_fund_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::FxShortfallWindowPassed)));

    // ... and its consequence executes immediately: the escrowed remainder
    // sweeps to the cooperative, Defaulted.
    let escrow_before = s.token.balance(&s.contract.address); // the 700_000 funded remainder
    let cooperative_before = s.token.balance(&s.cooperative); // the claimed 300_000
    s.contract.expire_fx_shortfall_window();
    assert_eq!(s.contract.get_status(), Status::Defaulted);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(
        s.token.balance(&s.cooperative),
        cooperative_before + escrow_before
    );
}

// ---------- multi-party Soroban auth model scrutiny (#138-#141) ----------
//
// The tests above run under `mock_all_auths()`, which makes every
// `require_auth()` succeed and therefore can say nothing about *rejection*.
// The tests below deliberately drop into `mock_auths(&[...])`, which mocks only
// the exact invocations listed and fails every other `require_auth()` — that is
// what lets the auth-shape claims in these four issues be proven rather than
// asserted. `mock_auths` is installed *after* setup, so state can still be
// driven into place under `mock_all_auths` first.

/// #138 — a staged multi-party proposal whose signature-expiration ledger
/// lapses before every party has signed.
///
/// In the off-chain staging flow each party signs its own auth entry at its own
/// time. If one entry's `signature_expiration_ledger` is reached before the last
/// co-signer submits, the submission must fail cleanly and must not half-apply.
///
/// On-chain this is enforced by the host, not by this contract: the expiry is
/// checked while the auth entry is verified, before `resolve_dispute`'s body
/// runs at all. The failure therefore surfaces as a host/Auth invocation error
/// (`InvokeError::Abort` here) — the same shape any missing signature produces —
/// not a confusing contract-returned `Error`. Because the rejected auth aborts
/// the whole invocation, none of `resolve_dispute`'s state writes land.
#[test]
fn expired_staged_auth_entry_fails_cleanly_with_no_partial_state() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);
    let before = s.contract.get_commitment();

    let invoke = mock_invoke(&s.contract.address, "resolve_dispute", ().into_val(&s.env));
    s.env.mock_auths(&[
        MockAuth {
            address: &s.buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &s.cooperative,
            invoke: &invoke,
        },
        MockAuth {
            address: &s.warehouse,
            invoke: &invoke,
        },
    ]);
    // `mock_auths` stamps every entry with
    // `signature_expiration_ledger = current_sequence + max_entry_ttl`. Advance
    // one ledger past that, so every staged entry is genuinely expired.
    s.env
        .ledger()
        .set_sequence_number(s.env.storage().max_ttl() + 1);

    let result = s.contract.try_resolve_dispute();
    assert!(
        matches!(result, Err(Err(_))),
        "an expired auth entry must fail as a host auth error, not a contract error: {result:?}"
    );

    // Atomicity: the invocation's state writes were rolled back with it.
    let after = s.contract.get_commitment();
    assert_eq!(after.status, Status::Disputed);
    assert_eq!(after.dispute_pre_status, before.dispute_pre_status);
    assert_eq!(after.dispute_deadline, before.dispute_deadline);
}

/// #139 — the same address legitimately holding two of the three named roles
/// (here buyer == warehouse_operator, cooperative distinct).
///
/// Soroban auth is per-address. The shared address's single entry satisfies
/// both of the `require_auth()` calls that name it — correctly, because it is
/// one genuine principal, and a contract cannot (and should not) demand two
/// distinct signatures from one key. What must not happen is one address's
/// signature standing in for a *different* party: the distinct cooperative's
/// own consent is still required, and a single signature is never counted as
/// two separate parties' agreement.
#[test]
fn two_of_three_identical_parties_still_require_the_distinct_partys_auth() {
    let s = setup_shaped(PartyShape::BuyerIsWarehouse);
    // Sanity-check the shape the test is actually exercising.
    assert_eq!(s.buyer, s.warehouse);
    assert_ne!(s.cooperative, s.buyer);

    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    let invoke = mock_invoke(&s.contract.address, "resolve_dispute", ().into_val(&s.env));
    let shared = s.buyer.clone(); // == s.warehouse

    // Only the shared buyer/warehouse address authorizes. The distinct
    // cooperative's separate consent is missing, so the call must fail.
    s.env.mock_auths(&[MockAuth {
        address: &shared,
        invoke: &invoke,
    }]);
    let result = s.contract.try_resolve_dispute();
    assert!(
        result.is_err(),
        "the distinct cooperative's signature must still be required: {result:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // Two real signatures now — one for the shared buyer/warehouse principal,
    // one for the cooperative — are enough, because two addresses hold the
    // three roles. No third signature is invented for the duplicate role.
    s.env.mock_auths(&[
        MockAuth {
            address: &shared,
            invoke: &invoke,
        },
        MockAuth {
            address: &s.cooperative,
            invoke: &invoke,
        },
    ]);
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Locked);
}

/// #139, the `cancel` half of the same principle: when the buyer and the
/// cooperative are one and the same principal, `cancel` needs that principal's
/// one genuine signature — not two impossible-to-produce ones for the same key
/// — and must still reject a stranger.
///
/// This is the method the duplicate-`require_auth()` bug bit most directly:
/// before the fix, the second `require_auth()` on the shared address aborted
/// the whole call under enforcing auth, so a realistic buyer/cooperative
/// operator could never cancel.
#[test]
fn cancel_succeeds_when_buyer_and_cooperative_share_one_address() {
    let s = setup_shaped(PartyShape::BuyerIsCooperative);
    assert_eq!(s.buyer, s.cooperative);
    s.contract.lock();

    let invoke = mock_invoke(&s.contract.address, "cancel", ().into_val(&s.env));
    let stranger = Address::generate(&s.env);

    // A stranger's signature satisfies neither role.
    s.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &invoke,
    }]);
    assert!(s.contract.try_cancel().is_err());
    assert_eq!(s.contract.get_status(), Status::Locked);

    // The shared principal's single signature is the whole requirement.
    s.env.mock_auths(&[MockAuth {
        address: &s.buyer,
        invoke: &invoke,
    }]);
    s.contract.cancel();
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
}

/// #139, the `reassign_buyer` half. With buyer == cooperative, the two
/// outgoing-position signatures collapse into one principal; the incoming
/// buyer's own signature is still separately required.
#[test]
fn reassign_buyer_succeeds_when_buyer_and_cooperative_share_one_address() {
    let s = setup_shaped(PartyShape::BuyerIsCooperative);
    assert_eq!(s.buyer, s.cooperative);
    s.contract.lock();
    let new_buyer = Address::generate(&s.env);

    let invoke = mock_invoke(
        &s.contract.address,
        "reassign_buyer",
        (new_buyer.clone(),).into_val(&s.env),
    );

    // The shared principal alone is not enough — the incoming buyer must
    // consent to receiving the position.
    s.env.mock_auths(&[MockAuth {
        address: &s.buyer,
        invoke: &invoke,
    }]);
    assert!(s.contract.try_reassign_buyer(&new_buyer).is_err());
    assert_eq!(s.contract.get_commitment().buyer, s.buyer);

    // Shared principal + incoming buyer: two genuine signatures for three
    // roles, and no duplicate-signature abort.
    s.env.mock_auths(&[
        MockAuth {
            address: &s.buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &new_buyer,
            invoke: &invoke,
        },
    ]);
    s.contract.reassign_buyer(&new_buyer);
    assert_eq!(s.contract.get_commitment().buyer, new_buyer);
}

/// #140 — `resolve_dispute`'s three-party auth with three genuinely distinct
/// addresses.
///
/// Two of the three is not enough; three distinct signatures are required. A
/// single key can never stand in for the other two when the addresses differ.
#[test]
fn resolve_dispute_requires_three_distinct_signatures_when_parties_are_distinct() {
    let s = setup_shaped(PartyShape::Distinct);
    assert_ne!(s.buyer, s.cooperative);
    assert_ne!(s.cooperative, s.warehouse);
    assert_ne!(s.buyer, s.warehouse);

    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    let invoke = mock_invoke(&s.contract.address, "resolve_dispute", ().into_val(&s.env));

    // Buyer + cooperative only — the warehouse operator's consent is missing.
    s.env.mock_auths(&[
        MockAuth {
            address: &s.buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &s.cooperative,
            invoke: &invoke,
        },
    ]);
    let result = s.contract.try_resolve_dispute();
    assert!(
        result.is_err(),
        "two of three distinct parties must not be enough: {result:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // All three distinct parties.
    s.env.mock_auths(&[
        MockAuth {
            address: &s.buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &s.cooperative,
            invoke: &invoke,
        },
        MockAuth {
            address: &s.warehouse,
            invoke: &invoke,
        },
    ]);
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Locked);
}

/// The all-identical companion case: when one address genuinely holds all three
/// roles, one signature is the only thing there is to require — the contract
/// cannot conjure two more distinct signers for one principal. What matters is
/// that it does not accept some *other* address's signature in that principal's
/// place.
#[test]
fn all_three_identical_parties_accept_only_that_principals_signature() {
    let s = setup_shaped(PartyShape::AllSame);
    assert_eq!(s.buyer, s.cooperative);
    assert_eq!(s.cooperative, s.warehouse);

    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);

    let invoke = mock_invoke(&s.contract.address, "resolve_dispute", ().into_val(&s.env));
    let stranger = Address::generate(&s.env);

    // A stranger's signature satisfies none of the three role checks.
    s.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &invoke,
    }]);
    assert!(s.contract.try_resolve_dispute().is_err());
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // The one real principal's signature satisfies all three.
    s.env.mock_auths(&[MockAuth {
        address: &s.buyer,
        invoke: &invoke,
    }]);
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Locked);
}

/// #141 — an auth entry for one method must not authorize a different method on
/// the same contract instance.
///
/// A warehouse-operator entry scoped to `confirm_delivery` is rejected when
/// presented to `mark_checkpoint`, even though both are warehouse-gated methods
/// on this same contract. Only an entry scoped to `mark_checkpoint` is accepted.
#[test]
fn auth_entry_for_one_method_cannot_authorize_a_different_method() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();

    let wrong = mock_invoke(
        &s.contract.address,
        "confirm_delivery",
        (CONTRACTED_QUANTITY, FULL_PRICE_GRADE).into_val(&s.env),
    );
    s.env.mock_auths(&[MockAuth {
        address: &s.warehouse,
        invoke: &wrong,
    }]);
    let result = s.contract.try_mark_checkpoint();
    assert!(
        result.is_err(),
        "a confirm_delivery-scoped entry must not authorize mark_checkpoint: {result:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Advance1Released);

    let right = mock_invoke(&s.contract.address, "mark_checkpoint", ().into_val(&s.env));
    s.env.mock_auths(&[MockAuth {
        address: &s.warehouse,
        invoke: &right,
    }]);
    s.contract.mark_checkpoint();
    assert_eq!(s.contract.get_status(), Status::CheckpointPassed);
}

/// #141, the argument half: an auth entry is bound to the exact serialized
/// arguments of the invocation, not just the method name.
///
/// Consent to reassign the buyer position to one address must not authorize
/// reassigning it to a different address.
#[test]
fn auth_entry_for_one_argument_set_cannot_authorize_a_different_argument_set() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    let approved = Address::generate(&s.env);
    let substituted = Address::generate(&s.env);

    // Every signer consents to `reassign_buyer(approved)` ...
    let for_approved = mock_invoke(
        &s.contract.address,
        "reassign_buyer",
        (approved.clone(),).into_val(&s.env),
    );
    s.env.mock_auths(&[
        MockAuth {
            address: &s.buyer,
            invoke: &for_approved,
        },
        MockAuth {
            address: &s.cooperative,
            invoke: &for_approved,
        },
        MockAuth {
            address: &approved,
            invoke: &for_approved,
        },
    ]);

    // ... but the call substitutes a different `new_buyer`. The entries do not
    // match the actual invocation's arguments, so it must be rejected.
    let result = s.contract.try_reassign_buyer(&substituted);
    assert!(
        result.is_err(),
        "consent for a different new_buyer argument must not transfer: {result:?}"
    );
    assert_eq!(s.contract.get_commitment().buyer, s.buyer);

    // The original, correctly-scoped consent still works.
    s.env.mock_auths(&[
        MockAuth {
            address: &s.buyer,
            invoke: &for_approved,
        },
        MockAuth {
            address: &s.cooperative,
            invoke: &for_approved,
        },
        MockAuth {
            address: &approved,
            invoke: &for_approved,
        },
    ]);
    s.contract.reassign_buyer(&approved);
    assert_eq!(s.contract.get_commitment().buyer, approved);
}

#[test]
fn commitment_struct_footprint_and_field_invariants() {
    let s = setup(1_500, 2_000);
    let c = s.contract.get_commitment();

    // Verify all fields initialize to their precise expected sentinels and baseline terms
    assert_eq!(c.buyer, s.buyer);
    assert_eq!(c.cooperative, s.cooperative);
    assert_eq!(c.warehouse_operator, s.warehouse);
    assert_eq!(c.token, s.token.address);
    assert_eq!(c.total_amount, s.total_amount);
    assert_eq!(c.advance1_bps, 1_500);
    assert_eq!(c.advance2_bps, 2_000);
    assert_eq!(c.claim_window_secs, WINDOW);
    assert_eq!(c.remainder_window_secs, REMAINDER_WINDOW);
    assert_eq!(c.status, Status::Draft);
    assert_eq!(
        c.delivery_deadline,
        s.env.ledger().timestamp() + DELIVERY_WINDOW
    );
    assert_eq!(c.advance1_deadline, 0);
    assert!(!c.advance1_claimed);
    assert!(!c.advance1_expired);
    assert_eq!(c.advance2_deadline, 0);
    assert!(!c.advance2_claimed);
    assert!(!c.advance2_expired);
    assert_eq!(c.remainder_deadline, 0);
    assert!(!c.remainder_funded);
    assert_eq!(c.contracted_quantity, CONTRACTED_QUANTITY);
    assert_eq!(c.grade_price_bps.len(), 3);
    assert_eq!(c.delivered_quantity, 0);
    assert_eq!(c.grade_index, 0);
    assert_eq!(c.settlement_bps, 0);
    assert!(!c.fx_resolved);
    assert_eq!(c.fx_adjusted_total, 0);
    assert_eq!(c.fx_shortfall_amount, 0);
    assert!(!c.fx_shortfall_funded);
    assert_eq!(c.fx_shortfall_deadline, 0);
    assert_eq!(c.dispute_pre_status, Status::Draft);
    assert_eq!(c.dispute_deadline, 0);
}

// ---------- edge cases #130-133 ----------

#[test]
fn set_allocation_with_duplicate_member_hashes_is_stored_verbatim() {
    // Edge case #130: two allocation members whose phone numbers hash to
    // the same BytesN<32> under different salts. A genuine collision is
    // astronomically unlikely, but set_allocation performs no
    // dedup/uniqueness check on member_hash — it only rejects an empty
    // list or a share_bps sum over 10_000. Confirm both identical-hash
    // entries are silently accepted and stored verbatim.
    let s = setup(1_500, 1_500);
    let colliding_hash = BytesN::random(&s.env); // stand-in for the colliding hash
    let members = Vec::from_array(
        &s.env,
        [
            AllocationMember {
                member_hash: colliding_hash.clone(),
                share_bps: 5_000,
            },
            AllocationMember {
                member_hash: colliding_hash.clone(),
                share_bps: 5_000,
            },
        ],
    );

    s.contract.set_allocation(&members);

    let stored = s.contract.get_allocation();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored.get(0).unwrap().member_hash, colliding_hash);
    assert_eq!(stored.get(1).unwrap().member_hash, colliding_hash);
    assert_eq!(stored.get(0).unwrap().share_bps, 5_000);
    assert_eq!(stored.get(1).unwrap().share_bps, 5_000);
}

#[test]
fn release_and_claim_advance_1_with_zero_bps_moves_no_funds() {
    // Edge case #131: advance1_bps = 0 is valid per the
    // advance1_bps + advance2_bps <= 10_000 rule. Releasing the zero-sized
    // tranche opens its window normally, and claiming succeeds while
    // moving no funds — the zero amount is skipped, not treated as an
    // error — so the rest of the lifecycle proceeds unchanged.
    let s = setup(0, 2_000);
    s.contract.lock();
    let deposit = deposit_amount(s.total_amount, 0, 2_000);
    assert_eq!(deposit, s.total_amount * 2_000 / 10_000);

    s.contract.release_advance_1();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(
        s.contract.get_commitment().advance1_deadline,
        s.env.ledger().timestamp() + WINDOW
    );

    s.contract.claim_advance_1();

    assert!(s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), deposit);

    // The zero tranche resolves like any other: the full lifecycle still
    // completes and pays out exactly total_amount.
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
    assert_eq!(s.token.balance(&s.buyer), 0);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn fund_remainder_exact_outstanding_balance_is_accepted() {
    // Edge case #132: fund_remainder takes no amount argument — it always
    // computes remainder = total_amount - deposit internally, so
    // over/under-payment is unreachable by construction (there is no
    // caller-supplied amount to be "close enough", unlike the FX-adjusted
    // path which derives its figure from a live oracle rate). This test
    // pins the exact-match behavior: the buyer is debited exactly the
    // outstanding remainder, to the stroop.
    let s = setup(1_500, 2_000); // 35% deposit, 65% remainder
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();

    let deposit = deposit_amount(s.total_amount, 1_500, 2_000);
    let remainder = s.total_amount - deposit;
    assert_eq!(remainder, 650_000); // exact figure, not an approximation
    let buyer_before = s.token.balance(&s.buyer);
    assert_eq!(buyer_before, s.total_amount - deposit);

    s.contract.fund_remainder();

    assert_eq!(s.token.balance(&s.buyer), buyer_before - remainder);
    assert!(s.contract.get_commitment().remainder_funded);
    // Contract now holds the full total less what the cooperative already
    // claimed — every stroop accounted for, nothing created or destroyed.
    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    assert_eq!(
        s.token.balance(&s.contract.address),
        s.total_amount - advance1_amount - advance2_amount
    );
}

#[test]
fn dispute_resolve_and_expire_race_leaves_consistent_state_regardless_of_order() {
    // Edge case #133: expire_dispute_window and resolve_dispute racing for
    // the same dispute. Soroban executes one transaction first; the winner
    // restores the pre-dispute status and clears the dispute fields, so the
    // loser must fail with InvalidState (status is no longer Disputed) —
    // the end state is identical regardless of submission order, not an
    // accident of which call landed first.

    // Order A: resolve_dispute wins (before the deadline), expire loses.
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.flag_dispute(&s.buyer);

    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Advance1Released);

    advance_time(&s, REMAINDER_WINDOW + 1); // past when the dispute window would have lapsed
    let loser = s.contract.try_expire_dispute_window();
    assert_eq!(loser, Err(Ok(Error::InvalidState)));
    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Draft
    );
    assert_eq!(s.contract.get_commitment().dispute_deadline, 0);

    // Order B: expire_dispute_window wins (after the deadline), resolve loses.
    let s2 = setup(1_500, 1_500);
    s2.contract.lock();
    s2.contract.release_advance_1();
    s2.contract.flag_dispute(&s2.buyer);
    advance_time(&s2, REMAINDER_WINDOW + 1);

    s2.contract.expire_dispute_window();
    assert_eq!(s2.contract.get_status(), Status::Advance1Released);

    let loser2 = s2.contract.try_resolve_dispute();
    assert_eq!(loser2, Err(Ok(Error::InvalidState)));
    assert_eq!(s2.contract.get_status(), Status::Advance1Released);
    assert_eq!(
        s2.contract.get_commitment().dispute_pre_status,
        Status::Draft
    );
    assert_eq!(s2.contract.get_commitment().dispute_deadline, 0);
}

// ---------- status_changed events (#69) ----------

/// The XDR form of one `status_changed` event, the encoding
/// `env.events().all()` compares against — this asserts on the exact
/// `(from, to)` pair the contract published, not just "an event fired".
fn status_event(s: &Setup, from: Status, to: Status) -> xdr::ContractEvent {
    StatusChanged { from, to }.to_xdr(&s.env, &s.contract.address)
}

/// Asserts the escrow contract's most recent invocation published exactly
/// one `status_changed` event, `from -> to` — `env.events().all()` scopes
/// to the last invocation, so this doubles as the "and nothing else was
/// published by that call" check. Filtered by contract because the SAC
/// token's own transfer events land in the same stream whenever funds move.
fn assert_transition(s: &Setup, from: Status, to: Status) {
    assert_eq!(
        s.env.events().all().filter_by_contract(&s.contract.address),
        [status_event(s, from, to)]
    );
}

/// Asserts the escrow contract's most recent invocation published no
/// `status_changed` — a call moved funds, set a clock, or created the
/// commitment without being a state transition.
fn assert_no_transition(s: &Setup) {
    assert_eq!(
        s.env
            .events()
            .all()
            .filter_by_contract(&s.contract.address)
            .events(),
        []
    );
}

#[test]
fn every_transition_publishes_status_changed() {
    // Walks the full lifecycle asserting each call's event immediately.
    // Because each call is checked right after it runs, the chain is also
    // verified in order: every assertion's `from` is the previous
    // assertion's `to`.
    let s = setup(1_500, 2_000);
    s.contract.lock();
    assert_transition(&s, Status::Draft, Status::Locked);
    s.contract.release_advance_1();
    assert_transition(&s, Status::Locked, Status::Advance1Released);
    s.contract.claim_advance_1();
    assert_no_transition(&s); // moves funds, status unchanged
    s.contract.mark_checkpoint();
    assert_transition(&s, Status::Advance1Released, Status::CheckpointPassed);
    s.contract.release_advance_2();
    assert_transition(&s, Status::CheckpointPassed, Status::Advance2Released);
    s.contract.claim_advance_2();
    assert_no_transition(&s);
    s.contract.ready_for_delivery();
    assert_transition(&s, Status::Advance2Released, Status::ReadyForDelivery);
    s.contract.fund_remainder();
    assert_no_transition(&s);
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    assert_transition(&s, Status::ReadyForDelivery, Status::Delivered);
    s.contract.settle();
    assert_transition(&s, Status::Delivered, Status::Settled);
}

#[test]
fn dispute_freeze_and_resume_publish_status_changed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.flag_dispute(&s.buyer);
    assert_transition(&s, Status::Locked, Status::Disputed);
    s.contract.resolve_dispute();
    // The resume event's `to` is the restored pre-dispute status, not
    // Draft — the same restoration `status` itself gets, mirrored into
    // the event.
    assert_transition(&s, Status::Disputed, Status::Locked);
}

#[test]
fn unwind_and_default_paths_publish_status_changed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.cancel();
    assert_transition(&s, Status::Locked, Status::Cancelled);

    let s2 = setup(1_500, 1_500);
    s2.contract.lock();
    advance_time(&s2, DELIVERY_WINDOW + 1);
    s2.contract.reclaim_on_nondelivery();
    assert_transition(&s2, Status::Locked, Status::Forfeited);

    // The remainder window lapses without fund_remainder — buyer default.
    // Claims are deliberately skipped: they emit nothing (asserted in
    // every_transition_publishes_status_changed) and no guard on this path
    // depends on them.
    let s3 = setup(1_500, 2_000);
    s3.contract.lock();
    s3.contract.release_advance_1();
    s3.contract.mark_checkpoint();
    s3.contract.release_advance_2();
    s3.contract.ready_for_delivery();
    advance_time(&s3, REMAINDER_WINDOW + 1);
    s3.contract.expire_remainder_window();
    assert_transition(&s3, Status::ReadyForDelivery, Status::Defaulted);
}

#[test]
fn initialize_is_not_a_transition_and_publishes_nothing() {
    // A commitment being created in Draft is an initial state, not a
    // transition into one — see StatusChanged's doc comment. This pins
    // that decision: initialize emits nothing for an off-chain consumer
    // to key a "created" event on, deliberately.
    let s = setup(1_500, 1_500);
    assert_no_transition(&s);
}

// ---------- composition tests (#103-#106) ----------
//
// Each test below pairs two features that are each well covered on their own
// happy path and checks the interaction between them — the cases that fall
// between the two suites, where neither feature exercises the other already
// being in flight.

/// #103 — `reassign_buyer` rewrites only the commitment's `buyer` field. The
/// allocation ledger lives under its own `DataKey::Allocation` instance entry
/// and describes the cooperative's members, not who holds the buyer's
/// position, so a novation must leave it exactly as recorded — and must not
/// fail because of it either.
#[test]
fn reassign_buyer_leaves_a_recorded_allocation_ledger_completely_untouched() {
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(&s.env, [member(&s.env, 6_000), member(&s.env, 4_000)]);
    s.contract.set_allocation(&members);
    s.contract.lock();
    let recorded = s.contract.get_allocation();
    s.contract.release_advance_1();

    let new_buyer = Address::generate(&s.env);
    s.contract.reassign_buyer(&new_buyer);

    // Byte-for-byte the same ledger: same entries, same order, same hashes and
    // same shares. Nothing was copied, re-keyed, or cleared.
    let after = s.contract.get_allocation();
    assert_eq!(after, recorded);
    assert_eq!(after.len(), 2);
    assert_eq!(after.get(0).unwrap(), recorded.get(0).unwrap());
    assert_eq!(after.get(1).unwrap(), recorded.get(1).unwrap());

    // Only `buyer` moved, and the new holder genuinely owns the position's
    // buyer-only rights — the reassignment really landed, while the ledger sat
    // untouched beside it.
    assert_eq!(s.contract.get_commitment().buyer, new_buyer);
    advance_time(&s, WINDOW + 1);
    s.contract.reclaim_advance_1();
    let advance1_amount = s.total_amount * 1_500 / 10_000;
    assert_eq!(s.token.balance(&new_buyer), advance1_amount);
    assert_eq!(s.contract.get_allocation(), recorded);
}

/// #104 — `flag_dispute` from `Delivered` while the FX-shortfall funding window
/// is still open. The freeze blocks `settle` and the shortfall calls
/// themselves (all status-gated), but per `flag_dispute`'s documented
/// limitation it does *not* pause `fx_shortfall_deadline`: that deadline keeps
/// running on absolute time, so a dispute outliving it lands on the default
/// path instead of buying the buyer a free extension.
#[test]
fn flag_dispute_during_an_open_fx_shortfall_window_freezes_settle_without_disturbing_the_deadline()
{
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);
    advance_to_delivered_with_shortfall(&s);
    s.contract.resolve_fx_shortfall();

    let before = s.contract.get_commitment();
    assert_eq!(before.fx_shortfall_amount, EXPECTED_SHORTFALL);
    assert!(!before.fx_shortfall_funded);
    let shortfall_deadline = before.fx_shortfall_deadline;
    assert!(
        shortfall_deadline > s.env.ledger().timestamp(),
        "precondition: the shortfall funding window is still open"
    );

    s.contract.flag_dispute(&s.buyer);
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // Everything the open window would otherwise allow is frozen — by the
    // status gate, before any of the window's own guards get a chance to speak.
    assert_eq!(s.contract.try_settle(), Err(Ok(Error::InvalidState)));
    assert_eq!(
        s.contract.try_fund_fx_shortfall(),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        s.contract.try_expire_fx_shortfall_window(),
        Err(Ok(Error::InvalidState))
    );

    // …but only `status` was frozen. The shortfall's own deadline and amount
    // are exactly as they were: nothing extended, no clock reset.
    let frozen = s.contract.get_commitment();
    assert_eq!(frozen.fx_shortfall_deadline, shortfall_deadline);
    assert_eq!(frozen.fx_shortfall_amount, EXPECTED_SHORTFALL);
    assert!(!frozen.fx_shortfall_funded);
    assert_eq!(frozen.dispute_pre_status, Status::Delivered);

    // Let the dispute lapse *after* the shortfall deadline passed.
    advance_time(&s, REMAINDER_WINDOW + 1);
    s.contract.expire_dispute_window();
    assert_eq!(s.contract.get_status(), Status::Delivered);

    // Curing the shortfall is now too late: the deadline kept running through
    // the freeze, which is the limitation, checked for this specific pair.
    assert_eq!(
        s.contract.try_fund_fx_shortfall(),
        Err(Ok(Error::FxShortfallWindowPassed))
    );

    // And the consequence that deadline was always gating is reachable: the
    // escrow sweeps to the cooperative and the buyer defaults.
    s.contract.expire_fx_shortfall_window();
    assert_eq!(s.contract.get_status(), Status::Defaulted);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    // Deposit + remainder escrowed (1_000_000) less the 300_000 the
    // cooperative already claimed across the two tranches.
    assert_eq!(s.token.balance(&s.cooperative), 1_000_000);
}

/// #105 — `cancel` after an allocation has been recorded. `cancel` unwinds the
/// *commitment* — its status and whatever balance is escrowed — and knows
/// nothing about `DataKey::Allocation`; cleaning that up is not its job. The
/// recorded hashes therefore outlive the deal they described and simply become
/// meaningless: still on-chain, still unlinkable from people (the salts live
/// off-chain), and permanently orphaned — not an error, and not a reason for
/// `cancel` to fail.
#[test]
fn cancel_after_an_allocation_is_recorded_leaves_the_ledger_orphaned_not_an_error() {
    let s = setup(1_500, 1_500);
    let members = Vec::from_array(&s.env, [member(&s.env, 6_000), member(&s.env, 4_000)]);
    s.contract.set_allocation(&members);
    s.contract.lock();
    let recorded = s.contract.get_allocation();

    // Cancel with an escrowed deposit and an allocation already on record.
    s.contract.cancel();

    // The unwind succeeded as if no allocation existed at all — it was never
    // consulted, so it could not block or alter the cancellation.
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
    assert_eq!(s.token.balance(&s.cooperative), 0);

    // The entry survived untouched: `get_allocation` still answers with the
    // same ledger even though the commitment it belonged to is dead.
    let after = s.contract.get_allocation();
    assert_eq!(after, recorded);
    assert_eq!(after.len(), 2);
    assert_eq!(after.get(0).unwrap(), recorded.get(0).unwrap());

    // And it was not swept away as part of the unwind — there is no cleanup
    // path for it, so the orphan is permanent. Read through the contract's own
    // storage context, since the entry is instance-scoped to it.
    let still_recorded = s.env.as_contract(&s.contract.address, || {
        s.env.storage().instance().has(&DataKey::Allocation)
    });
    assert!(still_recorded);

    // A second `set_allocation` still rejects, but with `InvalidState`: the
    // terminal status is checked first, so the allocation guard is never even
    // reached. The ledger is unreachable-by-writing as well as meaningless.
    let other = Vec::from_array(&s.env, [member(&s.env, 10_000)]);
    assert_eq!(
        s.contract.try_set_allocation(&other),
        Err(Ok(Error::InvalidState))
    );
    // Cancellation is terminal on its own account, not because of the ledger.
    assert_eq!(s.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    assert_eq!(s.contract.try_lock(), Err(Ok(Error::InvalidState)));
}

/// #106 — `resolve_dispute` restoring the hardest pre-dispute status:
/// `ReadyForDelivery`, i.e. mid-way through the two-phase-funding window with
/// the remainder still unfunded. The freeze lands on top of a clock only the
/// buyer can satisfy, so restoring must bring back that exact window state —
/// same absolute deadline, still unfunded — not just a plausible-looking
/// status. The simpler earlier states have their own coverage; this is the one
/// where a restored-but-broken window would silently cost the cooperative the
/// deal.
#[test]
fn resolve_dispute_restores_a_ready_for_delivery_commitment_still_inside_its_funding_window() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();

    let mid_window = s.contract.get_commitment();
    assert_eq!(mid_window.status, Status::ReadyForDelivery);
    assert!(!mid_window.remainder_funded);
    let remainder_deadline = mid_window.remainder_deadline;
    assert!(
        remainder_deadline > s.env.ledger().timestamp(),
        "precondition: the remainder funding window is still open"
    );

    // Freeze it while the buyer's outstanding funding obligation is live.
    s.contract.flag_dispute(&s.warehouse);
    assert_eq!(s.contract.get_status(), Status::Disputed);
    assert_eq!(
        s.contract.try_fund_remainder(),
        Err(Ok(Error::InvalidState))
    );

    // Unanimous resolution restores the precise mid-window state.
    s.contract.resolve_dispute();
    let restored = s.contract.get_commitment();
    assert_eq!(restored.status, Status::ReadyForDelivery);
    assert_eq!(restored.dispute_pre_status, Status::Draft);
    assert_eq!(restored.dispute_deadline, 0);
    // Same absolute deadline — the dispute neither paused nor extended it…
    assert_eq!(restored.remainder_deadline, remainder_deadline);
    // …and the funding flag survived the round trip as still-unfunded.
    assert!(!restored.remainder_funded);

    // The restored state is live, not merely displayable: the buyer can still
    // meet the obligation inside the window it always had, and the deal then
    // runs to settlement as it would have without the dispute.
    s.contract.fund_remainder();
    assert!(s.contract.get_commitment().remainder_funded);

    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);
    s.contract.settle();
    assert_eq!(s.contract.get_status(), Status::Settled);

    // Both tranches (300_000) plus the remainder (700_000) all land with the
    // cooperative at full delivery and full grade; nothing is stranded.
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
    assert_eq!(s.token.balance(&s.buyer), 0);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

// ---------------------------------------------------------------------------
// Mutation-testing evidence (#62): each of these kills a mutant that
// survived the full cargo-mutants run. See docs/mutation-testing.md.
// ---------------------------------------------------------------------------

#[test]
fn advance_bps_summing_to_exactly_10000_is_accepted_at_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let (token_address, _token, _admin) = create_token(&env, &token_admin);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    // 5_000 + 5_000 == 10_000: the whole total is advances, no remainder.
    // `initialize` must accept this -- only a sum *over* 10_000 is invalid.
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &1_000_000,
        &5_000,
        &5_000,
        &WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, GRADE_PRICE_BPS),
        &None,
    );
}

#[test]
fn resolve_fx_shortfall_rejects_while_the_first_tranche_is_still_open() {
    let (s, _oracle) = setup_with_oracle(3_600);
    s.contract.lock();
    // Advance 1 is released but never claimed or reclaimed: still open.
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    let c = s.contract.get_commitment();
    assert!(!c.advance1_claimed && !c.advance1_expired);
    assert!(c.advance2_claimed);

    // Both-tranches-open is already covered; this asserts tranche 1's own
    // guard rather than one that its sibling could answer for it.
    let result = s.contract.try_resolve_fx_shortfall();
    assert_eq!(result, Err(Ok(Error::TrancheUnresolved)));
}

// #81 — Kitchen-sink integration test: every optional feature active on one commitment.
// Drives a single commitment through its full lifecycle with:
//   - oracle_config set (FX conversion)
//   - allocation ledger populated
//   - both tranches released and claimed
//   - a mid-lifecycle dispute flagged and resolved
//   - FX shortfall computed, funded, and settled
//   - partial delivery with grade adjustment
#[test]
fn kitchen_sink_all_optional_features_on_one_commitment() {
    // Use SHORTFALL_DENOMINATION so resolve_fx_shortfall actually finds a gap
    let (s, _oracle) = setup_with_oracle_denomination(3_600, SHORTFALL_DENOMINATION);

    // Set allocation ledger while still in Draft (required before lock)
    let m1 = member(&s.env, 6_000);
    let m2 = member(&s.env, 4_000);
    s.contract
        .set_allocation(&Vec::from_array(&s.env, [m1, m2]));

    // Lock the commitment (Draft → Locked)
    s.contract.lock();

    // Release and claim advance 1 (Locked → Advance1Released)
    s.contract.release_advance_1();
    s.contract.claim_advance_1();

    // Mark checkpoint (Advance1Released → CheckpointPassed)
    s.contract.mark_checkpoint();

    // Release and claim advance 2 (CheckpointPassed → Advance2Released)
    s.contract.release_advance_2();
    s.contract.claim_advance_2();

    // Flag a dispute mid-lifecycle (Advance2Released → Disputed)
    s.contract.flag_dispute(&s.buyer);
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // Resolve the dispute — should restore pre-dispute status (Disputed → Advance2Released)
    s.contract.resolve_dispute();
    assert_eq!(s.contract.get_status(), Status::Advance2Released);

    // Signal ready for delivery (Advance2Released → ReadyForDelivery)
    s.contract.ready_for_delivery();

    // Fund remainder within the window
    s.contract.fund_remainder();

    // Confirm full delivery at full price grade (ReadyForDelivery → Delivered)
    s.contract
        .confirm_delivery(&CONTRACTED_QUANTITY, &FULL_PRICE_GRADE);

    // Resolve FX shortfall (reads fresh oracle rate, computes what buyer owes)
    s.contract.resolve_fx_shortfall();

    // Verify a shortfall was actually found
    let c = s.contract.get_commitment();
    assert!(c.fx_resolved);
    assert!(c.fx_shortfall_amount > 0);
    assert!(!c.fx_shortfall_funded);

    // Fund the FX shortfall
    s.contract.fund_fx_shortfall();

    // Settle — should complete successfully with all features exercised (Delivered → Settled)
    s.contract.settle();
    assert_eq!(s.contract.get_status(), Status::Settled);

    // Verify allocation was recorded
    let alloc = s.contract.get_allocation();
    assert_eq!(alloc.len(), 2);
}
