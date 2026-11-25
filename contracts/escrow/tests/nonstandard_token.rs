//! #264 — non-standard SEP-41 token suite.
//!
//! `initialize`'s `token` parameter takes any `Address`. The threat model
//! calls a *wrong* address "buyer-borne" (entry 30), but a token that *looks*
//! like SEP-41 and nevertheless misbehaves slips past that label: the buyer
//! did pick an address with the expected interface — it just doesn't honor
//! the contract's assumption that `transfer(x)` moves exactly `x` and
//! `balance()` reports what the contract actually holds. The harm there
//! lands on the cooperative, not the buyer who chose the address, which is
//! what makes "use a Stellar Asset Contract (SAC)" a documented requirement
//! rather than folklore.
//!
//! Two deliberately non-standard mock tokens are wired up here, each
//! implementing the subset of SEP-41 the escrow actually uses (`balance`,
//! `transfer`) — Soroban matches cross-contract calls on XDR shape, so
//! that subset is enough for `lock`/`fund_remainder`/`settle`/`cancel`/
//! `withdraw_payout` to
//! run exactly as they would against a real SAC:
//!
//! 1. **Fee-on-transfer**: each `transfer(amount)` credits the receiver
//!    `amount - FEE`. The sender loses `amount`; the token itself keeps
//!    `FEE`. This is a close relative of several real-world ERC-20 designs
//!    (reflection/deflationary tokens); the SEP-41 interface has no concept
//!    of it, so from the escrow's perspective the contract balance is
//!    silently short of the expected total.
//!
//! 2. **Misreporting balance**: `balance()` returns `actual + LIE` — i.e.
//!    the token lies *upward* about what the contract holds. `transfer`
//!    still only moves what's really there; a transfer that would exceed
//!    the real balance panics inside the token, matching how a real SAC
//!    rejects an insufficient-balance transfer.
//!
//! These are deliberately not malicious *token admin* scenarios (e.g. the
//! admin pulling tokens out after `lock`) — those sit under PRD §4.5's
//! "custody model is the SAC; HarvestLock has no admin" assumption and
//! would be a different, separate suite. The two chosen here are the shapes
//! where the token itself, under normal use, makes the escrow's internal
//! accounting wrong.
//!
//! What each test asserts:
//!
//! - **Fee-on-transfer, `lock` -> `settle`**: `lock` escrows deposit-minus-fee;
//!   `fund_remainder` escrows remainder-minus-fee; `settle` records the
//!   contract's actual balance as the cooperative's credit. The cooperative
//!   loses one more fee on withdrawal.
//! - **Fee-on-transfer, `cancel`**: cancel records the actual deposit-minus-fee
//!   held; the buyer loses one more fee on withdrawal.
//! - **Misreporting, terminal action + withdrawal**: terminal actions record
//!   the token-reported entitlement without attempting a transfer. Withdrawal
//!   traps when the token rejects the overdraw, leaving the terminal state
//!   and payout credit intact.
//!
//! Both tokens are kept per-instance (each test creates a fresh contract),
//! so their simulated misbehavior cannot cross between tests. Neither
//! mock needs an administrator mint path: each test credits the buyer by
//! calling a `credit` helper on the mock directly before `lock`.

#![cfg(test)]

use harvestlock_escrow::{CommitmentTerms, EscrowContract, EscrowContractClient, Status};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{symbol_short, token, Address, Env, Vec};

const WINDOW: u64 = 7 * 24 * 60 * 60;
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
const CONTRACTED_QUANTITY: u32 = 1_000;
const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];
const TOTAL_AMOUNT: i128 = 1_000_000;
const ADVANCE1_BPS: u32 = 1_500;
const ADVANCE2_BPS: u32 = 2_000;
const DEPOSIT: i128 = TOTAL_AMOUNT * (ADVANCE1_BPS as i128 + ADVANCE2_BPS as i128) / 10_000; // 350_000
const REMAINDER: i128 = TOTAL_AMOUNT - DEPOSIT; // 650_000

// ---------- fee-on-transfer token ----------

