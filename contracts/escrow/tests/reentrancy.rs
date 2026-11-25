//! Issue #217 — prove the checks-effects-interactions (CEI) ordering with a
//! re-entrant mock SEP-41 token.
//!
//! Every token transfer in `lib.rs` saves its state effect *before* it calls
//! `token::Client::transfer`. Terminal transitions now record credits without
//! transferring; `withdraw_payout` clears the caller's credit before its
//! transfer. `MockToken` below is a deliberately hostile SEP-41 token: its
//! `transfer` invokes a caller-configured escrow function before returning,
//! which is the strongest re-entry a token contract can attempt.
//!
//! # Which layer stops the re-entry
//!
//! soroban-env-host 29.0.1 bans re-entry into a contract that is already on
//! the call stack (`ContractReentryMode::Prohibited`, "Contract re-entry is
//! not allowed"). That check runs in the host *before* the escrow's own
//! status guard is evaluated, so in this suite **every** callback is stopped
//! by the host ban, and `MockToken` records the failure as
//! [`CallbackKind::HostAbort`].
//!
//! The CEI save-before-transfer is therefore a second, currently unreachable
//! layer on this host: it is what would reject the re-entry (`Locked` is
//! already saved, an already-claimed tranche refuses, ...) if a future host or
//! a different token standard ever allowed the callback through. The tests
//! assert both facts they can observe:
//!
//! * the callback was attempted and failed, and
//! * the outer call succeeded and left the documented effects intact
//!   (status/flags, escrow balance, and each party's balance).
//!
//! `reentry_is_stopped_by_the_host_ban_not_the_status_guard` pins the layer
//! itself (a host abort, not a contract error) so a host change that lets the
//! callback reach the guard shows up as a failing test rather than silently
//! weakening this suite.
//!
//! The callback targets the state-changing method whose effects must already
//! be committed before the withdrawal transfer (`settle` from inside a
//! settlement withdrawal, `cancel` from inside a cancel withdrawal, etc.).
//! The host rejects each attempt before that method can run.

#[path = "common/mod.rs"]
mod common;

use common::{commitment_terms, mock_oracle, Terms};
use harvestlock_escrow::{EscrowContract, EscrowContractClient, OracleConfig, Status};
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env,
    InvokeError, Map, Symbol, Val, Vec,
};

// ---------------------------------------------------------------------------
// The re-entrant mock token
// ---------------------------------------------------------------------------

/// A minimal SEP-41 token whose `transfer` can invoke a configured function on
/// a configured contract before returning. Test-only: `mint` is unauthenticated
/// because every test here controls the whole `Env`.
pub mod mock_token {
    use super::*;

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum MockTokenError {
        NegativeAmount = 1,
        InsufficientBalance = 2,
    }

