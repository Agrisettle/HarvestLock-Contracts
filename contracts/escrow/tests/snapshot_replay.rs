//! #125 — snapshot-based regression replay.
//!
//! ## What this actually replays, and what it does not
//!
//! `HANDOFF.md`'s "Verified on testnet" section records ten deployments with
//! contract ids, WASM hashes, and — crucially — the *figures read back off
//! the ledger* for every call (`lock` moved 300,000,000; `reclaim_advance_2`
//! returned 200,000,000; `settle` paid 105,300,006 …). What it does **not**
//! contain is a single transaction envelope XDR, auth-entries XDR, or an
//! exported ledger snapshot: nothing in this repo records raw bytes, and the
//! instances themselves are explicitly declared "validation artifacts, not
//! infrastructure" (HANDOFF.md, after Deployment 10) — several are already
//! unreachable in practice.
//!
//! So this is **not** a replay of recorded chain bytes. It is a replay of
//! *recorded chain outcomes*: each fixture under `tests/fixtures/` is a
//! call/args/expected-outcome transcript transcribed from the numbers
//! HANDOFF.md states were read back from testnet, driven through the same
//! public entry points against the current build. `provenance =` in every
//! fixture names exactly which deployment it came from and how faithful it
//! is. If real XDRs are ever captured (e.g. from `stellar rpc getTransaction`
//! for the ids above), the loader's `Step`/`Snapshot` types are the seam to
//! swap: the expectations stay the same, the transport changes.
//!
//! Two outcomes count as this suite doing its job:
//! 1. green — the current build reproduces every recorded number, or
//! 2. a real, actionable finding — the current build *diverges* from a
//!    recorded transaction. It does, on one snapshot:
//!    `deployment-2-pre-twophase-funding.snap` ran on the pre-two-phase-funding
//!    WASM, and three of its recorded outcomes (the `settle` rejection *reason*,
//!    a `confirm_delivery` that succeeded, and the closing `settle` itself) are
//!    not reproduced by today's build. Those gaps are declared in the fixture
//!    with `known_divergence =` lines, which the replay asserts in both
//!    directions — recorded must still differ, current must still match — so a
//!    divergence can neither be silently papered over nor quietly go stale. An
//!    *undeclared* gap fails the job outright.
//!
//! ## Fixture format
//!
//! Line-oriented, no third-party parser (this crate has no `serde` dependency
//! of its own, and a diffable text format keeps a recorded transcript reviewable
//! in a PR). Full spec in `tests/fixtures/README.md`. Sketch:
//!
//! ```text
//! snapshot = deployment-5-buyer-default
//! provenance = derived from HANDOFF.md Deployment 5 scenario 2 (instance CA3AB4QX…),
//!   amounts read back from testnet; no tx XDR is recorded in this repo.
//! total_amount = 1000000000
//! advance1_bps = 1000
//! claim_window_secs = 604800
//! grade_price_bps = 10000,9000,7500
//! step = lock => ok
//! step = advance 16
//! step = expire_remainder_window => ok
//! check = status Defaulted
//! check = coop_balance 200000000
//! ```

use harvestlock_escrow::{
    AllocationMember, Error, EscrowContract, EscrowContractClient, OracleConfig, Status,
};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, BytesN, Env, Symbol, Vec as OnChainVec};
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

// ---------- the fixed-behavior oracle a snapshot may point at ----------

/// Not a fault injector (that's `tests/oracle_fault_injection.rs`) — this one
/// exists only so FX snapshots can reproduce a *recorded* rate, including
/// Deployment 9's real live GBP/USD quote. It answers the same quote forever
/// and never ages, so a snapshot's expectation is about the contract's math,
/// not about the simulated feed.
mod fixture_oracle {
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
    pub struct FixtureOracle;

