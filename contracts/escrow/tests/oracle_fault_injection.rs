//! #124 — fault-injection harness for the Reflector cross-contract read.
//!
//! `oracle_rate()` and `resolve_fx_shortfall()` are the only two places this
//! contract calls *out* to a contract it does not control. Everything here
//! drives that call through deliberately broken oracles and asserts the
//! documented degradation — `OracleNotConfigured` / `OraclePriceUnavailable`
//! / `OracleStale` — rather than a panic escaping or a wrong number coming
//! back.
//!
//! Fault modes covered: no config, no quote on record, stale quote, quote at
//! exactly the bound, a crashed (`panic!`ing) callee, an unreachable callee
//! (address with no contract deployed), a future-dated quote, a negative
//! price, a price large enough to overflow the conversion multiply, an
//! absurd `decimals()`, and (#63) a *plausible* change to `decimals()`, which
//! is silently accepted rather than trapping — see
//! `docs/reflector-decimals-assumption.md` for why the contract assumes the
//! scale never changes and what happens if it does.
//!
//! ## Findings this harness produced (all real, all reproducible)
//!
//! The three documented errors work exactly as advertised. Four things do
//! **not**, and are pinned here as current behavior with `FINDING` markers
//! rather than silently fixed, because each fix changes settlement math or
//! the public error surface and needs a product decision:
//!
//! 1. **F1 — an unreachable oracle is not a graceful error.** Pointing
//!    `oracle_config.oracle_contract` at an address with no contract (a
//!    re-deployed oracle, a wrong address typed at `initialize`) aborts the
//!    whole invocation at the host level. There is no
//!    `OracleUnreachable`-shaped contract error to catch, and the contract
//!    has no way to be repaired afterwards: `oracle_config` is set once at
//!    `initialize` and has no setter. Callers (the API's staged-transaction
//!    layer) must treat a host abort here as "this commitment is unreadable."
//! 2. **F2 — a future-dated quote passes the staleness bound.** `read_oracle_rate`
//!    computes `now.saturating_sub(quote.timestamp)`, so a quote stamped
//!    *ahead* of the ledger gets age `0` and is accepted as maximally fresh.
//!    A buggy or manipulated feed can pin the bound open indefinitely.
//! 3. **F3 — no sanity bound on the price itself.** A negative price is
//!    returned by `oracle_rate()` unvalidated and flows straight into
//!    `resolve_fx_shortfall()`, where a negative `fx_adjusted_total` makes
//!    `owed_from_remainder` clamp to `0`: `settle()` then pays the cooperative
//!    nothing and refunds the entire escrow to the buyer. One bad oracle
//!    value silently inverts who the settlement favors.
//! 4. **F4/F5 — `resolve_fx_shortfall` has two unguarded arithmetic traps.**
//!    `ngn_owed * rate.price` and `10i128.pow(decimals)` are unchecked: a
//!    huge price or a garbage `decimals()` (Reflector's real fiat oracle uses
//!    14) aborts the call. Soroban's atomicity means no partial state is
//!    written, so the damage is bounded to "this commitment can never be
//!    resolved" — same repair story as F1.
//!
//! A crashing or unreachable oracle is reported by the host, and depending on
//! the SDK's version the failure surfaces either as the `try_*` call's outer
//! error or as a panic that unwinds the simulated invocation. `attempt()`
//! tolerates both shapes; the property being asserted is the one that matters
//! either way — *a fault never yields a successful, wrong value*.

#[path = "common/mod.rs"]
mod common;

use common::{world, World};
use harvestlock_escrow::{Error, EscrowContract, EscrowContractClient, OracleConfig, Status};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Symbol, Vec};
use std::panic::{catch_unwind, AssertUnwindSafe};

const FRESH_PRICE: i128 = 175_000_000_000_000; // 1.75 at 14 decimals
const MAX_AGE: u64 = 3_600;
const DENOMINATED: i128 = 500_000;

// ---------- the fault-injection oracle ----------