/// Fixed per-call fee the token swallows. Chosen comfortably smaller than
/// `DEPOSIT`/`REMAINDER` so every transfer in the lifecycle still delivers
/// a positive amount — the escrow's "silently short" story is the
/// interesting one, not "the token ate the whole transfer."
const FEE_ON_TRANSFER: i128 = 1_000;

mod fee_on_transfer_token {
    //! `transfer(amount)` moves `amount` out of `from`, credits
    //! `amount - FEE` to `to`, and keeps `FEE` in its own balance. This
    //! deliberately differs from the SEP-41 contract: sender and receiver
    //! no longer net to zero.

    use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Map};

    use super::FEE_ON_TRANSFER;

    #[contract]
    pub struct FeeOnTransferToken;

    #[contractimpl]
    impl FeeOnTransferToken {
        /// Test-only mint: no SEP-41 `admin` plumbing, just credit the
        /// account directly. The real SAC has `StellarAssetClient::mint`
        /// — this mock keeps the same practical shape without the
        /// administrator interface, which this suite doesn't exercise.
        pub fn credit(env: Env, to: Address, amount: i128) {
            let mut balances = load_balances(&env);
            let current = balances.get(to.clone()).unwrap_or(0);
            balances.set(to, current + amount);
            save_balances(&env, &balances);
        }

        pub fn balance(env: Env, id: Address) -> i128 {
            load_balances(&env).get(id).unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            if amount < 0 {
                panic!("fee-on-transfer: negative amount");
            }
            let mut balances = load_balances(&env);
            let from_bal = balances.get(from.clone()).unwrap_or(0);
            if from_bal < amount {
                panic!("fee-on-transfer: insufficient balance");
            }
            let delivered = (amount - FEE_ON_TRANSFER).max(0);
            balances.set(from.clone(), from_bal - amount);
            let to_bal = balances.get(to.clone()).unwrap_or(0);
            balances.set(to, to_bal + delivered);
            save_balances(&env, &balances);
        }
    }

    fn load_balances(env: &Env) -> Map<Address, i128> {
        env.storage()
            .instance()
            .get(&symbol_short!("bal"))
            .unwrap_or_else(|| Map::new(env))
    }

    fn save_balances(env: &Env, balances: &Map<Address, i128>) {
        env.storage()
            .instance()
            .set(&symbol_short!("bal"), balances);
    }
}

// ---------- misreporting-balance token ----------

/// How much the token over-reports its holdings by, in the misreporting
/// mock's `balance()` lie. Tests choose per-case values via `set_lie`.
const MISREPORT_LIE: i128 = 1_000_000;

mod misreporting_token {
    //! `balance(id)` returns `real + lie(id)` — the token lies *upward*
    //! only, matching the direction that stresses the escrow (an
    //! under-reported balance would just mean the escrow didn't pay out
    //! what it could have, which is the "wrongly refused" direction the
    //! threat model treats as recoverable). `transfer` moves only what is
    //! actually there; an over-ambitious transfer traps inside the token.

    use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Map, Symbol};

    const LIES_KEY: Symbol = symbol_short!("lies");

    #[contract]
    pub struct MisreportingToken;

    #[contractimpl]
    impl MisreportingToken {
        pub fn credit(env: Env, to: Address, amount: i128) {
            let mut balances = load_balances(&env);
            let current = balances.get(to.clone()).unwrap_or(0);
            balances.set(to, current + amount);
            save_balances(&env, &balances);
        }

        /// Install a per-address lie. `balance(id)` then returns
        /// `real_balance(id) + lie`.
        pub fn set_lie(env: Env, id: Address, lie: i128) {
            let mut lies = load_lies(&env);
            lies.set(id, lie);
            env.storage().instance().set(&LIES_KEY, &lies);
        }

        pub fn balance(env: Env, id: Address) -> i128 {
            let real = load_balances(&env).get(id.clone()).unwrap_or(0);
            let lie = load_lies(&env).get(id).unwrap_or(0);
            real + lie
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            if amount < 0 {
                panic!("misreporting: negative amount");
            }
            let mut balances = load_balances(&env);
            let from_bal = balances.get(from.clone()).unwrap_or(0);
            if from_bal < amount {
                // The lie is only in `balance()`; `transfer` still can't
                // move stroops that aren't really there.
                panic!("misreporting: insufficient real balance");
            }
            balances.set(from.clone(), from_bal - amount);
            let to_bal = balances.get(to.clone()).unwrap_or(0);
            balances.set(to, to_bal + amount);
            save_balances(&env, &balances);
        }
    }

    fn load_balances(env: &Env) -> Map<Address, i128> {
        env.storage()
            .instance()
            .get(&symbol_short!("bal"))
            .unwrap_or_else(|| Map::new(env))
    }

    fn save_balances(env: &Env, balances: &Map<Address, i128>) {
        env.storage()
            .instance()
            .set(&symbol_short!("bal"), balances);
    }

    fn load_lies(env: &Env) -> Map<Address, i128> {
        env.storage()
            .instance()
            .get(&LIES_KEY)
            .unwrap_or_else(|| Map::new(env))
    }
}