    /// What the token should call after moving balances.
    #[contracttype]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Callback {
        pub target: Address,
        pub function: Symbol,
    }

    /// How the callback ended, classified from the SDK's `InvokeError`.
    #[contracttype]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CallbackKind {
        /// Never attempted (no callback configured).
        NotAttempted,
        /// The callback ran to completion. A test failure by construction.
        Succeeded,
        /// The host rejected the re-entrant invocation before the escrow ran
        /// any code: `InvokeError::Abort` (the same-contract re-entry ban).
        HostAbort,
        /// The escrow ran and rejected the call with its own contract error:
        /// the status/flags guard, not the host.
        ContractError,
    }

    #[contracttype]
    pub enum DataKey {
        Balances,
        Callback,
        Attempts,
        LastKind,
    }

    #[contract]
    pub struct MockToken;

    fn balances(env: &Env) -> Map<Address, i128> {
        env.storage()
            .instance()
            .get(&DataKey::Balances)
            .unwrap_or_else(|| Map::new(env))
    }

    fn write_balances(env: &Env, map: &Map<Address, i128>) {
        env.storage().instance().set(&DataKey::Balances, map);
    }

    #[contractimpl]
    impl MockToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let mut map = balances(&env);
            let next = map.get(to.clone()).unwrap_or(0) + amount;
            map.set(to, next);
            write_balances(&env, &map);
        }

        pub fn balance(env: Env, id: Address) -> i128 {
            balances(&env).get(id).unwrap_or(0)
        }

        pub fn decimals(_env: Env) -> u32 {
            7
        }

        pub fn transfer(
            env: Env,
            from: Address,
            to: Address,
            amount: i128,
        ) -> Result<(), MockTokenError> {
            from.require_auth();
            if amount < 0 {
                return Err(MockTokenError::NegativeAmount);
            }
            let mut map = balances(&env);
            let from_balance = map.get(from.clone()).unwrap_or(0);
            if from_balance < amount {
                return Err(MockTokenError::InsufficientBalance);
            }
            map.set(from.clone(), from_balance - amount);
            let to_balance = map.get(to.clone()).unwrap_or(0);
            map.set(to.clone(), to_balance + amount);
            write_balances(&env, &map);

            Self::attempt_reentry(&env);
            Ok(())
        }

        /// Configure (or replace) the function `transfer` calls back into.
        pub fn set_callback(env: Env, target: Address, function: Symbol) {
            env.storage()
                .instance()
                .set(&DataKey::Callback, &Callback { target, function });
            env.storage().instance().set(&DataKey::Attempts, &0u32);
            env.storage()
                .instance()
                .set(&DataKey::LastKind, &CallbackKind::NotAttempted);
        }

        pub fn attempts(env: Env) -> u32 {
            env.storage()
                .instance()
                .get(&DataKey::Attempts)
                .unwrap_or(0)
        }

        pub fn last_callback_kind(env: Env) -> CallbackKind {
            env.storage()
                .instance()
                .get(&DataKey::LastKind)
                .unwrap_or(CallbackKind::NotAttempted)
        }

        /// Try to re-enter `target.function()` with no arguments. The escrow
        /// methods exercised by this suite all take no arguments; keeping the
        /// mock argument-free also keeps the callback pointed at the *same*
        /// method that is currently paying out.
        fn attempt_reentry(env: &Env) {
            let Some(callback) = env
                .storage()
                .instance()
                .get::<_, Callback>(&DataKey::Callback)
            else {
                return;
            };
            let attempts = env
                .storage()
                .instance()
                .get::<_, u32>(&DataKey::Attempts)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::Attempts, &(attempts + 1));

            let args: Vec<Val> = Vec::new(env);
            let result = env.try_invoke_contract::<(), InvokeError>(
                &callback.target,
                &callback.function,
                args,
            );

            let kind = match result {
                Ok(Ok(())) => CallbackKind::Succeeded,
                // The callee ran and returned a contract error (or panicked
                // with one) — the status guard did its job.
                Ok(Err(_)) | Err(Ok(InvokeError::Contract(_))) => CallbackKind::ContractError,
                // The call never reached the callee: host-level abort. The
                // same-contract re-entry ban is the only reason this suite's
                // calls land here.
                Err(Ok(InvokeError::Abort)) | Err(Err(_)) => CallbackKind::HostAbort,
            };
            env.storage().instance().set(&DataKey::LastKind, &kind);
        }
    }
}

use mock_token::{CallbackKind, MockToken, MockTokenClient};

// ---------------------------------------------------------------------------
// Harness: a real escrow instance whose settlement token is the mock
// ---------------------------------------------------------------------------

struct Harness {
    env: Env,
    escrow: EscrowContractClient<'static>,
    escrow_id: Address,
    mock: MockTokenClient<'static>,
    token: token::Client<'static>,
    buyer: Address,
    cooperative: Address,
    total: i128,
}

fn harness_with(env: Env, terms: Terms, oracle: Option<OracleConfig>) -> Harness {
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);

    let token_id = env.register(MockToken, ());
    let mock = MockTokenClient::new(&env, &token_id);
    // Generous mint: the FX-shortfall tests top up well beyond `total_amount`.
    mock.mint(&buyer, &(terms.total_amount * 3));
    let token = token::Client::new(&env, &token_id);

    let escrow_id = env.register(EscrowContract, ());
    let escrow = EscrowContractClient::new(&env, &escrow_id);
    escrow.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_id,
        &commitment_terms(&env, &terms),
        &oracle,
    );

    Harness {
        env,
        escrow,
        escrow_id,
        mock,
        token,
        buyer,
        cooperative,
        total: terms.total_amount,
    }
}

fn quiet_env() -> Env {
    Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    })
}

fn harness() -> Harness {
    harness_with(quiet_env(), Terms::default(), None)
}

impl Harness {
    fn bps(&self, bps: u32) -> i128 {
        self.total * i128::from(bps) / 10_000
    }

