//! #251 — Executable authorization matrix: table-driven test that for every
//! public method in a state where it succeeds calls it under `mock_all_auths()`
//! and asserts `env.auths()` equals exactly the expected signer set from
//! docs/authorization-matrix.md §1.
//!
//! **What this proves**: that every require_auth() site matches the matrix, and
//! that permissionless methods record no escrow-contract auth at all — not just
//! that the expected signer "appears somewhere", but that no extra signer does.
//!
//! **Why env.auths() is the right tool**: `mock_all_auths()` satisfies every
//! require_auth() call and records each one as a `(signer, invocation)` tuple.
//! Filtering to invocations on the escrow contract eliminates token-SAC
//! sub-invocations and lets us compare the escrow-level signer set directly
//! against the matrix.
//!
//! For oracle-dependent methods (resolve_fx_shortfall, fund_fx_shortfall,
//! expire_fx_shortfall_window) a minimal stand-in oracle is wired in — same
//! cross-contract matching technique as src/test.rs.

#[path = "common/mod.rs"]
mod common;

use common::{world, World, CONTRACTED_QUANTITY, DELIVERY_WINDOW, GRADE_PRICE_BPS, WINDOW};
use harvestlock_escrow::{
    AllocationMember, CommitmentTerms, EscrowContract, EscrowContractClient, OracleConfig,
};
use soroban_sdk::testutils::{Address as _, AuthorizedFunction};
use soroban_sdk::{token, Address, BytesN, Env, Symbol};

// ---------------------------------------------------------------------------
// Minimal oracle stand-in (same wire shape as src/test.rs::mock_oracle)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Helper: compare the escrow-contract signers from env.auths() exactly
// ---------------------------------------------------------------------------

/// Extract only the top-level signers whose authorized function targets the
/// escrow contract (not token SAC sub-invocations, which are nested inside
/// the escrow's auth entry and do not appear as separate top-level entries).
fn escrow_signers(env: &Env, contract_id: &Address) -> std::vec::Vec<Address> {
    env.auths()
        .into_iter()
        .filter(|(_, inv)| {
            matches!(
                &inv.function,
                AuthorizedFunction::Contract((id, _, _)) if id == contract_id
            )
        })
        .map(|(signer, _)| signer)
        .collect()
}

/// Assert that after the last contract call, `env.auths()` names exactly the
/// signers in `expected` for the escrow contract — no more, no less.
fn assert_exact_signers(env: &Env, contract_id: &Address, expected: &[&Address], method: &str) {
    let got = escrow_signers(env, contract_id);
    assert_eq!(
        got.len(),
        expected.len(),
        "auth mismatch for `{method}`: got {} signers, expected {}",
        got.len(),
        expected.len()
    );
    for addr in expected {
        assert!(
            got.iter().any(|g| g == *addr),
            "auth mismatch for `{method}`: expected signer not found in env.auths()"
        );
    }
    for g in &got {
        assert!(
            expected.iter().any(|e| *e == g),
            "auth mismatch for `{method}`: unexpected signer found in env.auths()"
        );
    }
}

// ---------------------------------------------------------------------------
// Drive helpers
// ---------------------------------------------------------------------------

fn drive_to_locked(w: &World) {
    w.contract.lock();
}

fn drive_to_advance1_released(w: &World) {
    w.contract.lock();
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

fn drive_to_disputed(w: &World) {
    drive_to_locked(w);
    w.contract.flag_dispute(&w.buyer);
}

// ---------------------------------------------------------------------------
// §1 matrix — permissionless methods (zero escrow-contract auths)
// ---------------------------------------------------------------------------

#[test]
fn release_advance_1_is_permissionless() {
    let w = world(1_500, 2_000);
    w.contract.lock();
    let _ = w.env.auths();
    w.contract.release_advance_1();
    assert_exact_signers(&w.env, &w.contract.address, &[], "release_advance_1");
}

#[test]
fn release_advance_2_is_permissionless() {
    let w = world(1_500, 2_000);
    drive_to_checkpoint_passed(&w);
    let _ = w.env.auths();
    w.contract.release_advance_2();
    assert_exact_signers(&w.env, &w.contract.address, &[], "release_advance_2");
}

#[test]
fn expire_remainder_window_is_permissionless() {
    let w = world(1_500, 2_000);
    drive_to_ready_for_delivery(&w);
    w.advance(WINDOW + 1);
    let _ = w.env.auths();
    w.contract.expire_remainder_window();
    assert_exact_signers(&w.env, &w.contract.address, &[], "expire_remainder_window");
}

#[test]
fn settle_is_permissionless() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    let _ = w.env.auths();
    w.contract.settle();
    assert_exact_signers(&w.env, &w.contract.address, &[], "settle");
}