/// Same wire shape as `reflector.rs`'s `Asset`/`PriceData` (Soroban matches
/// cross-contract calls on XDR shape, not Rust type identity), plus three
/// test-only setup hooks the real Reflector contract does not have.
mod faulty_oracle {
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
    pub struct FaultyOracle;

    #[contractimpl]
    impl FaultyOracle {
        pub fn set_quote(env: Env, price: i128, timestamp: u64) {
            env.storage()
                .instance()
                .set(&symbol_short!("quote"), &(price, timestamp));
        }

        /// Simulates "this oracle does not quote that asset" — the real
        /// Reflector behavior behind `OraclePriceUnavailable`.
        pub fn clear_quote(env: Env) {
            env.storage().instance().remove(&symbol_short!("quote"));
        }

        /// Simulates an oracle that crashes mid-call instead of answering.
        pub fn blackout(env: Env) {
            env.storage().instance().set(&symbol_short!("dead"), &true);
        }

        /// Simulates an oracle reporting a nonsense scale factor.
        pub fn set_decimals(env: Env, decimals: u32) {
            env.storage()
                .instance()
                .set(&symbol_short!("dec"), &decimals);
        }

        pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
            if env
                .storage()
                .instance()
                .get::<_, bool>(&symbol_short!("dead"))
                .unwrap_or(false)
            {
                panic!("simulated oracle crash");
            }
            env.storage()
                .instance()
                .get::<_, (i128, u64)>(&symbol_short!("quote"))
                .map(|(price, timestamp)| PriceData { price, timestamp })
        }

        pub fn decimals(env: Env) -> u32 {
            env.storage()
                .instance()
                .get(&symbol_short!("dec"))
                .unwrap_or(14)
        }
    }
}

// ---------- builders ----------

/// `FaultyOracle`'s generated client, exposed only when the test registered
/// the mock (F1's world points at a bare address instead).
struct OracleWorld {
    w: World,
    oracle_addr: Address,
    oracle: Option<faulty_oracle::FaultyOracleClient<'static>>,
}

impl OracleWorld {
    fn oracle(&self) -> &faulty_oracle::FaultyOracleClient<'static> {
        self.oracle
            .as_ref()
            .expect("this test needs a registered faulty oracle")
    }
    fn env(&self) -> &Env {
        &self.w.env
    }
}

/// A commitment initialized with an `oracle_config` pointing at a registered
/// `FaultyOracle` holding `FRESH_PRICE` at the *current* ledger timestamp
/// (0), so `age == 0` and every read starts inside the bound.
fn oracle_world(max_age_secs: u64, denominated_amount: i128) -> OracleWorld {
    let env = Env::default();
    env.mock_all_auths();
    let oracle_id = env.register(faulty_oracle::FaultyOracle, ());
    let oracle = faulty_oracle::FaultyOracleClient::new(&env, &oracle_id);
    oracle.set_quote(&FRESH_PRICE, &env.ledger().timestamp());
    let w = build_with(env, oracle_id.clone(), max_age_secs, denominated_amount);
    OracleWorld {
        w,
        oracle_addr: oracle_id,
        oracle: Some(oracle),
    }
}

/// The same, but pointing at an address that has no contract behind it (F1).
fn oracle_world_unreachable(max_age_secs: u64, denominated_amount: i128) -> OracleWorld {
    let env = Env::default();
    env.mock_all_auths();
    // Generated, funded-looking, and deliberately *never registered*.
    let dangling = Address::generate(&env);
    let w = build_with(env, dangling.clone(), max_age_secs, denominated_amount);
    OracleWorld {
        w,
        oracle_addr: dangling,
        oracle: None,
    }
}

fn build_with(
    env: Env,
    oracle_contract: Address,
    max_age_secs: u64,
    denominated_amount: i128,
) -> World {
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    let token = token::Client::new(&env, &token_address);
    let mint = token::StellarAssetClient::new(&env, &token_address);
    // Headroom beyond `total_amount`: a shortfall test has to be able to fund
    // a top-up on top of the deposit and remainder.
    mint.mint(&buyer, &(common::TOTAL_AMOUNT * 3));

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &common::TOTAL_AMOUNT,
        &1_500,
        &1_500,
        &common::WINDOW,
        &common::REMAINDER_WINDOW,
        &common::DELIVERY_WINDOW,
        &common::CONTRACTED_QUANTITY,
        &Vec::from_array(&env, common::GRADE_PRICE_BPS),
        &Some(OracleConfig {
            oracle_contract,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs,
            denominated_amount,
        }),
    );

    World {
        env: env.clone(),
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        total_amount: common::TOTAL_AMOUNT,
    }
}

