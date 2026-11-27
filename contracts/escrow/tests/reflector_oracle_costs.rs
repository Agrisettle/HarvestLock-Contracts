//! The FX path's cost against the **real** Reflector oracle, not a mock.
//!
//! Every other oracle test talks to a hand-written mock whose `lastprice` is a
//! single instance-storage read. The deployed Reflector does more per read:
//! it loads its whole instance (asset list, settings, a history bitmask and a
//! cache of recent rounds), resolves the asset's index, finds the latest round,
//! and decodes that asset's price out of the round's masked price vector. It
//! also runs as WASM in the VM rather than as native Rust.
//! `resolve_fx_shortfall` makes two such cross-contract calls (`lastprice`,
//! then `decimals`).
//!
//! This suite registers the exact testnet binary, pinned by hash under
//! `tests/wasm/` (see the README there for provenance). It configures the
//! binary through Reflector's own admin entry points (`config`, `set_price`),
//! publishes price rounds, and meters `oracle_rate` and
//! `resolve_fx_shortfall` with `env.cost_estimate()`. It runs the same two
//! calls against `testutils::mock_oracle::MockOracle` and prints both side by
//! side. The figures are recorded in `docs/gas-costs.md` ("Oracle group").
//!
//! ## The oracle configuration mirrors testnet
//!
//! Every `config` value below was read off the live fiat oracle's instance
//! storage on 2026-11-25 (testnet ledger ~5,914,834). They matter because
//! Reflector loads its whole instance on every call:
//!
//! * base `Other(USD)`, 14 decimals;
//! * 9 assets: EUR, GBP, CHF, CAD, MXN, ARS, BRL, THB, XAU;
//! * resolution 300,000 ms (one round per 5 minutes);
//! * history retention 86,400,000 ms (1 day);
//! * cache size 2 (the live `cache` entry holds exactly two rounds);
//! * a fee config (`Some((token, 2_000_000_000))`).
//!
//! On that date only EUR was still being published (each live round's mask
//! has one bit set); the other eight feeds' sponsorship had expired. The
//! contract's documented real target, NGN, is not quoted by this oracle at all
//! (see `reflector.rs`), so the escrow is pointed at a quoted asset here.
//!
//! ## Two scenarios, and why there is no "cache miss" one
//!
//! * **live**: single-asset EUR rounds, escrow quotes EUR (index 0). This is
//!   what testnet is doing today.
//! * **full round**: every round quotes all nine assets and the escrow quotes
//!   XAU (index 8). The cached rounds are as large as they get and the price
//!   decode walks the whole mask, so this is the most expensive successful
//!   read.
//!
//! This binary's `lastprice` only consults the **latest** round (see
//! `lastprice_sees_only_the_latest_round`), and with a cache size of 2 the
//! latest round is always in the instance cache. A temporary-storage read is
//! therefore unreachable through `lastprice`, so there is no cache-miss
//! scenario to measure.
//!
//! ## What the numbers mean
//!
//! These are host-model figures, the same kind `tests/budget.rs` records. The
//! escrow runs as **native Rust** here (it is registered from the crate, as in
//! every other suite), while Reflector runs as **WASM** in the VM. So
//! "Reflector − mock" is the oracle's real logic **plus** the cost of
//! instantiating and running a 20 KB module, which a native mock pays nothing
//! for. The standalone `lastprice`/`decimals` rows show the oracle's share on
//! its own. In production the escrow would be WASM too, so its own share would
//! also grow; see the doc section for that caveat.

use harvestlock_escrow::testutils::{commitment_terms, mock_oracle, Terms, CONTRACTED_QUANTITY};
use harvestlock_escrow::{EscrowContract, EscrowContractClient, Error, OracleConfig};
use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger as _};
use soroban_sdk::{token, Address, Bytes, Env, Symbol, Vec as OnChainVec};

/// The deployed testnet binary. `sha256` makes this a *build* failure if the
/// file is ever replaced; `tests/wasm/README.md` has the re-fetch procedure.
mod reflector {
    soroban_sdk::contractimport!(
        file = "tests/wasm/reflector_fiat_testnet.wasm",
        sha256 = "25fc306065cbb9b05461db78ec259802cb916053ba5b3ab911b0a72dd1a81439",
    );
}

// ---------- the live testnet configuration ----------