#[test]
fn expire_dispute_window_is_permissionless() {
    let w = world(1_500, 2_000);
    drive_to_disputed(&w);
    w.advance(WINDOW + 1);
    let _ = w.env.auths();
    w.contract.expire_dispute_window();
    assert_exact_signers(&w.env, &w.contract.address, &[], "expire_dispute_window");
}

// ---------------------------------------------------------------------------
// §1 matrix — views (zero escrow-contract auths)
// ---------------------------------------------------------------------------

#[test]
fn views_require_no_auth() {
    let w = world(1_500, 2_000);
    drive_to_delivered(&w);
    let _ = w.env.auths();

    let _ = w.contract.get_status();
    assert_exact_signers(&w.env, &w.contract.address, &[], "get_status");
    let _ = w.env.auths();

    let _ = w.contract.get_commitment();
    assert_exact_signers(&w.env, &w.contract.address, &[], "get_commitment");
    let _ = w.env.auths();

    let _ = w.contract.get_oracle_config();
    assert_exact_signers(&w.env, &w.contract.address, &[], "get_oracle_config");
}

#[test]
fn get_allocation_requires_no_auth() {
    let w = world(1_500, 2_000);
    let member_hash = BytesN::from_array(&w.env, &[1u8; 32]);
    let members = soroban_sdk::vec![
        &w.env,
        AllocationMember {
            member_hash,
            share_bps: 10_000
        }
    ];
    w.contract.set_allocation(&members);
    let _ = w.env.auths();
    let _ = w.contract.get_allocation();
    assert_exact_signers(&w.env, &w.contract.address, &[], "get_allocation");
}

// ---------------------------------------------------------------------------
// §1 matrix — buyer-only methods
// ---------------------------------------------------------------------------

#[test]
fn initialize_requires_exactly_buyer() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = sac.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);
    token_admin_client.mint(&buyer, &1_000_000i128);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);

    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &CommitmentTerms {
            total_amount: 1_000_000,
            advance1_bps: 1_500,
            advance2_bps: 2_000,
            claim_window_secs: WINDOW,
            remainder_window_secs: WINDOW,
            delivery_window_secs: 120 * 24 * 60 * 60,
            contracted_quantity: 1_000,
            grade_price_bps: soroban_sdk::Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &None,
    );
    assert_exact_signers(&env, &contract_id, &[&buyer], "initialize");
}

#[test]
fn lock_requires_exactly_buyer() {
    let w = world(1_500, 2_000);
    let _ = w.env.auths();
    w.contract.lock();
    assert_exact_signers(&w.env, &w.contract.address, &[&w.buyer], "lock");
}

#[test]
fn reclaim_advance_1_requires_exactly_buyer() {
    let w = world(1_500, 2_000);
    drive_to_advance1_released(&w);
    let deadline = w.contract.get_commitment().advance1_deadline;
    w.set_time(deadline + 1);
    let _ = w.env.auths();
    w.contract.reclaim_advance_1();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer],
        "reclaim_advance_1",
    );
}

#[test]
fn reclaim_advance_2_requires_exactly_buyer() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    let deadline = w.contract.get_commitment().advance2_deadline;
    w.set_time(deadline + 1);
    let _ = w.env.auths();
    w.contract.reclaim_advance_2();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer],
        "reclaim_advance_2",
    );
}

#[test]
fn fund_remainder_requires_exactly_buyer() {
    let w = world(1_500, 2_000);
    drive_to_ready_for_delivery(&w);
    let _ = w.env.auths();
    w.contract.fund_remainder();
    assert_exact_signers(&w.env, &w.contract.address, &[&w.buyer], "fund_remainder");
}