    #[contractimpl]
    impl FixtureOracle {
        pub fn set_quote(env: Env, price: i128, timestamp: u64, decimals: u32) {
            env.storage()
                .instance()
                .set(&symbol_short!("quote"), &(price, timestamp));
            env.storage()
                .instance()
                .set(&symbol_short!("dec"), &decimals);
        }
        pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
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

// ---------- snapshot model ----------

#[derive(Debug, Clone)]
struct Terms {
    total_amount: i128,
    advance1_bps: u32,
    advance2_bps: u32,
    claim_window_secs: u64,
    remainder_window_secs: u64,
    delivery_window_secs: u64,
    contracted_quantity: u32,
    /// Plain `std::vec::Vec` — a soroban `Vec` belongs to a specific `Env`,
    /// and the fixture's grades have no env until `build` creates one.
    grade_price_bps: Vec<u32>,
    buyer_mint: i128,
    /// `symbol,denominated_amount,max_age_secs,price,decimals`
    oracle: Option<(String, i128, u64, i128, u32)>,
}

impl Default for Terms {
    fn default() -> Self {
        Self {
            total_amount: 1_000_000,
            advance1_bps: 1_500,
            advance2_bps: 1_500,
            claim_window_secs: 7 * 24 * 60 * 60,
            remainder_window_secs: 7 * 24 * 60 * 60,
            delivery_window_secs: 120 * 24 * 60 * 60,
            contracted_quantity: 1_000,
            grade_price_bps: Vec::new(),
            buyer_mint: 3_000_000,
            oracle: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Expectation {
    Ok,
    ContractError(Error),
}

#[derive(Debug, Clone)]
struct Step {
    call: String,
    args: Vec<String>,
    expect: Expectation,
    /// Set by a `known_divergence =` line immediately above this step: the
    /// recorded outcome below is *not* what the current build does, and this is
    /// what it does instead. The replay asserts both halves, so the divergence
    /// stays a documented finding instead of either a red build or a silence.
    current: Option<Expectation>,
    file: String,
    line: usize,
}

impl Step {
    /// `call arg1 arg2` — the identity a `known_divergence` line has to repeat
    /// so annotations cannot drift onto the wrong step.
    fn signature(&self) -> String {
        signature_of(&self.call, &self.args)
    }
}

fn signature_of(call: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(call);
    parts.extend(args.iter().map(String::as_str));
    parts.join(" ")
}

#[derive(Debug)]
struct Snapshot {
    name: String,
    provenance: String,
    terms: Terms,
    /// Steps and `check =` lines, in the order the fixture wrote them: a
    /// recorded balance assertion belongs to a point in the transcript, not
    /// to the end of it, so both are replayed as one ordered sequence.
    steps: Vec<Step>,
    path: PathBuf,
}

fn parse_error(file: &str, line: usize, msg: &str) -> ! {
    panic!(
        "fixture {file}:{line}: {msg}\n\
         Fixtures are parsed by tests/snapshot_replay.rs; the format is \
         documented in tests/fixtures/README.md."
    )
}

fn err_name(name: &str) -> Error {
    let table: &[(&str, Error)] = &[
        ("AlreadyInitialized", Error::AlreadyInitialized),
        ("NotInitialized", Error::NotInitialized),
        ("InvalidState", Error::InvalidState),
        ("InvalidBps", Error::InvalidBps),
        ("ZeroAmount", Error::ZeroAmount),
        ("InvalidWindow", Error::InvalidWindow),
        ("NotYetOpened", Error::NotYetOpened),
        ("AlreadyClaimed", Error::AlreadyClaimed),
        ("AlreadyExpired", Error::AlreadyExpired),
        ("ClaimWindowPassed", Error::ClaimWindowPassed),
        ("ClaimWindowNotPassed", Error::ClaimWindowNotPassed),
        ("TrancheUnresolved", Error::TrancheUnresolved),
        ("RemainderAlreadyFunded", Error::RemainderAlreadyFunded),
        ("RemainderWindowPassed", Error::RemainderWindowPassed),
        ("RemainderWindowNotPassed", Error::RemainderWindowNotPassed),
        ("RemainderNotFunded", Error::RemainderNotFunded),
        (
            "DeliveryDeadlineNotPassed",
            Error::DeliveryDeadlineNotPassed,
        ),
        ("InvalidQuantity", Error::InvalidQuantity),
        ("InvalidGradeSchedule", Error::InvalidGradeSchedule),
        ("InvalidGradeIndex", Error::InvalidGradeIndex),
        ("AllocationAlreadySet", Error::AllocationAlreadySet),
        ("InvalidAllocation", Error::InvalidAllocation),
        ("AllocationNotSet", Error::AllocationNotSet),
        ("InvalidOracleConfig", Error::InvalidOracleConfig),
        ("OracleNotConfigured", Error::OracleNotConfigured),
        ("OraclePriceUnavailable", Error::OraclePriceUnavailable),
        ("OracleStale", Error::OracleStale),
        ("FxAlreadyResolved", Error::FxAlreadyResolved),
        ("FxNotResolved", Error::FxNotResolved),
        ("FxShortfallUnfunded", Error::FxShortfallUnfunded),
        ("NoFxShortfall", Error::NoFxShortfall),
        ("FxShortfallAlreadyFunded", Error::FxShortfallAlreadyFunded),
        ("FxShortfallWindowPassed", Error::FxShortfallWindowPassed),
        (
            "FxShortfallWindowNotPassed",
            Error::FxShortfallWindowNotPassed,
        ),
        ("NotAParty", Error::NotAParty),
        ("DisputeWindowNotPassed", Error::DisputeWindowNotPassed),
    ];
    table.iter().find(|(n, _)| *n == name).map_or_else(
        || panic!("fixture references unknown Error variant `{name}`"),
        |(_, e)| *e,
    )
}

fn status_name(name: &str) -> Status {
    let table: &[(&str, Status)] = &[
        ("Draft", Status::Draft),
        ("Locked", Status::Locked),
        ("Advance1Released", Status::Advance1Released),
        ("CheckpointPassed", Status::CheckpointPassed),
        ("Advance2Released", Status::Advance2Released),
        ("ReadyForDelivery", Status::ReadyForDelivery),
        ("Delivered", Status::Delivered),
        ("Settled", Status::Settled),
        ("Cancelled", Status::Cancelled),
        ("Defaulted", Status::Defaulted),
        ("Forfeited", Status::Forfeited),
        ("Disputed", Status::Disputed),
    ];
    table.iter().find(|(n, _)| *n == name).map_or_else(
        || panic!("fixture references unknown Status variant `{name}`"),
        |(_, s)| *s,
    )
}

/// The whole fixture directory, parsed. Any parse failure panics with the
/// offending file and line — a malformed transcript should fail the job, not
/// be silently skipped.
fn load_snapshots() -> Vec<Snapshot> {
    let dir = Path::new(FIXTURE_DIR);
    assert!(
        dir.is_dir(),
        "fixture directory {} is missing",
        dir.display()
    );
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("readable fixture dir").path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("snap"))
        .collect();
    files.sort();
    files.iter().map(|p| load_snapshot(p)).collect()
}

fn load_snapshot(path: &Path) -> Snapshot {
    let file = path.file_name().unwrap().to_string_lossy().to_string();
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut terms = Terms::default();
    let mut grade_bps: Option<Vec<u32>> = None;
    let mut name = String::new();
    let mut provenance = String::new();
    let mut steps = Vec::new();
    let mut pending: Option<(String, Expectation, usize)> = None;
    let mut saw_snapshot_line = false;

    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // A continuation of the previous `provenance` value: HANDOFF-derived
        // provenance notes are long, so fixtures wrap them over indented
        // lines instead of cramming one field onto one line.
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !provenance.is_empty() {
            provenance.push(' ');
            provenance.push_str(trimmed);
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            parse_error(
                &file,
                line,
                &format!("expected `key = value`, got `{trimmed}`"),
            );
        };
        let key = key.trim();
        let value = value.trim();

        match key {
            "snapshot" => {
                name = value.to_string();
                saw_snapshot_line = true;
            }
            "provenance" => provenance = value.to_string(),
            "note" => {} // free-form commentary for reviewers
            "total_amount" => terms.total_amount = num(value, &file, line),
            "advance1_bps" => terms.advance1_bps = num(value, &file, line),
            "advance2_bps" => terms.advance2_bps = num(value, &file, line),
            "claim_window_secs" => terms.claim_window_secs = num(value, &file, line),
            "remainder_window_secs" => terms.remainder_window_secs = num(value, &file, line),
            "delivery_window_secs" => terms.delivery_window_secs = num(value, &file, line),
            "contracted_quantity" => terms.contracted_quantity = num(value, &file, line),
            "buyer_mint" => terms.buyer_mint = num(value, &file, line),
            "grade_price_bps" => {
                grade_bps = Some(
                    value
                        .split(',')
                        .map(|s| num(s.trim(), &file, line))
                        .collect(),
                );
            }
            "oracle" => {
                let parts: Vec<&str> = value.split(',').map(str::trim).collect();
                if parts.len() != 5 {
                    parse_error(
                        &file,
                        line,
                        "oracle = symbol,denominated_amount,max_age_secs,price,decimals",
                    );
                }
                terms.oracle = Some((
                    parts[0].to_string(),
                    num(parts[1], &file, line),
                    num(parts[2], &file, line),
                    num(parts[3], &file, line),
                    num(parts[4], &file, line),
                ));
            }
            "step" => {
                let (call, args, expectation) = parse_step_value(value, &file, line);
                steps.push(Step {
                    current: take_divergence(&mut pending, &file, line, &call, &args),
                    call,
                    args,
                    expect: expectation,
                    file: file.clone(),
                    line,
                });
            }
            "known_divergence" => {
                // `known_divergence = <call> [args...] => <today's outcome>`
                // Annotates the next `step` line, which must repeat the same
                // call and arguments.
                let (call, args, expectation) = parse_step_value(value, &file, line);
                let signature = signature_of(&call, &args);
                if pending.replace((signature, expectation, line)).is_some() {
                    parse_error(
                        &file,
                        line,
                        "two `known_divergence` lines in a row; the first was never consumed by a step",
                    );
                }
            }
            "check" => {
                let Some((k, v)) = value.split_once(' ') else {
                    parse_error(&file, line, "check = <key> <value>");
                };
                steps.push(Step {
                    call: "check".to_string(),
                    args: vec![k.to_string(), v.trim().to_string()],
                    expect: Expectation::Ok,
                    current: None,
                    file: file.clone(),
                    line,
                });
            }
            other => parse_error(&file, line, &format!("unknown fixture key `{other}`")),
        }
    }

    if let Some((signature, _, declared_line)) = pending {
        parse_error(
            &file,
            declared_line,
            &format!("`known_divergence = {signature}` is not followed by a matching step"),
        );
    }

    if !saw_snapshot_line {
        parse_error(&file, 0, "fixture has no `snapshot =` name line");
    }
    if provenance.is_empty() {
        parse_error(
            &file,
            0,
            "fixture has no `provenance =` line — #125 requires every \
             snapshot to say where its numbers came from",
        );
    }
    if let Some(g) = grade_bps {
        terms.grade_price_bps = g;
    }

    Snapshot {
        name,
        provenance,
        terms,
        steps,
        path: path.to_path_buf(),
    }
}

/// Parses the right-hand side of a `step =` / `known_divergence =` line into
/// `(call, args, expectation)`.
///
/// Time shifts (`advance`, `set_time`) are engine actions rather than contract
/// calls, so they may omit the expectation; anything that reaches the ledger
/// has to state one.
fn parse_step_value(value: &str, file: &str, line: usize) -> (String, Vec<String>, Expectation) {
    let (lhs, expect) = match value.rsplit_once("=>") {
        Some((lhs, expect)) => (lhs, Some(expect.trim())),
        None => (value, None),
    };
    let mut tokens = lhs
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        parse_error(file, line, "step has no call name");
    }
    let call = tokens.remove(0);
    let expectation = match expect {
        None if matches!(call.as_str(), "advance" | "set_time") => Expectation::Ok,
        None => parse_error(
            file,
            line,
            "a contract-call step needs `=> ok` or `=> err(Name)`",
        ),
        Some(e) => match e.strip_prefix("err(").and_then(|s| s.strip_suffix(')')) {
            Some(n) => Expectation::ContractError(err_name(n.trim())),
            None if e == "ok" => Expectation::Ok,
            other => parse_error(file, line, &format!("unknown expectation `{other:?}`")),
        },
    };
    (call, tokens, expectation)
}

/// Consumes the `known_divergence` annotation queued for this step, if the
/// fixture declared one, and refuses to continue if it names a different call:
/// a divergence attached to the wrong step would document the wrong regression.
fn take_divergence(
    pending: &mut Option<(String, Expectation, usize)>,
    file: &str,
    line: usize,
    call: &str,
    args: &[String],
) -> Option<Expectation> {
    let (signature, _declared_expectation, declared_line) = pending.as_ref()?;
    if *signature != signature_of(call, args) {
        parse_error(
            file,
            line,
            &format!(
                "`known_divergence = {signature}` on line {declared_line} does not name \
                 this step (`{}`); annotations must immediately precede the step they describe",
                signature_of(call, args)
            ),
        );
    }
    pending.take().map(|(_, e, _)| e)
}

fn num<T>(s: &str, file: &str, line: usize) -> T
where
    T: std::str::FromStr,
{
    s.trim()
        .parse()
        .unwrap_or_else(|_| parse_error(file, line, &format!("`{s}` is not a number")))
}

// ---------- the replay engine ----------

/// Which party a fixture step names as an address argument.
#[derive(Clone, Copy)]
enum Party {
    Buyer,
    Cooperative,
    Warehouse,
    Stranger,
}

struct Scene {
    env: Env,
    contract: EscrowContractClient<'static>,
    token: token::Client<'static>,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    stranger: Address,
}

impl Scene {
    fn addr(&self, p: Party) -> Address {
        match p {
            Party::Buyer => self.buyer.clone(),
            Party::Cooperative => self.cooperative.clone(),
            Party::Warehouse => self.warehouse.clone(),
            Party::Stranger => self.stranger.clone(),
        }
    }
}

fn party_of(s: &str) -> Party {
    match s {
        "buyer" => Party::Buyer,
        "cooperative" => Party::Cooperative,
        "warehouse" => Party::Warehouse,
        "stranger" => Party::Stranger,
        other => panic!("fixture names an unknown party `{other}`"),
    }
}

fn hex_to_32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let b = s.as_bytes();
    assert_eq!(
        b.len(),
        64,
        "member_hash must be 64 hex chars, got {}",
        b.len()
    );
    for i in 0..32 {
        let digit = |c: u8| {
            char::from(c)
                .to_digit(16)
                .unwrap_or_else(|| panic!("member_hash must be hex, found `{}`", char::from(c)))
        };
        let hi = digit(b[i * 2]);
        let lo = digit(b[i * 2 + 1]);
        out[i] = u8::try_from((hi << 4) | lo).expect("two hexadecimal digits fit in one byte");
    }
    out
}

/// Builds a fresh instance per snapshot: nothing carries over between
/// transcripts, so a fixture can never depend on another fixture's state.
fn build(snap: &Snapshot) -> Scene {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    let token = token::Client::new(&env, &token_address);
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &snap.terms.buyer_mint);

