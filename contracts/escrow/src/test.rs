#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::Env;

const WINDOW: u64 = 7 * 24 * 60 * 60; // 7 days, arbitrary but realistic

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
        &buyer, &cooperative, &warehouse, &token_address, &1_000_000, &1_000, &1_000, &0,
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
        &buyer, &cooperative, &warehouse, &token_address, &1_000_000, &6_000, &5_000, &WINDOW,
    );
    assert_eq!(result, Err(Ok(Error::InvalidBps)));
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

// ---------- opening a tranche moves no funds ----------

#[test]
fn release_advance_1_opens_window_but_transfers_nothing() {
    let s = setup(1_500, 2_000);
    s.contract.lock();

    s.contract.release_advance_1();

    assert_eq!(s.contract.get_status(), Status::Advance1Released);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), s.total_amount);
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
    assert_eq!(s.token.balance(&s.cooperative), expected);
    assert_eq!(s.token.balance(&s.contract.address), s.total_amount - expected);
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
    assert_eq!(s.token.balance(&s.buyer), expected);
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), s.total_amount - expected);
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
    s.contract.confirm_delivery();

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
    s.contract.confirm_delivery();

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
    s.contract.confirm_delivery();
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
    s.contract.confirm_delivery();
    s.contract.settle();

    let advance1_amount = s.total_amount * 1_500 / 10_000;
    let advance2_amount = s.total_amount * 2_000 / 10_000;
    let remainder = s.total_amount - advance1_amount - advance2_amount;

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.buyer), advance1_amount);
    assert_eq!(s.token.balance(&s.cooperative), advance2_amount + remainder);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    // Everything that started in the contract is accounted for somewhere.
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
    s.contract.confirm_delivery();

    let blocked = s.contract.try_settle();
    assert_eq!(blocked, Err(Ok(Error::TrancheUnresolved)));

    s.contract.claim_advance_1();
    s.contract.claim_advance_2();
    s.contract.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.cooperative), s.total_amount);
}

// ---------- auth gating ----------

#[test]
fn confirm_delivery_requires_warehouse_operator() {
    let s = setup(1_500, 1_500);
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();

    // mock_all_auths() means this test can't check *rejection* of a wrong
    // signer (everything auths successfully in that mode) — what it does
    // confirm is that the call path genuinely requires an auth to exist at
    // all, by checking the recorded auth trace names the warehouse address.
    s.contract.confirm_delivery();
    let auths = s.env.auths();
    let touched_warehouse = auths.iter().any(|(addr, _)| *addr == s.warehouse);
    assert!(touched_warehouse, "expected warehouse operator auth on confirm_delivery");
}