const QUOTED: [&str; 9] = ["EUR", "GBP", "CHF", "CAD", "MXN", "ARS", "BRL", "THB", "XAU"];
const DECIMALS: u32 = 14;
const RESOLUTION_MS: u32 = 300_000;
const RESOLUTION_SECS: u64 = (RESOLUTION_MS / 1_000) as u64;
const RETENTION_MS: u64 = 86_400_000;
const CACHE_SIZE: u32 = 2;
const FEE_PER_DAY: i128 = 2_000_000_000;

/// EUR/USD as quoted by the live oracle in its last cached round (1.0946…).
/// Used for every asset and for the mock, so the escrow's arithmetic is
/// identical in every scenario and only the oracle differs.
const PRICE: i128 = 109_462_760_876_884;
/// The live oracle's last round, in seconds; a multiple of the resolution.
const NOW: u64 = 1_795_597_800;
/// Rounds published before measuring: one hour at the 5-minute resolution.
/// More than fills the cache; the history bitmask is the same size (9 assets
/// × 32 bytes) from the first round on, so more rounds change nothing.
const ROUNDS: u64 = 12;

// ---------- escrow terms ----------

const MAX_AGE_SECS: u64 = 3_600;
/// Twice the escrowed total at a ~1.09 rate: a positive shortfall, so
/// `resolve_fx_shortfall` is measured on its full success path (it writes a
/// deadline), as `tests/budget.rs` does.
const DENOMINATED: i128 = 2_000_000;

// ---------- per-transaction network limits (see tests/resource_limits.rs) ----------

/// Read from testnet's `ConfigSetting` entries on 2026-11-24 (ledger
/// 5,901,514), the same values `tests/resource_limits.rs` asserts against.
const TX_MAX_INSTRUCTIONS: u64 = 400_000_000;
const TX_MEMORY_LIMIT_BYTES: u64 = 41_943_040;
const TX_MAX_READ_ENTRIES: u64 = 200;
const TX_MAX_WRITE_ENTRIES: u64 = 200;
const TX_MAX_WRITE_BYTES: u64 = 132_096;
/// The same headroom bar `resource_limits.rs` uses.
const LIMIT_FRACTION_PERCENT: u64 = 50;

// ---------- measurement ----------

#[derive(Clone, Copy, Debug)]
struct Sample {
    cpu: u64,
    mem: u64,
    read_entries: u64,
    write_entries: u64,
    write_bytes: u64,
}

/// Resets the meter, runs exactly one top-level invocation, reads the meter.
/// Same discipline as `budget.rs`: setup never leaks into the figure.
fn measure<R>(env: &Env, call: impl FnOnce() -> R) -> (R, Sample) {
    env.cost_estimate().budget().reset_default();
    let out = call();
    let budget = env.cost_estimate().budget();
    let used = env.cost_estimate().resources();
    (
        out,
        Sample {
            cpu: budget.cpu_instruction_cost(),
            mem: budget.memory_bytes_cost(),
            read_entries: u64::from(used.memory_read_entries),
            write_entries: u64::from(used.write_entries),
            write_bytes: u64::from(used.write_bytes),
        },
    )
}

fn fresh_env() -> Env {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.mock_all_auths();
    env
}

// ---------- Reflector setup, through its own entry points ----------

/// Registers the pinned Reflector binary and configures it exactly as the
/// live testnet instance is, through its own admin `config` entry point.
fn deploy_reflector(env: &Env) -> (Address, reflector::Client<'static>) {
    let id = env.register(reflector::WASM, ());
    let client = reflector::Client::new(env, &id);

    let fee_token = env
        .register_stellar_asset_contract_v2(Address::generate(env))
        .address();
    let mut assets = OnChainVec::new(env);
    for code in QUOTED {
        assets.push_back(asset(env, code));
    }
    client.config(&reflector::ConfigData {
        admin: Address::generate(env),
        assets,
        base_asset: asset(env, "USD"),
        cache_size: CACHE_SIZE,
        decimals: DECIMALS,
        fee_config: reflector::FeeConfig::Some((fee_token, FEE_PER_DAY)),
        history_retention_period: RETENTION_MS,
        resolution: RESOLUTION_MS,
    });
    (id, client)
}

fn asset(env: &Env, code: &str) -> reflector::Asset {
    reflector::Asset::Other(Symbol::new(env, code))
}