    let oracle_config =
        snap.terms
            .oracle
            .as_ref()
            .map(|(symbol, denominated, max_age, price, decimals)| {
                let id = env.register(fixture_oracle::FixtureOracle, ());
                fixture_oracle::FixtureOracleClient::new(&env, &id).set_quote(
                    price,
                    &env.ledger().timestamp(),
                    decimals,
                );
                OracleConfig {
                    oracle_contract: id,
                    price_asset: Symbol::new(&env, symbol),
                    max_age_secs: *max_age,
                    denominated_amount: *denominated,
                }
            });

    let mut grade_schedule = OnChainVec::<u32>::new(&env);
    for g in &snap.terms.grade_price_bps {
        grade_schedule.push_back(*g);
    }

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(&env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &snap.terms.total_amount,
        &snap.terms.advance1_bps,
        &snap.terms.advance2_bps,
        &snap.terms.claim_window_secs,
        &snap.terms.remainder_window_secs,
        &snap.terms.delivery_window_secs,
        &snap.terms.contracted_quantity,
        &grade_schedule,
        &oracle_config,
    );

    Scene {
        env: env.clone(),
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        stranger,
    }
}

/// Classifies a generated client's `try_*` result without naming any of the
/// SDK's error wrapper types, so the engine stays agnostic of the call's
/// success type. `Ok(Ok(_))` is a pass, `Err(Ok(e))` is the contract's own
/// error, anything else is a host-level abort.
macro_rules! classify {
    ($expr:expr) => {
        match $expr {
            Ok(Ok(_v)) => Outcome::Passed,
            Ok(Err(c)) => panic!("could not decode the call's return value: {c:?}"),
            Err(Ok(e)) => Outcome::ContractError(e),
            Err(Err(_h)) => Outcome::Aborted,
        }
    };
}