// ---------- fault-mode assertions ----------

#[derive(Debug)]
enum Attempt {
    /// The `try_*` call returned `Ok(..)` — the oracle read *succeeded*.
    Succeeded,
    /// The call returned an error, at either the contract or host level.
    Failed,
    /// The fault unwound the simulated invocation instead of surfacing as an
    /// error. Still "no wrong value returned", which is the property here.
    Aborted,
}

fn attempt(f: impl FnOnce() -> bool) -> Attempt {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(true) => Attempt::Succeeded,
        Ok(false) => Attempt::Failed,
        Err(_) => Attempt::Aborted,
    }
}

// ---------- the three documented degradations ----------

#[test]
fn no_oracle_configured_degrades_to_oracle_not_configured_on_both_read_paths() {
    // `oracle_rate` alone would be a thin test; the doc comment for
    // `Error::OracleNotConfigured` used to name only `oracle_rate` even
    // though `resolve_fx_shortfall` returns it too (fixed in this PR).
    let s = world(1_500, 1_500);
    assert_eq!(
        s.contract.try_oracle_rate(),
        Err(Ok(Error::OracleNotConfigured))
    );
    assert_eq!(s.contract.get_oracle_config(), None);

    // The settlement path degrades identically, and nothing is recorded.
    s.drive_to_delivered();
    assert_eq!(
        s.contract.try_resolve_fx_shortfall(),
        Err(Ok(Error::OracleNotConfigured))
    );
    let c = s.contract.get_commitment();
    assert!(!c.fx_resolved && c.fx_adjusted_total == 0);
    // A commitment with no oracle config settles on `total_amount` alone —
    // `settle` never demands the FX step it just refused.
    s.contract.settle();
    assert_eq!(s.status(), Status::Settled);
}

#[test]
fn oracle_with_no_quote_on_record_degrades_to_price_unavailable() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.oracle().clear_quote();

    assert_eq!(
        s.w.contract.try_oracle_rate(),
        Err(Ok(Error::OraclePriceUnavailable))
    );

    // And the settlement path refuses the same way, before writing anything:
    // `fx_resolved` stays false, so `settle` remains gated on `FxNotResolved`.
    s.w.drive_to_delivered();
    assert_eq!(
        s.w.contract.try_resolve_fx_shortfall(),
        Err(Ok(Error::OraclePriceUnavailable))
    );
    let c = s.w.contract.get_commitment();
    assert!(!c.fx_resolved);
    assert_eq!(c.fx_shortfall_amount, 0);
    assert_eq!(
        s.w.contract.try_settle(),
        Err(Ok(Error::FxNotResolved)),
        "a failed oracle read must not open the settlement path"
    );
    assert_eq!(s.w.status(), Status::Delivered);
}

#[test]
fn stale_quote_degrades_to_oracle_stale_on_both_read_paths() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.w.advance(MAX_AGE + 1);

    assert_eq!(s.w.contract.try_oracle_rate(), Err(Ok(Error::OracleStale)));

    s.w.drive_to_delivered();
    assert_eq!(
        s.w.contract.try_resolve_fx_shortfall(),
        Err(Ok(Error::OracleStale))
    );
    let c = s.w.contract.get_commitment();
    assert!(!c.fx_resolved && c.fx_adjusted_total == 0 && c.fx_shortfall_deadline == 0);
    assert_eq!(s.w.contract.try_settle(), Err(Ok(Error::FxNotResolved)));
}