/// One admin `set_price` round at `ts_secs` quoting the assets at
/// `asset_indexes`. Reflector's `PriceUpdate.mask` has one bit per asset index
/// (byte `i / 8`, bit `i % 8`), and `prices` holds one entry per set bit, in
/// index order. Timestamps go in as milliseconds and must be a multiple of the
/// resolution, strictly after the previous round, and not in the future.
fn publish_round(env: &Env, oracle: &reflector::Client, asset_indexes: &[u32], ts_secs: u64) {
    env.ledger().set_timestamp(ts_secs);
    let mut mask = [0u8; 32];
    let mut prices = OnChainVec::new(env);
    for &i in asset_indexes {
        mask[(i / 8) as usize] |= 1 << (i % 8);
        prices.push_back(PRICE);
    }
    oracle.set_price(
        &reflector::PriceUpdate {
            mask: Bytes::from_array(env, &mask),
            prices,
        },
        &(ts_secs * 1_000),
    );
}

/// `ROUNDS` identical rounds ending at `NOW`; leaves the ledger clock at `NOW`.
fn publish_history(env: &Env, oracle: &reflector::Client, asset_indexes: &[u32]) {
    for i in 0..ROUNDS {
        publish_round(env, oracle, asset_indexes, NOW - (ROUNDS - 1 - i) * RESOLUTION_SECS);
    }
    env.ledger().set_timestamp(NOW);
}

const ALL_ASSETS: [u32; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];

/// An oracle for the escrow to point at, plus the asset it should quote.
struct OracleUnderTest {
    id: Address,
    price_asset: &'static str,
}

/// The mock every other suite uses: one instance-storage read per `lastprice`.
fn mock(env: &Env) -> OracleUnderTest {
    env.ledger().set_timestamp(NOW);
    let id = env.register(mock_oracle::MockOracle, ());
    mock_oracle::MockOracleClient::new(env, &id).set_price(&PRICE, &NOW);
    OracleUnderTest { id, price_asset: "EUR" }
}

/// What testnet is doing today: single-asset EUR rounds.
fn reflector_live(env: &Env) -> OracleUnderTest {
    let (id, client) = deploy_reflector(env);
    publish_history(env, &client, &[0]);
    OracleUnderTest { id, price_asset: "EUR" }
}

/// The most expensive successful read: all nine assets in every round, and
/// the escrow quotes the last one.
fn reflector_full_round(env: &Env) -> OracleUnderTest {
    let (id, client) = deploy_reflector(env);
    publish_history(env, &client, &ALL_ASSETS);
    OracleUnderTest { id, price_asset: "XAU" }
}

// ---------- the escrow ----------

/// How the escrow itself is registered: from the crate as native Rust (every
/// suite's default), or from a built release WASM so it runs in the VM like
/// Reflector does (`fx_path_cost_with_escrow_as_wasm`).
#[derive(Clone, Copy)]
enum EscrowBuild {
    Native,
    Wasm(&'static [u8]),
}

/// An initialized commitment pointed at `oracle`, driven to `Delivered` (the
/// earliest state `resolve_fx_shortfall` succeeds in).
fn escrow_at_delivered(env: &Env, oracle: &OracleUnderTest, build: EscrowBuild) -> EscrowContractClient<'static> {
    let buyer = Address::generate(env);
    let terms = Terms::default();

    let sac = env.register_stellar_asset_contract_v2(Address::generate(env));
    // Three times the deal: deposit, remainder, and an FX top-up on top.
    token::StellarAssetClient::new(env, &sac.address()).mint(&buyer, &(terms.total_amount * 3));

    let id = match build {
        EscrowBuild::Native => env.register(EscrowContract, ()),
        EscrowBuild::Wasm(wasm) => env.register(wasm, ()),
    };
    let contract = EscrowContractClient::new(env, &id);
    contract.initialize(
        &buyer,
        &Address::generate(env),
        &Address::generate(env),
        &sac.address(),
        &commitment_terms(env, &terms),
        &Some(OracleConfig {
            oracle_contract: oracle.id.clone(),
            price_asset: Symbol::new(env, oracle.price_asset),
            max_age_secs: MAX_AGE_SECS,
            denominated_amount: DENOMINATED,
        }),
    );
    contract.lock();
    contract.release_advance_1();
    contract.mark_checkpoint();
    contract.release_advance_2();
    contract.claim_advance_1();
    contract.claim_advance_2();
    contract.ready_for_delivery();
    contract.fund_remainder();
    contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    contract
}