enum Outcome {
    Passed,
    ContractError(Error),
    Aborted,
}

fn run_step(scene: &Scene, step: &Step) -> Outcome {
    let c = &scene.contract;
    let a = |i: usize| -> String {
        step.args.get(i).cloned().unwrap_or_else(|| {
            panic!(
                "{}:{} `{}` needs more arguments",
                step.file, step.line, step.call
            )
        })
    };
    let qty = |i: usize| -> u32 { num(&a(i), &step.file, step.line) };

    match step.call.as_str() {
        "lock" => classify!(c.try_lock()),
        "release_advance_1" => classify!(c.try_release_advance_1()),
        "claim_advance_1" => classify!(c.try_claim_advance_1()),
        "reclaim_advance_1" => classify!(c.try_reclaim_advance_1()),
        "mark_checkpoint" => classify!(c.try_mark_checkpoint()),
        "release_advance_2" => classify!(c.try_release_advance_2()),
        "claim_advance_2" => classify!(c.try_claim_advance_2()),
        "reclaim_advance_2" => classify!(c.try_reclaim_advance_2()),
        "ready_for_delivery" => classify!(c.try_ready_for_delivery()),
        "fund_remainder" => classify!(c.try_fund_remainder()),
        "expire_remainder_window" => classify!(c.try_expire_remainder_window()),
        "reclaim_on_nondelivery" => classify!(c.try_reclaim_on_nondelivery()),
        "confirm_delivery" => {
            let (delivered, grade) = (qty(0), qty(1));
            classify!(c.try_confirm_delivery(&delivered, &grade))
        }
        "settle" => classify!(c.try_settle()),
        "cancel" => classify!(c.try_cancel()),
        "resolve_dispute" => classify!(c.try_resolve_dispute()),
        "expire_dispute_window" => classify!(c.try_expire_dispute_window()),
        "resolve_fx_shortfall" => classify!(c.try_resolve_fx_shortfall()),
        "fund_fx_shortfall" => classify!(c.try_fund_fx_shortfall()),
        "expire_fx_shortfall_window" => classify!(c.try_expire_fx_shortfall_window()),
        "flag_dispute" => {
            let who = scene.addr(party_of(&a(0)));
            classify!(c.try_flag_dispute(&who))
        }
        "reassign_buyer" => {
            let who = scene.addr(party_of(&a(0)));
            classify!(c.try_reassign_buyer(&who))
        }
        "set_allocation" => {
            // `set_allocation <hex32>:<share_bps> [<hex32>:<share_bps> ...]`
            let env = &scene.env;
            let mut members = OnChainVec::<AllocationMember>::new(env);
            for spec in &step.args {
                let (hash, bps) = spec.split_once(':').unwrap_or_else(|| {
                    panic!(
                        "{}:{} bad allocation member `{spec}` (want hex:bps)",
                        step.file, step.line
                    )
                });
                let bytes = hex_to_32(hash);
                members.push_back(AllocationMember {
                    member_hash: BytesN::from_array(env, &bytes),
                    share_bps: num(bps, &step.file, step.line),
                });
            }
            classify!(c.try_set_allocation(&members))
        }
        "check" => {
            let (key, want) = (a(0), a(1));
            if let Err(e) = check_value(scene, &key, &want) {
                panic!(
                    "{}:{} recorded check `{key} {want}` failed: {e}\n\
                     either the current build diverged from the recorded \
                     outcome, or the fixture's arithmetic is wrong",
                    step.file, step.line
                );
            }
            Outcome::Passed
        }
        "advance" => {
            let secs: u64 = num(&a(0), &step.file, step.line);
            let now = scene.env.ledger().timestamp();
            scene.env.ledger().set_timestamp(now + secs);
            Outcome::Passed
        }
        "set_time" => {
            let t: u64 = num(&a(0), &step.file, step.line);
            scene.env.ledger().set_timestamp(t);
            Outcome::Passed
        }
        other => panic!(
            "{}:{} unsupported call `{other}` — the replay engine needs a \
             match arm for it (tests/snapshot_replay.rs::run_step)",
            step.file, step.line
        ),
    }
}