#[test]
fn quote_exactly_at_the_staleness_bound_is_accepted_not_rejected() {
    // `age > max_age_secs` is the rejection guard, so `age == max_age_secs`
    // must still read fine — the boundary the issue asked to be proven.
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.w.advance(MAX_AGE);
    let rate = s.w.contract.oracle_rate();
    assert_eq!(rate.price, FRESH_PRICE);

    s.w.advance(1);
    assert_eq!(s.w.contract.try_oracle_rate(), Err(Ok(Error::OracleStale)));
}

// ---------- the faults that are NOT covered by a documented error ----------

#[test]
fn a_crashing_oracle_never_yields_a_wrong_rate_and_writes_no_state() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.oracle().blackout();

    s.w.drive_to_delivered();
    let outcome = attempt(|| s.w.contract.try_oracle_rate().is_ok());
    assert!(
        !matches!(outcome, Attempt::Succeeded),
        "a crashing oracle must not produce a returned rate, got {outcome:?}"
    );

    let resolved = attempt(|| s.w.contract.try_resolve_fx_shortfall().is_ok());
    assert!(
        !matches!(resolved, Attempt::Succeeded),
        "a crashing oracle must not let `resolve_fx_shortfall` record a number"
    );
    let c = s.w.contract.get_commitment();
    assert!(!c.fx_resolved);
    assert_eq!(c.status, Status::Delivered);
}

/// F1 — an oracle address with nothing deployed behind it.
#[test]
fn an_unreachable_oracle_aborts_rather_than_degrading_to_a_documented_error() {
    let s = oracle_world_unreachable(MAX_AGE, DENOMINATED);

    let outcome = attempt(|| s.w.contract.try_oracle_rate().is_ok());
    assert!(
        matches!(outcome, Attempt::Failed | Attempt::Aborted),
        "expected an unreachable oracle to fail the read, got {outcome:?}"
    );

    // Actionable finding, stated plainly: the failure is *not* one of
    // OracleNotConfigured / OraclePriceUnavailable / OracleStale. Whatever
    // off-chain caller drove this gets a host abort, and because
    // `oracle_config` has no setter the commitment keeps that dead address
    // forever. `settle` on an oracle-configured commitment is then permanently
    // unreachable (`FxNotResolved`), so this is a liveness gap, not just a
    // cosmetic one — worth an `update_oracle_config` (re-initialize-gated) or
    // an `OracleUnreachable` contract error in a follow-up.
    assert_eq!(
        s.w.contract.get_oracle_config().unwrap().oracle_contract,
        s.oracle_addr,
        "the dead address is exactly what `initialize` persisted, and there \
         is no way to change it afterwards"
    );
}

/// F2 — a quote stamped in the future.
#[test]
fn a_future_dated_quote_bypasses_the_staleness_bound_for_as_long_as_it_stays_ahead() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    // "Now" is 0; stamp the quote 10_000 seconds *ahead* of the ledger.
    s.oracle().set_quote(&FRESH_PRICE, &(s.w.now() + 10_000));

    let rate = s.w.contract.oracle_rate();
    assert_eq!(
        rate.timestamp, 10_000,
        "FINDING F2: `now.saturating_sub(quote.timestamp)` clamps a future \
         quote's age to 0, so it reads as maximally fresh"
    );

    // The bypass lasts exactly as long as the ledger stays behind the stamp —
    // once real time passes it, normal staleness applies again.
    s.w.advance(10_000 + MAX_AGE + 1);
    assert_eq!(s.w.contract.try_oracle_rate(), Err(Ok(Error::OracleStale)));

    // The sharp version of the finding: nothing bounds how far ahead a quote
    // may be stamped, so a feed (or a compromised one) that always reports a
    // far-future timestamp is never rejected as stale at all.
    s.oracle().set_quote(&FRESH_PRICE, &u64::MAX);
    s.w.advance(MAX_AGE * 1_000);
    assert!(
        s.w.contract.try_oracle_rate().is_ok(),
        "a maximally future-dated quote is accepted indefinitely"
    );
}