    fn deposit(&self) -> i128 {
        self.bps(3_000)
    }

    /// Point the token's transfer callback at the escrow method `method`.
    fn arm(&self, method: &str) {
        self.mock
            .set_callback(&self.escrow_id, &Symbol::new(&self.env, method));
    }

    fn assert_callback_blocked(&self) {
        assert_eq!(
            self.mock.attempts(),
            1,
            "re-entry was not attempted exactly once"
        );
        assert_eq!(
            self.mock.last_callback_kind(),
            CallbackKind::HostAbort,
            "expected the host re-entry ban, not the escrow's status guard, to stop the callback"
        );
    }

    fn escrow_balance(&self) -> i128 {
        self.token.balance(&self.escrow_id)
    }

    fn drive_to_remainder_funded(&self) {
        self.escrow.lock();
        self.escrow.release_advance_1();
        self.escrow.claim_advance_1();
        self.escrow.mark_checkpoint();
        self.escrow.release_advance_2();
        self.escrow.claim_advance_2();
        self.escrow.ready_for_delivery();
        self.escrow.fund_remainder();
    }

    /// Lock -> both advances claimed -> remainder funded -> Delivered.
    fn drive_to_delivered(&self, delivered_quantity: u32) {
        self.drive_to_remainder_funded();
        self.escrow.confirm_delivery(&delivered_quantity, &0);
    }
}

// ---------------------------------------------------------------------------
// Tests — one per value-moving method
// ---------------------------------------------------------------------------

/// `lock` from inside `lock`'s deposit transfer. Before the transfer,
/// `Locked` is already saved, which is exactly what a second `lock` would
/// have to observe if the host let it through.
#[test]
fn lock_cannot_reenter_lock() {
    let h = harness();
    h.arm("lock");
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.lock();

    h.assert_callback_blocked();
    assert_eq!(h.escrow.get_status(), Status::Locked);
    assert_eq!(h.escrow_balance(), h.deposit());
    assert_eq!(h.token.balance(&h.buyer), buyer_before - h.deposit());
    assert_eq!(h.token.balance(&h.cooperative), 0);
}

#[test]
fn claim_advance_1_cannot_reenter_itself() {
    let h = harness();
    h.escrow.lock();
    h.escrow.release_advance_1();
    h.arm("claim_advance_1");

    h.escrow.claim_advance_1();

    h.assert_callback_blocked();
    assert!(h.escrow.get_commitment().advance1_claimed);
    assert_eq!(h.escrow_balance(), h.deposit() - h.bps(1_500));
    assert_eq!(h.token.balance(&h.cooperative), h.bps(1_500));
}

#[test]
fn claim_advance_2_cannot_reenter_itself() {
    let h = harness();
    h.escrow.lock();
    h.escrow.release_advance_1();
    h.escrow.claim_advance_1();
    h.escrow.mark_checkpoint();
    h.escrow.release_advance_2();
    h.arm("claim_advance_2");

    h.escrow.claim_advance_2();

    h.assert_callback_blocked();
    assert!(h.escrow.get_commitment().advance2_claimed);
    assert_eq!(h.token.balance(&h.cooperative), h.bps(3_000));
}

#[test]
fn reclaim_advance_1_cannot_reenter_itself() {
    let h = harness();
    h.escrow.lock();
    h.escrow.release_advance_1();
    let deadline = h.escrow.get_commitment().advance1_deadline;
    h.env.ledger().set_timestamp(deadline + 1);
    h.arm("reclaim_advance_1");
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.reclaim_advance_1();

    h.assert_callback_blocked();
    assert!(h.escrow.get_commitment().advance1_expired);
    assert_eq!(h.token.balance(&h.buyer), buyer_before + h.bps(1_500));
}

#[test]
fn reclaim_advance_2_cannot_reenter_itself() {
    let h = harness();
    h.escrow.lock();
    h.escrow.release_advance_1();
    h.escrow.claim_advance_1();
    h.escrow.mark_checkpoint();
    h.escrow.release_advance_2();
    let deadline = h.escrow.get_commitment().advance2_deadline;
    h.env.ledger().set_timestamp(deadline + 1);
    h.arm("reclaim_advance_2");
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.reclaim_advance_2();

    h.assert_callback_blocked();
    assert!(h.escrow.get_commitment().advance2_expired);
    assert_eq!(h.token.balance(&h.buyer), buyer_before + h.bps(1_500));
}