fn check_value(scene: &Scene, key: &str, want: &str) -> Result<(), String> {
    let c = scene.contract.get_commitment();
    let balance = |a: &Address| scene.token.balance(a);
    let contract_address = scene.contract.address.clone();
    let eq_i128 = |got: i128| {
        let want_n: i128 = want
            .parse()
            .map_err(|_| format!("check value `{want}` is not an integer"))?;
        if got == want_n {
            Ok(())
        } else {
            Err(format!("expected {want_n}, got {got}"))
        }
    };
    let eq_bool = |got: bool| {
        let want_b = matches!(want, "true" | "yes" | "1");
        if got == want_b {
            Ok(())
        } else {
            Err(format!("expected {want_b}, got {got}"))
        }
    };

    match key {
        "status" => {
            if c.status == status_name(want) {
                Ok(())
            } else {
                Err(format!("expected {want}, got {:?}", c.status))
            }
        }
        "coop_balance" => eq_i128(balance(&scene.cooperative)),
        "buyer_balance" => eq_i128(balance(&scene.buyer)),
        "contract_balance" => eq_i128(balance(&contract_address)),
        "stranger_balance" => eq_i128(balance(&scene.stranger)),
        "settlement_bps" => eq_i128(i128::from(c.settlement_bps)),
        "fx_adjusted_total" => eq_i128(c.fx_adjusted_total),
        "fx_shortfall_amount" => eq_i128(c.fx_shortfall_amount),
        "fx_resolved" => eq_bool(c.fx_resolved),
        "fx_shortfall_funded" => eq_bool(c.fx_shortfall_funded),
        "remainder_funded" => eq_bool(c.remainder_funded),
        "advance1_claimed" => eq_bool(c.advance1_claimed),
        "advance1_expired" => eq_bool(c.advance1_expired),
        "advance2_claimed" => eq_bool(c.advance2_claimed),
        "advance2_expired" => eq_bool(c.advance2_expired),
        "dispute_pre_status" => {
            if c.dispute_pre_status == status_name(want) {
                Ok(())
            } else {
                Err(format!("expected {want}, got {:?}", c.dispute_pre_status))
            }
        }
        "delivered_quantity" => eq_i128(i128::from(c.delivered_quantity)),
        "buyer_party" => {
            if c.buyer == scene.addr(party_of(want)) {
                Ok(())
            } else {
                Err(format!("expected buyer to be {want}"))
            }
        }
        "allocation_count" => match scene.contract.try_get_allocation() {
            Ok(Ok(v)) => eq_i128(i128::from(v.len())),
            Ok(Err(c)) => Err(format!("could not decode get_allocation's return: {c:?}")),
            Err(Ok(e)) => Err(format!("get_allocation returned {e:?}")),
            Err(Err(_)) => Err("get_allocation aborted at the host level".to_string()),
        },
        other => Err(format!(
            "unsupported check key `{other}` — add it to \
             tests/snapshot_replay.rs::check_value"
        )),
    }
}