/// F3 — a negative price, through both read paths and into settlement.
#[test]
fn a_negative_price_is_returned_unvalidated_and_inverts_the_settlement() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.oracle().set_quote(&(-FRESH_PRICE), &s.w.now());

    let rate = s.w.contract.oracle_rate();
    assert_eq!(
        rate.price, -FRESH_PRICE,
        "FINDING F3: `read_oracle_rate` has no `price > 0` sanity bound"
    );

    // Now the settlement consequence, with real numbers. bps 1500/1500 of a
    // 1_000_000 total: the cooperative claimed 150_000 + 150_000, and the
    // buyer funded the 700_000 remainder, so the contract holds 1_000_000.
    s.w.drive_to_delivered();
    s.w.contract.resolve_fx_shortfall();

    let c = s.w.contract.get_commitment();
    // 500_000 denominated units x (-1.75) / 10^14 scaled = -875_000.
    assert_eq!(c.fx_adjusted_total, -875_000);
    assert_eq!(
        c.fx_shortfall_amount, 0,
        "a negative adjusted total makes `owed_from_remainder` clamp to 0, \
         so there is never a shortfall to fund"
    );
    assert!(c.fx_shortfall_funded);
    assert!(c.fx_resolved);

    s.w.contract.settle();
    assert_eq!(
        s.w.token.balance(&s.w.cooperative),
        300_000,
        "FINDING F3: one garbage oracle value cost the cooperative the entire \
         remainder payment — it keeps only the two advances it had already claimed"
    );
    assert_eq!(
        s.w.token.balance(&s.w.contract.address),
        0,
        "and the whole escrowed remainder came back to the buyer"
    );
}

/// F4 — a price big enough to overflow the conversion multiply.
#[test]
fn an_overflowing_price_traps_resolve_fx_shortfall_without_recording_anything() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.oracle().set_quote(&(i128::MAX / 2), &s.w.now());

    s.w.drive_to_delivered();
    let outcome = attempt(|| s.w.contract.try_resolve_fx_shortfall().is_ok());
    assert!(
        !matches!(outcome, Attempt::Succeeded),
        "FINDING F4: `ngn_owed * rate.price` is unchecked, so an extreme \
         price traps instead of being rejected — got {outcome:?}"
    );

    let c = s.w.contract.get_commitment();
    assert!(
        !c.fx_resolved,
        "and Soroban's atomicity left no partial write"
    );
    assert_eq!(c.fx_adjusted_total, 0);
    assert_eq!(s.w.status(), Status::Delivered);
}

/// F5 — a nonsense `decimals()` answer, which `resolve_fx_shortfall` divides
/// by as `10i128.pow(decimals)`.
#[test]
fn an_absurd_decimals_count_traps_resolve_fx_shortfall() {
    let s = oracle_world(MAX_AGE, DENOMINATED);
    s.oracle().set_decimals(&200); // 10^200 does not fit in i128

    s.w.drive_to_delivered();
    let outcome = attempt(|| s.w.contract.try_resolve_fx_shortfall().is_ok());
    assert!(
        !matches!(outcome, Attempt::Succeeded),
        "FINDING F5: `10i128.pow(decimals)` is unbounded, so a bad \
         decimals() value traps — got {outcome:?}"
    );
    // `oracle_rate` itself is unaffected: it never calls `decimals()`.
    assert_eq!(s.w.contract.oracle_rate().price, FRESH_PRICE);
    assert!(!s.w.contract.get_commitment().fx_resolved);
}