#[test]
fn reclaim_on_nondelivery_requires_exactly_buyer() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    w.advance(DELIVERY_WINDOW + 1);
    let _ = w.env.auths();
    w.contract.reclaim_on_nondelivery();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer],
        "reclaim_on_nondelivery",
    );
}

// ---------------------------------------------------------------------------
// §1 matrix — cooperative-only methods
// ---------------------------------------------------------------------------

#[test]
fn set_allocation_requires_exactly_cooperative() {
    let w = world(1_500, 2_000);
    let member_hash = BytesN::from_array(&w.env, &[1u8; 32]);
    let members = soroban_sdk::vec![
        &w.env,
        AllocationMember {
            member_hash,
            share_bps: 10_000
        }
    ];
    let _ = w.env.auths();
    w.contract.set_allocation(&members);
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.cooperative],
        "set_allocation",
    );
}

#[test]
fn claim_advance_1_requires_exactly_cooperative() {
    let w = world(1_500, 2_000);
    drive_to_advance1_released(&w);
    let _ = w.env.auths();
    w.contract.claim_advance_1();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.cooperative],
        "claim_advance_1",
    );
}

#[test]
fn claim_advance_2_requires_exactly_cooperative() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    let _ = w.env.auths();
    w.contract.claim_advance_2();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.cooperative],
        "claim_advance_2",
    );
}

#[test]
fn ready_for_delivery_requires_exactly_cooperative() {
    let w = world(1_500, 2_000);
    drive_to_advance2_released(&w);
    let _ = w.env.auths();
    w.contract.ready_for_delivery();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.cooperative],
        "ready_for_delivery",
    );
}

// ---------------------------------------------------------------------------
// §1 matrix — warehouse-operator-only methods
// ---------------------------------------------------------------------------

#[test]
fn mark_checkpoint_requires_exactly_warehouse() {
    let w = world(1_500, 2_000);
    drive_to_advance1_released(&w);
    let _ = w.env.auths();
    w.contract.mark_checkpoint();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.warehouse],
        "mark_checkpoint",
    );
}

#[test]
fn confirm_delivery_requires_exactly_warehouse() {
    let w = world(1_500, 2_000);
    drive_to_remainder_funded(&w);
    let _ = w.env.auths();
    w.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.warehouse],
        "confirm_delivery",
    );
}

// ---------------------------------------------------------------------------
// §1 matrix — multi-party methods
// ---------------------------------------------------------------------------

#[test]
fn cancel_requires_exactly_buyer_and_cooperative() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    let _ = w.env.auths();
    w.contract.cancel();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer, &w.cooperative],
        "cancel",
    );
}

#[test]
fn reassign_buyer_requires_buyer_cooperative_and_incoming_buyer() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    let new_buyer = Address::generate(&w.env);
    let _ = w.env.auths();
    w.contract.reassign_buyer(&new_buyer);
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer, &w.cooperative, &new_buyer],
        "reassign_buyer",
    );
}

#[test]
fn flag_dispute_requires_exactly_the_flagger_for_each_party() {
    for label in ["buyer", "cooperative", "warehouse"] {
        let w = world(1_500, 2_000);
        drive_to_locked(&w);
        let flagger = match label {
            "buyer" => w.buyer.clone(),
            "cooperative" => w.cooperative.clone(),
            _ => w.warehouse.clone(),
        };
        let _ = w.env.auths();
        w.contract.flag_dispute(&flagger);
        assert_exact_signers(
            &w.env,
            &w.contract.address,
            &[&flagger],
            &format!("flag_dispute({label})"),
        );
    }
}

#[test]
fn resolve_dispute_requires_all_three_parties() {
    let w = world(1_500, 2_000);
    drive_to_disputed(&w);
    let _ = w.env.auths();
    w.contract.resolve_dispute();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer, &w.cooperative, &w.warehouse],
        "resolve_dispute",
    );
}

// ---------------------------------------------------------------------------
// §1 matrix — oracle-wired methods
// ---------------------------------------------------------------------------

/// Drive the given world (already initialized with an oracle config producing a
/// shortfall) to `Status::Delivered` with both tranches claimed.
fn drive_to_delivered_oracle(w: &World) {
    drive_to_delivered(w);
}