// ---------- shared fixture ----------

/// A plain (non-standard) token plus an escrow commitment whose `token`
/// address points at that mock. The escrow's `c.token` is the mock, not
/// a real SAC — nothing in these tests reads a SAC.
struct Fixture {
    env: Env,
    contract: EscrowContractClient<'static>,
    buyer: Address,
    cooperative: Address,
    custom_token: Address,
}

fn fee_on_transfer_fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);

    let token_id = env.register(fee_on_transfer_token::FeeOnTransferToken, ());
    let token_client = fee_on_transfer_token::FeeOnTransferTokenClient::new(&env, &token_id);
    token_client.credit(&buyer, &TOTAL_AMOUNT);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_id,
        &CommitmentTerms {
            total_amount: TOTAL_AMOUNT,
            advance1_bps: ADVANCE1_BPS,
            advance2_bps: ADVANCE2_BPS,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &None,
    );

    Fixture {
        env,
        contract,
        buyer,
        cooperative,
        custom_token: token_id,
    }
}

fn misreporting_fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);

    let token_id = env.register(misreporting_token::MisreportingToken, ());
    let token_client = misreporting_token::MisreportingTokenClient::new(&env, &token_id);
    token_client.credit(&buyer, &TOTAL_AMOUNT);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_id,
        &CommitmentTerms {
            total_amount: TOTAL_AMOUNT,
            advance1_bps: ADVANCE1_BPS,
            advance2_bps: ADVANCE2_BPS,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&env, GRADE_PRICE_BPS),
        },
        &None,
    );

    Fixture {
        env,
        contract,
        buyer,
        cooperative,
        custom_token: token_id,
    }
}

/// Drive through to `Delivered` at full quantity and full grade. Order
/// is **fund the remainder before claiming the advances**: the usual
/// order (claim both advances, then fund the remainder) would overdraw
/// the fee-on-transfer escrow at `claim_advance_2` — the escrow holds
/// only `deposit - FEE` after `lock`, and the two claims together try
/// to move `deposit` out. Opening the tranche windows is still permitted
/// before `ready_for_delivery` (the state machine requires it), but
/// `claim_advance_*` only checks the tranche's own window, not the
/// overall status, so the actual outbound transfers can happen *after*
/// the remainder is funded. This is a deliberate test fact, not a
/// claim about typical real-world ordering: it is the sequence that
/// lets a full lifecycle reach `settle` under a fee-on-transfer token
/// so the `min(owed, balance)` behavior can actually be observed.
fn drive_to_delivered(f: &Fixture) {
    f.contract.lock();
    f.contract.release_advance_1();
    f.contract.mark_checkpoint();
    f.contract.release_advance_2();
    f.contract.ready_for_delivery();
    f.contract.fund_remainder();
    f.contract.claim_advance_1();
    f.contract.claim_advance_2();
    f.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
}

fn balance_of(env: &Env, token: &Address, id: &Address) -> i128 {
    token::Client::new(env, token).balance(id)
}

