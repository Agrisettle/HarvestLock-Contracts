#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

fn create_token<'a>(env: &Env, admin: &Address) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (
        address.clone(),
        token::Client::new(env, &address),
        token::StellarAssetClient::new(env, &address),
    )
}

struct Setup<'a> {
    contract: EscrowContractClient<'a>,
    token: token::Client<'a>,
    token_admin: token::StellarAssetClient<'a>,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    total_amount: i128,
}

fn setup(advance1_bps: u32, advance2_bps: u32) -> Setup<'static> {
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
    );

    Setup {
        contract,
        token,
        token_admin: token_admin_client,
        buyer,
        cooperative,
        warehouse,
        total_amount,
    }
}

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
    );
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn lock_transfers_full_deposit_into_contract() {
    let s = setup(1_500, 1_500);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
    assert_eq!(s.token.balance(&s.contract.address), 0);

    s.contract.lock();

    assert_eq!(s.contract.get_status(), Status::Locked);
    assert_eq!(s.token.balance(&s.buyer), 0);
    assert_eq!(s.token.balance(&s.contract.address), s.total_amount);
}

#[test]
fn release_advance_1_pays_cooperative_the_correct_bps() {
    let s = setup(1_500, 2_000); // 15% then 20%
    s.contract.lock();

    s.contract.release_advance_1();

    let expected = s.total_amount * 1_500 / 10_000;
    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(s.token.balance(&s.cooperative), expected);
    assert_eq!(
        s.token.balance(&s.contract.address),
        s.total_amount - expected
    );
}

#[test]
fn full_happy_path_reaches_settled_and_pays_out_exactly_total_amount() {
    let s = setup(1_500, 2_000); // 15% + 20% = 35%, so 65% remains at settlement
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.confirm_delivery();
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    let _ = s.token_admin; // silence unused-field warning; kept for clarity/future tests
}

#[test]
fn cannot_release_advance_1_before_lock() {
    let s = setup(1_500, 1_500);
    let result = s.contract.try_release_advance_1();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn cannot_settle_before_delivery_confirmed() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();

    let result = s.contract.try_settle();
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn zero_advance_bps_is_valid_and_pays_nothing_at_that_step() {
    let s = setup(0, 0);
    s.contract.lock();
    s.contract.release_advance_1();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), s.total_amount);
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
        &5_000, // 110% total
    );
    assert_eq!(result, Err(Ok(Error::InvalidBps)));
}