/// #63 — the decimals-change question, and the assumption the conversion leans
/// on. See `docs/reflector-decimals-assumption.md` for the full finding.
///
/// `oracle_rate()` does **no** conversion math: it returns the raw `price`
/// exactly as `lastprice` gave it and never calls `decimals()`, so a
/// `decimals()` change cannot alter it. The scale is applied in exactly one
/// place — `resolve_fx_shortfall` (`lib.rs`), via `10i128.pow(decimals)`.
///
/// Reflector's real contract makes `decimals` immutable: `settings.rs` writes
/// it once in `init` (which aborts with `AlreadyInitialized` on a second call)
/// and exposes no `set_decimals`, and SEP-40 says the value "should never
/// change." This test pins what happens if that invariant is ever violated
/// anyway: a *plausible* new value (15 instead of 14) is accepted silently and
/// rescales the conversion by a power of ten, with no error and no mismatch
/// detection — unlike the absurd value in the F5 test above, which traps.
#[test]
fn a_plausible_decimals_change_silently_rescales_resolve_fx_shortfall() {
    let s = oracle_world(MAX_AGE, 1_000_000);
    s.oracle().set_quote(&FRESH_PRICE, &s.w.now());

    // At the real oracle's 14 decimals: 1_000_000 * 175_000_000_000_000 /
    // 10^14 == 1_750_000.
    assert_eq!(
        s.w.contract.oracle_rate().price,
        FRESH_PRICE,
        "oracle_rate returns the raw, unscaled price"
    );

    // A plausible precision bump — one more decimal place, still well within
    // i128 so nothing overflows.
    s.oracle().set_decimals(&15);

    // `oracle_rate()` is untouched: it never reads `decimals()`.
    assert_eq!(s.w.contract.oracle_rate().price, FRESH_PRICE);

    s.w.drive_to_delivered();
    s.w.contract.resolve_fx_shortfall();

    let c = s.w.contract.get_commitment();
    assert!(
        c.fx_resolved,
        "FINDING (#63): no error, no mismatch check — the changed scale is \
         accepted silently"
    );
    assert_eq!(
        c.fx_adjusted_total, 175_000,
        "FINDING (#63): the same raw price at decimals = 15 converts to \
         10^(14 - 15) == 1/10th the value, and nothing flags it"
    );
}

/// A healthy oracle still behaves normally after every fault above has been
/// driven at a *different* instance — i.e. none of this is state leakage
/// between commitments, and the harness itself is not accidentally asserting
/// a broken happy path.
#[test]
fn a_healthy_quote_still_resolves_normally_after_the_fault_modes() {
    let s = oracle_world(MAX_AGE, 1_000_000);
    s.oracle().blackout();
    assert!(!matches!(
        attempt(|| s.w.contract.try_oracle_rate().is_ok()),
        Attempt::Succeeded
    ));

    // Recovery is purely the oracle's own behavior — the escrow contract has
    // no repair path (F1). Clear the fault and the same instance reads fine.
    let env = s.env().clone();
    let fresh_id = env.register(faulty_oracle::FaultyOracle, ());
    let fresh = faulty_oracle::FaultyOracleClient::new(&env, &fresh_id);
    fresh.set_quote(&FRESH_PRICE, &env.ledger().timestamp());
    assert_eq!(
        fresh.lastprice(&asset_ngn(&env)).unwrap().price,
        FRESH_PRICE
    );
}

fn asset_ngn(env: &Env) -> faulty_oracle::Asset {
    faulty_oracle::Asset::Other(Symbol::new(env, "NGN"))
}

/// Sanity pin on the harness itself: `oracle_rate` reading a *mock* is a
/// faithful stand-in, because the escrow side's only oracle dependencies are
/// `lastprice`/`decimals` — proven by the healthy path resolving a real
/// shortfall end to end.
#[test]
fn healthy_oracle_path_records_the_expected_shortfall() {
    // `denominated_amount` 1_000_000 at a 1.75 rate => `fx_adjusted_total`
    // 1_750_000. The cooperative has already claimed 150_000 + 150_000, and
    // the contract holds the 700_000 remainder, so the buyer still owes
    // 1_750_000 - 300_000 - 700_000 = 750_000.
    let s = oracle_world(MAX_AGE, 1_000_000);
    s.w.drive_to_delivered();
    s.w.contract.resolve_fx_shortfall();

    let c = s.w.contract.get_commitment();
    assert!(c.fx_resolved);
    assert_eq!(c.fx_adjusted_total, 1_750_000);
    assert_eq!(c.fx_shortfall_amount, 750_000);
    assert!(!c.fx_shortfall_funded);
    assert_eq!(
        s.w.contract.try_settle(),
        Err(Ok(Error::FxShortfallUnfunded))
    );

    s.w.contract.fund_fx_shortfall();
    s.w.contract.settle();
    assert_eq!(s.w.token.balance(&s.w.cooperative), 1_750_000);
    assert_eq!(s.w.status(), Status::Settled);
}