/// Reads the mock's `real` (not `balance()`-lied) holdings for an id, so
/// the assertions below can tell "escrow was not drained" from "escrow
/// reports it was not drained." The mocks keep their truthful ledger in
/// the same "bal" instance-storage slot; re-reading it from outside the
/// mock is a test-only convenience, not a production path.
fn real_balance(env: &Env, token: &Address, id: &Address) -> i128 {
    env.as_contract(token, || {
        let balances: soroban_sdk::Map<Address, i128> = env
            .storage()
            .instance()
            .get(&symbol_short!("bal"))
            .unwrap_or_else(|| soroban_sdk::Map::new(env));
        balances.get(id.clone()).unwrap_or(0)
    })
}

// ---------- fee-on-transfer: lock -> settle ----------

/// With a fee-on-transfer token, `lock` moves `DEPOSIT` out of the buyer
/// but only `DEPOSIT - FEE` lands in the escrow; the same applies to
/// `fund_remainder`. `settle` then pays `min(owed, balance)` and the
/// cooperative ends up short by exactly the fees swallowed on each
/// transfer leg — the deficit does not come out of the already-claimed
/// advances, it comes out of the remainder refund (which goes to zero).
#[test]
fn fee_on_transfer_lock_and_settle_leaves_the_escrow_short_by_the_fees_swallowed() {
    let f = fee_on_transfer_fixture();
    let buyer_before = balance_of(&f.env, &f.custom_token, &f.buyer);
    assert_eq!(buyer_before, TOTAL_AMOUNT);

    drive_to_delivered(&f);

    // Two inbound transfers (DEPOSIT, REMAINDER) each lose FEE on arrival,
    // and two advance-claim transfers already left the escrow along the
    // way, each debiting the full bps_amount.
    let advance1 = TOTAL_AMOUNT * ADVANCE1_BPS as i128 / 10_000;
    let advance2 = TOTAL_AMOUNT * ADVANCE2_BPS as i128 / 10_000;
    let escrow_balance_before_settle = balance_of(&f.env, &f.custom_token, &f.contract.address);
    let expected_escrow_before_settle =
        (DEPOSIT - FEE_ON_TRANSFER) + (REMAINDER - FEE_ON_TRANSFER) - advance1 - advance2;
    assert_eq!(escrow_balance_before_settle, expected_escrow_before_settle);

    f.contract.settle();

    // `settle` records the actual balance as the cooperative's credit; the
    // outbound token transfer occurs only on withdrawal.
    assert_eq!(
        f.contract.get_commitment().cooperative_payout,
        expected_escrow_before_settle
    );
    assert_eq!(
        balance_of(&f.env, &f.custom_token, &f.contract.address),
        expected_escrow_before_settle
    );
    f.contract.withdraw_payout(&f.cooperative);
    assert_eq!(balance_of(&f.env, &f.custom_token, &f.contract.address), 0);

    // Three transfer edges between buyer wallet and cooperative wallet:
    // lock (inbound), fund_remainder (inbound), settle (outbound to
    // coop). Add the two claim_advance_* outbound legs. Each edge
    // swallows one FEE, so the cooperative's net wallet balance is the
    // sum of the (bps_amount - FEE) legs plus (escrow_before_settle -
    // FEE).
    let coop_wallet = balance_of(&f.env, &f.custom_token, &f.cooperative);
    let expected_coop_wallet = (advance1 - FEE_ON_TRANSFER)
        + (advance2 - FEE_ON_TRANSFER)
        + (expected_escrow_before_settle - FEE_ON_TRANSFER);
    assert_eq!(coop_wallet, expected_coop_wallet);

    // The buyer paid the full TOTAL_AMOUNT and got nothing back — the
    // "silently short" conclusion, in the one direction that matters.
    assert_eq!(balance_of(&f.env, &f.custom_token, &f.buyer), 0);

    assert_eq!(f.contract.get_status(), Status::Settled);
}