// ---------- reporting ----------

struct Row {
    label: String,
    sample: Sample,
}

/// `oracle_rate` and `resolve_fx_shortfall` against one oracle setup, each on
/// its own fresh env so the first call cannot change the second's footprint.
fn fx_path(name: &str, escrow_build: EscrowBuild, build: fn(&Env) -> OracleUnderTest) -> [Row; 2] {
    let env = fresh_env();
    let escrow = escrow_at_delivered(&env, &build(&env), escrow_build);
    let (rate, rate_sample) = measure(&env, || escrow.oracle_rate());
    assert_eq!((rate.price, rate.timestamp), (PRICE, NOW), "{name}: wrong quote");

    let env = fresh_env();
    let escrow = escrow_at_delivered(&env, &build(&env), escrow_build);
    let ((), resolve_sample) = measure(&env, || escrow.resolve_fx_shortfall());
    let c = escrow.get_commitment();
    assert!(c.fx_resolved && c.fx_shortfall_amount > 0, "{name}: not on the shortfall path");

    [
        Row { label: format!("oracle_rate          / {name}"), sample: rate_sample },
        Row { label: format!("resolve_fx_shortfall / {name}"), sample: resolve_sample },
    ]
}

/// One direct call to the oracle, no escrow in between.
fn direct<R>(name: &str, build: fn(&Env) -> OracleUnderTest, call: fn(&Env, &reflector::Client, &str) -> R) -> Row {
    let env = fresh_env();
    let oracle = build(&env);
    let client = reflector::Client::new(&env, &oracle.id);
    let (_, sample) = measure(&env, || call(&env, &client, oracle.price_asset));
    Row { label: name.to_string(), sample }
}

fn print_table(rows: &[Row]) {
    println!(
        "\n{:<46} {:>11} {:>10} {:>6} {:>7} {:>8}",
        "call / oracle", "cpu_insns", "mem_bytes", "reads", "writes", "w_bytes"
    );
    for r in rows {
        let s = r.sample;
        println!(
            "{:<46} {:>11} {:>10} {:>6} {:>7} {:>8}",
            r.label, s.cpu, s.mem, s.read_entries, s.write_entries, s.write_bytes
        );
    }
}

fn assert_headroom(rows: &[Row]) {
    for r in rows {
        let s = r.sample;
        for (metric, used, limit) in [
            ("cpu instructions", s.cpu, TX_MAX_INSTRUCTIONS),
            ("memory bytes", s.mem, TX_MEMORY_LIMIT_BYTES),
            ("read entries", s.read_entries, TX_MAX_READ_ENTRIES),
            ("write entries", s.write_entries, TX_MAX_WRITE_ENTRIES),
            ("write bytes", s.write_bytes, TX_MAX_WRITE_BYTES),
        ] {
            assert!(
                used * 100 < limit * LIMIT_FRACTION_PERCENT,
                "{}: {metric} {used} exceeds {LIMIT_FRACTION_PERCENT}% of the per-tx limit {limit}",
                r.label,
            );
        }
    }
}

// ---------- tests ----------

/// The configured binary answers the way the live oracle does.
#[test]
fn reflector_wasm_answers_like_the_live_oracle() {
    let env = fresh_env();
    let (_, oracle) = deploy_reflector(&env);
    publish_history(&env, &oracle, &[0]);

    assert_eq!(oracle.decimals(), DECIMALS);
    assert_eq!(oracle.resolution(), RESOLUTION_MS / 1_000);
    assert_eq!(oracle.cache_size(), CACHE_SIZE);
    assert_eq!(oracle.assets().len(), QUOTED.len() as u32);
    // Reflector stores milliseconds but answers in seconds, which is what the
    // escrow's staleness check compares against the ledger clock.
    assert_eq!(
        oracle.lastprice(&asset(&env, "EUR")),
        Some(reflector::PriceData { price: PRICE, timestamp: NOW })
    );
    // The escrow's documented real target is not quoted at all.
    assert_eq!(oracle.lastprice(&asset(&env, "NGN")), None);
}