#[test]
fn fund_remainder_cannot_reenter_itself() {
    let h = harness();
    h.escrow.lock();
    h.escrow.release_advance_1();
    h.escrow.claim_advance_1();
    h.escrow.mark_checkpoint();
    h.escrow.release_advance_2();
    h.escrow.claim_advance_2();
    h.escrow.ready_for_delivery();
    h.arm("fund_remainder");
    let buyer_before = h.token.balance(&h.buyer);
    let remainder = h.total - h.deposit();

    h.escrow.fund_remainder();

    h.assert_callback_blocked();
    assert!(h.escrow.get_commitment().remainder_funded);
    assert_eq!(h.escrow_balance(), h.total - h.bps(3_000));
    assert_eq!(h.token.balance(&h.buyer), buyer_before - remainder);
}

#[test]
fn expire_remainder_window_withdrawal_cannot_reenter_expiry() {
    let h = harness();
    h.escrow.lock();
    h.escrow.release_advance_1();
    h.escrow.claim_advance_1();
    h.escrow.mark_checkpoint();
    h.escrow.release_advance_2();
    // Leave tranche 2 unclaimed so the default credits a non-zero payout.
    h.escrow.ready_for_delivery();
    let deadline = h.escrow.get_commitment().remainder_deadline;
    h.env.ledger().set_timestamp(deadline + 1);
    h.escrow.expire_remainder_window();
    h.arm("expire_remainder_window");
    h.escrow.withdraw_payout(&h.cooperative);

    h.assert_callback_blocked();
    assert_eq!(h.escrow.get_status(), Status::Defaulted);
    assert_eq!(h.escrow_balance(), 0);
    // 150_000 from the claimed first tranche plus the 150_000 that was still
    // escrowed as the unclaimed second tranche.
    assert_eq!(h.token.balance(&h.cooperative), h.deposit());
}

#[test]
fn reclaim_on_nondelivery_withdrawal_cannot_reenter_reclaim() {
    let h = harness();
    h.escrow.lock();
    let deadline = h.escrow.get_commitment().delivery_deadline;
    h.env.ledger().set_timestamp(deadline + 1);
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.reclaim_on_nondelivery();
    h.arm("reclaim_on_nondelivery");
    h.escrow.withdraw_payout(&h.buyer);

    h.assert_callback_blocked();
    assert_eq!(h.escrow.get_status(), Status::Forfeited);
    assert_eq!(h.escrow_balance(), 0);
    assert_eq!(h.token.balance(&h.buyer), buyer_before + h.deposit());
}

#[test]
fn expire_delivery_window_withdrawal_cannot_reenter_expiry() {
    let h = harness();
    h.escrow.lock();
    let c = h.escrow.get_commitment();
    h.env
        .ledger()
        .set_timestamp(c.delivery_deadline + c.remainder_window_secs + 1);
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.expire_delivery_window();
    h.arm("expire_delivery_window");
    h.escrow.withdraw_payout(&h.buyer);

    h.assert_callback_blocked();
    assert_eq!(h.escrow.get_status(), Status::Forfeited);
    assert_eq!(h.token.balance(&h.buyer), buyer_before + h.deposit());
}

/// A half-quantity delivery credits the cooperative and buyer separately;
/// each independent withdrawal triggers a callback attempt.
#[test]
fn settle_cannot_reenter_itself_from_either_payout() {
    let h = harness();
    h.drive_to_delivered(500);
    let coop_before = h.token.balance(&h.cooperative);
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.settle();
    h.arm("settle");
    h.escrow.withdraw_payout(&h.buyer);
    h.escrow.withdraw_payout(&h.cooperative);

    assert_eq!(h.mock.attempts(), 2);
    assert_eq!(h.mock.last_callback_kind(), CallbackKind::HostAbort);
    assert_eq!(h.escrow.get_status(), Status::Settled);
    assert_eq!(h.escrow_balance(), 0);
    // settlement_bps = 5_000: 500_000 total value, 300_000 already claimed,
    // so 200_000 leaves as the cooperative payment and 500_000 is refunded.
    assert_eq!(h.token.balance(&h.cooperative), coop_before + h.bps(2_000));
    assert_eq!(h.token.balance(&h.buyer), buyer_before + h.bps(5_000));
}