/// `cancel` after `lock` returns whatever the escrow currently holds,
/// which with a fee-on-transfer token is `DEPOSIT - FEE`, not `DEPOSIT`.
/// The cancel's outbound transfer swallows one more fee on the way back,
/// so the buyer's net loss across the full cycle is two fees, not zero.
#[test]
fn fee_on_transfer_cancel_refunds_what_is_actually_held_not_what_was_sent() {
    let f = fee_on_transfer_fixture();

    f.contract.lock();
    let escrow_after_lock = balance_of(&f.env, &f.custom_token, &f.contract.address);
    assert_eq!(escrow_after_lock, DEPOSIT - FEE_ON_TRANSFER);

    f.contract.cancel();

    // Cancel records the amount held; the buyer receives it on withdrawal.
    assert_eq!(
        balance_of(&f.env, &f.custom_token, &f.contract.address),
        escrow_after_lock
    );
    f.contract.withdraw_payout(&f.buyer);
    assert_eq!(balance_of(&f.env, &f.custom_token, &f.contract.address), 0);
    // Buyer receives (balance - FEE) on the refund leg — the escrow was
    // already short by one FEE from the lock inbound, and the refund
    // outbound swallows another.
    let buyer_after = balance_of(&f.env, &f.custom_token, &f.buyer);
    assert_eq!(
        buyer_after,
        TOTAL_AMOUNT - DEPOSIT + (DEPOSIT - 2 * FEE_ON_TRANSFER)
    );
    assert_eq!(f.contract.get_status(), Status::Cancelled);
}

// ---------- misreporting balance ----------

/// `settle` records credits without touching the token. The inflated balance
/// creates an oversized buyer refund, whose later withdrawal traps; the
/// terminal state and credit remain intact.
#[test]
fn misreporting_balance_traps_settlement_withdrawal_without_erasing_credit() {
    let f = misreporting_fixture();
    drive_to_delivered(&f);

    // Lie inflates `balance()` above the real holdings, which is enough
    // that `settle`'s `min(owed, balance)` path and subsequent transfer
    // try to move more than the real balance.
    misreporting_token::MisreportingTokenClient::new(&f.env, &f.custom_token)
        .set_lie(&f.contract.address, &MISREPORT_LIE);

    let real_before = real_balance(&f.env, &f.custom_token, &f.contract.address);
    f.contract.settle();
    assert_eq!(f.contract.get_status(), Status::Settled);
    let payout = f.contract.get_commitment().buyer_payout;
    assert!(payout > real_before);

    let withdrawal = f.contract.try_withdraw_payout(&f.buyer);
    assert!(
        withdrawal.is_err(),
        "a withdrawal that overdraws the real balance must trap even if balance() lied"
    );
    assert_eq!(f.contract.get_commitment().buyer_payout, payout);
    assert_eq!(
        real_balance(&f.env, &f.custom_token, &f.contract.address),
        real_before
    );
    // The lie-inflated balance is still what `balance()` reports — its
    // state lives in the token's own storage, outside Soroban's rollback
    // of the escrow's invocation. This is the token's problem, not the
    // escrow's.
    assert_eq!(
        token::Client::new(&f.env, &f.custom_token).balance(&f.contract.address),
        real_before + MISREPORT_LIE
    );
}

/// `cancel` records the token-reported amount without transferring it.
/// Withdrawal traps when `balance()` has lied upward, leaving the
/// cancellation and its retryable credit intact.
#[test]
fn misreporting_balance_traps_cancel_withdrawal_without_erasing_credit() {
    let f = misreporting_fixture();
    f.contract.lock();
    let real_before = real_balance(&f.env, &f.custom_token, &f.contract.address);

    misreporting_token::MisreportingTokenClient::new(&f.env, &f.custom_token)
        .set_lie(&f.contract.address, &MISREPORT_LIE);

    f.contract.cancel();
    let payout = f.contract.get_commitment().buyer_payout;
    assert!(payout > real_before);
    let withdrawal = f.contract.try_withdraw_payout(&f.buyer);
    assert!(
        withdrawal.is_err(),
        "the refund withdrawal must trap inside the token rather than draining nonexistent stroops"
    );
    // Cancellation committed, while the failed transfer moved nothing.
    assert_eq!(f.contract.get_status(), Status::Cancelled);
    assert_eq!(f.contract.get_commitment().buyer_payout, payout);
    assert_eq!(
        real_balance(&f.env, &f.custom_token, &f.contract.address),
        real_before
    );
}