/// Replays one transcript, returning the recorded-vs-current divergences it
/// confirmed on the way.
fn replay(snap: &Snapshot) -> Vec<String> {
    let scene = build(snap);
    let mut divergences = Vec::new();
    for step in &snap.steps {
        let outcome = run_step(&scene, step);
        let step_text = format!("{}:{} step `{}`", step.file, step.line, step.signature());
        match &step.current {
            // A declared divergence: today's build must do exactly what the
            // fixture says it does, and must *not* still do what the chain
            // recorded. Both halves are asserted, so the annotation cannot go
            // stale in either direction — behavior restored or behavior shifted
            // again both fail the job.
            Some(current) => {
                assert!(
                    matches_expectation(current, &outcome),
                    "{step_text}: the fixture declares that today's build answers {}, \
                     but it answers {}.\n  If the current behavior changed on purpose, \
                     update the `known_divergence` line; if it did not, this is a real \
                     regression on top of the recorded divergence.",
                    expect_dbg(current),
                    outcome_dbg(&outcome),
                );
                assert!(
                    !matches_expectation(&step.expect, &outcome),
                    "{step_text}: the `known_divergence` annotation above it is stale — \
                     the current build now reproduces the recorded outcome ({}). \
                     Delete the annotation so this step replays as a match.",
                    expect_dbg(&step.expect),
                );
                divergences.push(format!(
                    "{step_text}: recorded {}, current build gives {}",
                    expect_dbg(&step.expect),
                    expect_dbg(current),
                ));
            }
            None => assert!(
                matches_expectation(&step.expect, &outcome),
                "{step_text}\n  {} expected {}, got {}",
                snap.provenance,
                expect_dbg(&step.expect),
                outcome_dbg(&outcome),
            ),
        }
    }
    divergences
}