/// A behaviour of the deployed binary that the mock does not model, pinned
/// because it decides whether the FX path works: `lastprice` returns `None`
/// for an asset missing from the **latest** round, even when an earlier round
/// still inside the escrow's `max_age_secs` quoted it (and `price(asset, t)`
/// still returns that round). The public `reflector-contract` source later
/// added a fallback to the history bitmask (commit `fa1b9ac`, "Load asset last
/// price from the history mask"); this binary behaves as if it predates that.
///
/// For the escrow, one round that skips the configured asset makes
/// `oracle_rate`/`resolve_fx_shortfall` fail with `OraclePriceUnavailable`
/// until the next round that quotes it. That is a liveness gap, not a safety
/// one: nothing is written, and a retry after the next round succeeds.
#[test]
fn lastprice_sees_only_the_latest_round() {
    let env = fresh_env();
    let (id, oracle) = deploy_reflector(&env);
    publish_history(&env, &oracle, &[0]);
    // One more round, five minutes later, that quotes GBP but not EUR.
    publish_round(&env, &oracle, &[1], NOW + RESOLUTION_SECS);

    let eur = asset(&env, "EUR");
    assert_eq!(oracle.lastprice(&eur), None);
    assert_eq!(
        oracle.price(&eur, &NOW),
        Some(reflector::PriceData { price: PRICE, timestamp: NOW }),
        "the five-minute-old EUR round is still on chain"
    );

    let escrow = escrow_at_delivered(
        &env,
        &OracleUnderTest { id, price_asset: "EUR" },
        EscrowBuild::Native,
    );
    assert_eq!(escrow.try_oracle_rate(), Err(Ok(Error::OraclePriceUnavailable)));
    assert_eq!(escrow.try_resolve_fx_shortfall(), Err(Ok(Error::OraclePriceUnavailable)));
}

/// The measurement: printed with `--nocapture`, recorded in
/// `docs/gas-costs.md`, and held to the same 50%-of-limit headroom bar as
/// `tests/resource_limits.rs`.
#[test]
fn fx_path_cost_real_reflector_vs_mock() {
    let mut rows = std::vec::Vec::new();
    rows.extend(fx_path("MockOracle (native)", EscrowBuild::Native, mock));
    rows.extend(fx_path("Reflector, live", EscrowBuild::Native, reflector_live));
    rows.extend(fx_path("Reflector, full round", EscrowBuild::Native, reflector_full_round));

    rows.push(direct("Reflector.lastprice  / live", reflector_live, |env, c, a| {
        assert_eq!(c.lastprice(&asset(env, a)).map(|q| q.price), Some(PRICE));
    }));
    rows.push(direct("Reflector.lastprice  / full round", reflector_full_round, |env, c, a| {
        assert_eq!(c.lastprice(&asset(env, a)).map(|q| q.price), Some(PRICE));
    }));
    rows.push(direct("Reflector.decimals   / live", reflector_live, |_, c, _| {
        assert_eq!(c.decimals(), DECIMALS);
    }));

    print_table(&rows);
    assert_headroom(&rows);
}

/// Opt-in: the same comparison with the **escrow also running as WASM**, the
/// closest this suite gets to a production invocation. Skipped unless
/// `ESCROW_WASM` names a release build, so plain `cargo test` stays hermetic
/// and does not depend on a prior WASM build:
///
/// ```text
/// cargo build -p harvestlock-escrow --target wasm32v1-none --release
/// ESCROW_WASM=$PWD/target/wasm32v1-none/release/harvestlock_escrow.wasm \
///   cargo test -p harvestlock-escrow --test reflector_oracle_costs \
///   fx_path_cost_with_escrow_as_wasm -- --nocapture
/// ```
#[test]
fn fx_path_cost_with_escrow_as_wasm() {
    let Ok(path) = std::env::var("ESCROW_WASM") else {
        println!("ESCROW_WASM not set; skipping the escrow-as-WASM measurement");
        return;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let escrow = EscrowBuild::Wasm(std::vec::Vec::leak(bytes));

    let mut rows = std::vec::Vec::new();
    rows.extend(fx_path("escrow WASM + MockOracle", escrow, mock));
    rows.extend(fx_path("escrow WASM + Reflector, live", escrow, reflector_live));
    rows.extend(fx_path("escrow WASM + Reflector, full", escrow, reflector_full_round));

    print_table(&rows);
    assert_headroom(&rows);
}