fn setup_with_shortfall() -> (World, mock_oracle::MockOracleClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);

    let oracle_id = env.register(mock_oracle::MockOracle, ());
    let oracle = mock_oracle::MockOracleClient::new(&env, &oracle_id);
    // rate = 1.0 (100_000_000_000_000 / 10^14), denominated_amount = 1_500_000.
    // ngn_owed = 1_500_000, fx_adjusted_total = 1_500_000.
    // After full delivery: claimed_by_coop = 350_000, available_balance = 650_000.
    // shortfall = (1_500_000 - 350_000) - 650_000 = 500_000 → buyer must top up.
    oracle.set_price(&100_000_000_000_000i128, &0u64);

    let oracle_config = OracleConfig {
        oracle_contract: oracle_id.clone(),
        price_asset: Symbol::new(&env, "GBP"),
        max_age_secs: 3_600,
        denominated_amount: 1_500_000,
    };

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = sac.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);
    token_admin_client.mint(&buyer, &2_000_000i128); // extra for fx shortfall

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &CommitmentTerms {
            total_amount: 1_000_000,
            advance1_bps: 1_500,
            advance2_bps: 2_000,
            claim_window_secs: WINDOW,
            remainder_window_secs: WINDOW,
            delivery_window_secs: 120 * 24 * 60 * 60,
            contracted_quantity: 1_000,
            grade_price_bps: soroban_sdk::Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &Some(oracle_config),
    );

    let w = World {
        env: env.clone(),
        contract,
        token: token::Client::new(&env, &token_address),
        token_admin: token_admin_client,
        issuer: Some(sac.issuer()),
        buyer,
        cooperative,
        warehouse,
        total_amount: 1_000_000,
    };

    (w, oracle)
}

#[test]
fn oracle_rate_requires_no_auth() {
    let (w, _oracle) = setup_with_shortfall();
    let _ = w.env.auths();
    let _ = w.contract.oracle_rate();
    assert_exact_signers(&w.env, &w.contract.address, &[], "oracle_rate");
}

#[test]
fn resolve_fx_shortfall_is_permissionless() {
    let (w, _oracle) = setup_with_shortfall();
    drive_to_delivered_oracle(&w);
    let _ = w.env.auths();
    w.contract.resolve_fx_shortfall();
    assert_exact_signers(&w.env, &w.contract.address, &[], "resolve_fx_shortfall");
}

#[test]
fn fund_fx_shortfall_requires_exactly_buyer() {
    let (w, _oracle) = setup_with_shortfall();
    drive_to_delivered_oracle(&w);
    w.contract.resolve_fx_shortfall();
    let _ = w.env.auths();
    w.contract.fund_fx_shortfall();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[&w.buyer],
        "fund_fx_shortfall",
    );
}

#[test]
fn expire_fx_shortfall_window_is_permissionless() {
    let (w, _oracle) = setup_with_shortfall();
    drive_to_delivered_oracle(&w);
    w.contract.resolve_fx_shortfall();
    let shortfall_deadline = w.contract.get_commitment().fx_shortfall_deadline;
    w.set_time(shortfall_deadline + 1);
    let _ = w.env.auths();
    w.contract.expire_fx_shortfall_window();
    assert_exact_signers(
        &w.env,
        &w.contract.address,
        &[],
        "expire_fx_shortfall_window",
    );
}

// ---------------------------------------------------------------------------
// Cross-check: a permissionless call that moves funds still records no
// escrow-contract require_auth at all
// ---------------------------------------------------------------------------

#[test]
fn permissionless_fund_mover_has_no_escrow_contract_auth() {
    // expire_remainder_window sweeps tokens to the cooperative, so funds move
    // — but no party's require_auth() is called in the escrow contract.
    let w = world(1_500, 2_000);
    drive_to_ready_for_delivery(&w);
    w.advance(WINDOW + 1);
    let _ = w.env.auths();

    w.contract.expire_remainder_window();

    let escrow_auths_count = w
        .env
        .auths()
        .into_iter()
        .filter(|(_, inv)| {
            matches!(
                &inv.function,
                AuthorizedFunction::Contract((id, _, _)) if id == &w.contract.address
            )
        })
        .count();
    assert_eq!(
        escrow_auths_count, 0,
        "expire_remainder_window must record no escrow-contract require_auth"
    );
}