#[test]
fn cancel_withdrawal_cannot_reenter_cancel() {
    let h = harness();
    h.escrow.lock();
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.cancel();
    h.arm("cancel");
    h.escrow.withdraw_payout(&h.buyer);

    h.assert_callback_blocked();
    assert_eq!(h.escrow.get_status(), Status::Cancelled);
    assert_eq!(h.escrow_balance(), 0);
    assert_eq!(h.token.balance(&h.buyer), buyer_before + h.deposit());
}

// ---------------------------------------------------------------------------
// FX paths — same shape, reached through the configured-oracle branch
// ---------------------------------------------------------------------------

fn oracle_harness() -> Harness {
    // 2_000_000_000 NGN (at the 7-decimal convention) at 0.000667 USD/NGN
    // settles to 1_334_000 token units for a full, full-grade delivery.
    // 300_000 is already claimed as advances and 700_000 sits in escrow, so
    // resolution records a 334_000 shortfall.
    let env = quiet_env();
    let oracle_id = env.register(mock_oracle::MockOracle, ());
    mock_oracle::MockOracleClient::new(&env, &oracle_id).set_price(&66_700_000_000, &0);
    let oracle = OracleConfig {
        oracle_contract: oracle_id,
        price_asset: symbol_short!("NGN"),
        max_age_secs: 3_600,
        denominated_amount: 2_000_000_000,
    };
    harness_with(env, Terms::default(), Some(oracle))
}

#[test]
fn fund_fx_shortfall_cannot_reenter_itself() {
    let h = oracle_harness();
    h.drive_to_delivered(1_000);
    h.escrow.resolve_fx_shortfall();
    let shortfall = h.escrow.get_commitment().fx_shortfall_amount;
    assert!(shortfall > 0, "test setup must produce a real shortfall");
    h.arm("fund_fx_shortfall");
    let buyer_before = h.token.balance(&h.buyer);

    h.escrow.fund_fx_shortfall();

    h.assert_callback_blocked();
    assert!(h.escrow.get_commitment().fx_shortfall_funded);
    assert_eq!(h.token.balance(&h.buyer), buyer_before - shortfall);
}

#[test]
fn expire_fx_shortfall_window_withdrawal_cannot_reenter_expiry() {
    let h = oracle_harness();
    h.drive_to_delivered(1_000);
    h.escrow.resolve_fx_shortfall();
    let deadline = h.escrow.get_commitment().fx_shortfall_deadline;
    assert!(deadline > 0, "test setup must produce a real shortfall");
    h.env.ledger().set_timestamp(deadline + 1);
    h.escrow.expire_fx_shortfall_window();
    h.arm("expire_fx_shortfall_window");
    h.escrow.withdraw_payout(&h.cooperative);

    h.assert_callback_blocked();
    assert_eq!(h.escrow.get_status(), Status::Defaulted);
    assert_eq!(h.escrow_balance(), 0);
}

// ---------------------------------------------------------------------------
// Pin which defence applies (#217's "show which layer stopped it")
// ---------------------------------------------------------------------------

#[test]
fn reentry_is_stopped_by_the_host_ban_not_the_status_guard() {
    // `lock` from inside `lock` is the clearest case: the status guard would
    // see `Locked` and return `InvalidState`, but the host rejects the call
    // before the escrow frame is even pushed. This assertion is the record of
    // which defence is actually load-bearing on soroban-env-host 29.0.1.
    let h = harness();
    h.arm("lock");
    h.escrow.lock();

    assert_eq!(h.mock.attempts(), 1);
    assert_eq!(h.mock.last_callback_kind(), CallbackKind::HostAbort);
    assert_ne!(h.mock.last_callback_kind(), CallbackKind::ContractError);
}

/// The mock itself moves tokens correctly, so the balance assertions above are
/// meaningful rather than vacuous. This guards the harness, not the contract.
#[test]
fn mock_token_transfer_moves_balances_without_a_callback() {
    let h = harness();
    let buyer_before = h.token.balance(&h.buyer);
    h.escrow.lock();
    assert_eq!(h.token.balance(&h.buyer), buyer_before - h.deposit());
    assert_eq!(h.escrow_balance(), h.deposit());
    assert_eq!(h.mock.attempts(), 0);
}