fn matches_expectation(want: &Expectation, got: &Outcome) -> bool {
    match (want, got) {
        (Expectation::Ok, Outcome::Passed) => true,
        (Expectation::ContractError(w), Outcome::ContractError(g)) => w == g,
        _ => false,
    }
}

/// Failure messages spell out the error variant: "a contract error" tells a
/// reviewer nothing about *which* recorded rejection changed.
fn expect_dbg(e: &Expectation) -> String {
    match e {
        Expectation::Ok => "ok".to_string(),
        Expectation::ContractError(err) => format!("{err:?}"),
    }
}

fn outcome_dbg(o: &Outcome) -> String {
    match o {
        Outcome::Passed => "ok".to_string(),
        Outcome::ContractError(e) => format!("{e:?}"),
        Outcome::Aborted => "host abort".to_string(),
    }
}

// ---------- the suite ----------

#[test]
fn replay_all_recorded_snapshots() {
    let snapshots = load_snapshots();
    let mut report = Vec::new();
    let mut divergences = Vec::new();
    for snap in &snapshots {
        divergences.extend(replay(snap));
        let checks = snap.steps.iter().filter(|s| s.call == "check").count();
        let calls = snap.steps.len() - checks;
        report.push(format!(
            "  {} — {} recorded calls, {} outcome assertions (replayed)",
            snap.name, calls, checks
        ));
    }
    println!(
        "replayed {} recorded snapshots:\n{}",
        snapshots.len(),
        report.join("\n")
    );
    if divergences.is_empty() {
        println!("no recorded outcome diverges from the current build.");
    } else {
        println!(
            "\n{} recorded outcome(s) the current build does NOT reproduce \
             (each is declared by a `known_divergence` line in its fixture):\n{}",
            divergences.len(),
            divergences
                .iter()
                .map(|d| format!("  ! {d}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// The recorded-vs-current gaps are findings, so they are asserted to exist.
/// If one disappears — because the behavior was restored, or because someone
/// deleted the annotation — this names it instead of going quiet.
#[test]
fn the_recorded_divergences_are_still_divergences() {
    let found: Vec<String> = load_snapshots().iter().flat_map(replay).collect();
    for expected in [
        // The status gate now fires before the tranche gate, so an out-of-order
        // `settle` reports a different error than the one read off the chain.
        "deployment-2-pre-twophase-funding.snap step `settle`: recorded TrancheUnresolved, current build gives InvalidState",
        // Delivery confirmation used to be reachable straight from Advance2Released.
        "deployment-2-pre-twophase-funding.snap step `confirm_delivery 1000 0`: recorded ok, current build gives InvalidState",
        // The recorded closing `settle` swept the remaining 650,000,000; today's
        // build refuses to settle an unfunded, unconfirmed commitment at all.
        "deployment-2-pre-twophase-funding.snap step `settle`: recorded ok, current build gives InvalidState",
    ] {
        // Matched without the line number: the fixture's prose is allowed to move.
        let (file, tail) = expected.split_once(" step `").unwrap();
        assert!(
            found
                .iter()
                .any(|d| d.starts_with(file) && d.contains(&format!("step `{tail}"))),
            "expected divergence not reproduced by the harness: {expected}\n  found: {found:?}"
        );
    }
    assert_eq!(
        found.len(),
        3,
        "the harness reports {} declared divergences, and this suite documents exactly \
         3 (all from the pre-two-phase-funding deployment). A fourth is either a new \
         recorded-vs-current gap — name it in the fixture and here — or a fixture that \
         no longer says what it records.",
        found.len()
    );
}

/// The loader itself is under test: fixtures that do not parse, or that parse
/// into nothing, would otherwise make `replay_all_recorded_snapshots` green by
/// accident.
#[test]
fn fixtures_are_present_named_and_sourced() {
    let snapshots = load_snapshots();
    assert!(
        snapshots.len() >= 6,
        "expected at least 6 recorded snapshots, found {} — deleting fixtures \
         must not be able to make this suite pass silently",
        snapshots.len()
    );
    for s in &snapshots {
        assert!(
            !s.name.is_empty(),
            "{} has no `snapshot =` name",
            s.path.display()
        );
        assert!(
            s.provenance.contains("HANDOFF.md"),
            "{}'s provenance must cite HANDOFF.md's deployment history (it says: {})",
            s.path.display(),
            s.provenance
        );
        assert!(
            !s.steps.is_empty(),
            "{} records no steps — an empty transcript proves nothing",
            s.path.display()
        );
        assert!(
            s.steps.iter().any(|st| st.call == "check"),
            "{} has no recorded outcome checks",
            s.path.display()
        );
        assert!(
            s.path.file_name().unwrap().to_string_lossy() == format!("{}.snap", s.name),
            "{}: `snapshot =` name must match the file name",
            s.path.display()
        );
    }
}

/// One recorded rejection must stay a rejection: Deployment 10's live testnet
/// run shows `release_advance_1` being refused *on chain* while a dispute is
/// open. Asserted here directly, rather than only through the fixture, because
/// it is the single strongest "the freeze is real, not a label" claim in the
/// deployment history.
#[test]
fn the_recorded_dispute_freeze_rejection_is_still_a_rejection() {
    let snap = load_snapshots()
        .into_iter()
        .find(|s| s.name == "deployment-10-dispute-freeze")
        .expect("deployment-10-dispute-freeze.snap must exist (#125)");
    let scene = build(&snap);
    let c = &scene.contract;
    c.lock();
    c.flag_dispute(&scene.cooperative);
    assert_eq!(c.get_status(), Status::Disputed);
    let blocked = match classify!(c.try_release_advance_1()) {
        Outcome::ContractError(e) => e,
        other => panic!(
            "the recorded on-chain rejection changed shape: {}",
            outcome_dbg(&other)
        ),
    };
    assert_eq!(blocked, Error::InvalidState);
}
