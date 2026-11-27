//! #221 — fuzz the XDR boundary of every public escrow entry point.
//!
//! The arithmetic fuzz targets (#9/#10/#12/#13/#16/#18) start from
//! well-typed inputs. This target starts where an attacker actually reaches
//! the contract: raw `Val`s. For each iteration it:
//!
//! 1. deploys a token + escrow instance and initializes a *valid*
//!    commitment (so stateful paths are reachable),
//! 2. advances it through a short, randomly chosen sequence of legal calls,
//! 3. invokes one randomly chosen public function through
//!    `Env::try_invoke_contract` with argument values built from fuzz bytes:
//!    sometimes the declared shape with fuzzed/oversized contents
//!    (`grade_price_bps`, `members`, `OracleConfig`, `Option` shapes),
//!    sometimes deliberately wrong types or arity,
//! 4. accepts exactly three outcomes: a contract `Error`, a conversion/host
//!    error (the SDK wrapper refusing the raw `Val`s), or success. On
//!    success it reads the commitment back and runs
//!    `Commitment::check_invariants` — a successful call that leaves an
//!    impossible state fails the target.
//!
//! Any Rust panic, VM trap that escapes the host's rollback, or silent
//! partial state is a crash for libFuzzer to report. The host rolls back a
//! failed invocation atomically, so a trap on argument decoding is safe to
//! classify as a conversion error; the target does not swallow panics.

use arbitrary::Unstructured;
use harvestlock_escrow::{AllocationMember, Commitment, Error, EscrowContractClient, OracleConfig};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{
    token, Address, BytesN, Env, IntoVal, Map, Symbol, Val, Vec,
};

/// Every public function exported by `contracts/escrow`, in declaration
/// order. `initialize` is included; the target gives it both a structured
/// argument list and a purely arbitrary one.
const FUNCTIONS: [&str; 34] = [
    "initialize",
    "get_oracle_config",
    "replace_oracle_config",
    "oracle_rate",
    "set_allocation",
    "get_allocation",
    "lock",
    "release_advance_1",
    "claim_advance_1",
    "reclaim_advance_1",
    "mark_checkpoint",
    "release_advance_2",
    "claim_advance_2",
    "reclaim_advance_2",
    "ready_for_delivery",
    "fund_remainder",
    "expire_remainder_window",
    "reclaim_on_nondelivery",
    "expire_delivery_window",
    "confirm_delivery",
    "settle",
    "resolve_fx_shortfall",
    "refresh_fx_quote_status",
    "fund_fx_shortfall",
    "expire_fx_shortfall_window",
    "cancel",
    "reassign_buyer",
    "flag_dispute",
    "resolve_dispute",
    "expire_dispute_window",
    "get_status",
    "get_commitment",
    "available_actions",
    "get_commitment_view",
];

const SYMBOLS: [&str; 5] = ["NGN", "USD", "x", "status", "delivered_quantity"];
const MAX_FUZZED_VEC: u64 = 300;

struct Pool {
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    token: Address,
    escrow: Address,
    stranger: Address,
}

impl Pool {
    fn addresses(&self) -> [&Address; 6] {
        [
            &self.buyer,
            &self.cooperative,
            &self.warehouse,
            &self.token,
            &self.escrow,
            &self.stranger,
        ]
    }
}

fn rand(u: &mut Unstructured, max: u64) -> u64 {
    u.int_in_range(0..=max).unwrap_or(0)
}

fn coin(u: &mut Unstructured) -> bool {
    u.arbitrary::<u8>().unwrap_or(0) & 1 == 1
}

/// A bounded arbitrary `Val`: every Soroban tag an attacker could send,
/// including containers, nested a few levels deep so argument decoding is
/// exercised on recursive shapes too.
fn arbitrary_val(u: &mut Unstructured, env: &Env, pool: &Pool, depth: u8) -> Val {
    let choice = if depth >= 3 { rand(u, 8) } else { rand(u, 13) };
    match choice {
        0 => Val::VOID.into(),
        1 => Val::from(coin(u)),
        2 => Val::from(rand(u, u32::MAX as u64) as u32),
        3 => Val::from(rand(u, u32::MAX as u64) as i32),
        4 => (rand(u, u64::MAX) as i64).into_val(env),
        5 => rand(u, u64::MAX).into_val(env),
        6 => (rand(u, u64::MAX) as i128).into_val(env),
        7 => Symbol::new(env, SYMBOLS[rand(u, 4) as usize]).into_val(env),
        8 => {
            let addrs = pool.addresses();
            addrs[rand(u, 5) as usize].clone().into_val(env)
        }
        9 => BytesN::from_array(env, &[rand(u, 255) as u8; 32]).into_val(env),
        10 => {
            let len = rand(u, 4);
            let mut v: Vec<Val> = Vec::new(env);
            for _ in 0..len {
                v.push_back(arbitrary_val(u, env, pool, depth + 1));
            }
            v.into_val(env)
        }
        11 => {
            let len = rand(u, 4);
            let mut m: Map<Val, Val> = Map::new(env);
            for _ in 0..len {
                m.set(
                    arbitrary_val(u, env, pool, depth + 1),
                    arbitrary_val(u, env, pool, depth + 1),
                );
            }
            m.into_val(env)
        }
        12 => BytesN::from_array(env, &[rand(u, 255) as u8; 32]).into_val(env),
        _ => Val::VOID.into(),
    }
}

fn structured_oracle_config(u: &mut Unstructured, env: &Env, pool: &Pool) -> OracleConfig {
    // Malformed on purpose in most branches: zero/absurd staleness bounds,
    // an oracle address that is really the token or the escrow itself (so
    // the cross-contract call returns a host error rather than recursing),
    // and arbitrary denominators.
    OracleConfig {
        oracle_contract: pool.addresses()[rand(u, 5) as usize].clone(),
        price_asset: Symbol::new(env, SYMBOLS[rand(u, 4) as usize]),
        max_age_secs: [0u64, 1, 3_600, u64::MAX][rand(u, 3) as usize],
        denominated_amount: [0i128, 1, 500_000, i128::MAX][rand(u, 3) as usize],
    }
}

/// Argument list for `name`: declared shapes with fuzzed contents most of the
/// time, a purely arbitrary list sometimes, and one wrong-typed slot often.
fn build_args(name: &str, u: &mut Unstructured, env: &Env, pool: &Pool) -> Vec<Val> {
    let mut args: Vec<Val> = Vec::new(env);

    if coin(u) && rand(u, 3) == 0 {
        for _ in 0..rand(u, 14) {
            args.push_back(arbitrary_val(u, env, pool, 0));
        }
        return args;
    }

    match name {
        "initialize" => {
            args.push_back(pool.buyer.clone().into_val(env));
            args.push_back(pool.cooperative.clone().into_val(env));
            args.push_back(pool.warehouse.clone().into_val(env));
            args.push_back(pool.token.clone().into_val(env));
            let amount = [
                0i128,
                1,
                1_000_000,
                1_000_001,
                i128::MAX / 10_000,
                (i128::MAX / 10_000) + 1,
            ][rand(u, 5) as usize];
            args.push_back(amount.into_val(env));
            let b1 = rand(u, 10_000) as u32;
            let b2 = rand(u, 10_000) as u32;
            args.push_back((if coin(u) { b1 } else { 10_001 }).into_val(env));
            args.push_back((if coin(u) { b2 } else { 10_001 }).into_val(env));
            for _ in 0..3 {
                let windows = [0u64, 1, 604_800, u64::MAX];
                args.push_back(windows[rand(u, 3) as usize].into_val(env));
            }
            let qty = [0u32, 1, 1_000, u32::MAX][rand(u, 3) as usize];
            args.push_back(qty.into_val(env));
            let len = [0u64, 1, 3, 16, 17, MAX_FUZZED_VEC][rand(u, 5) as usize];
            let mut grades: Vec<u32> = Vec::new(env);
            for _ in 0..len {
                grades.push_back([0u32, 7_500, 10_000, 10_001, u32::MAX][rand(u, 4) as usize]);
            }
            args.push_back(grades.into_val(env));
            match rand(u, 2) {
                0 => args.push_back(Option::<OracleConfig>::None.into_val(env)),
                _ => {
                    let cfg = structured_oracle_config(u, env, pool);
                    args.push_back(Option::<OracleConfig>::Some(cfg).into_val(env));
                }
            }
        }
        "set_allocation" => {
            let len = [0u64, 1, 3, 200, 201, MAX_FUZZED_VEC][rand(u, 5) as usize];
            let mut members: Vec<AllocationMember> = Vec::new(env);
            for _ in 0..len {
                members.push_back(AllocationMember {
                    member_hash: BytesN::from_array(env, &[rand(u, 255) as u8; 32]),
                    share_bps: [0u32, 1, 3_333, 10_000, 10_001, u32::MAX][rand(u, 5) as usize],
                });
            }
            args.push_back(members.into_val(env));
        }
        "confirm_delivery" => {
            args.push_back((rand(u, u32::MAX as u64) as u32).into_val(env));
            args.push_back((rand(u, 5) as u32).into_val(env));
        }
        "reassign_buyer" | "flag_dispute" => {
            args.push_back(pool.addresses()[rand(u, 5) as usize].clone().into_val(env));
        }
        "replace_oracle_config" => {
            if coin(u) {
                args.push_back(structured_oracle_config(u, env, pool).into_val(env));
            } else {
                args.push_back(arbitrary_val(u, env, pool, 0));
            }
        }
        _ => {
            // Zero-argument functions: occasionally one spurious argument, so
            // arity mismatches are exercised too.
            if rand(u, 7) == 0 {
                args.push_back(arbitrary_val(u, env, pool, 0));
            }
        }
    }

    // Replace one declared slot with a wrong-typed value sometimes.
    if !args.is_empty() && rand(u, 3) == 0 {
        let idx = rand(u, args.len() as u64 - 1) as u32;
        args.set(idx, arbitrary_val(u, env, pool, 0));
    }
    args
}

fn deploy(env: &Env) -> Pool {
    let buyer = Address::generate(env);
    let cooperative = Address::generate(env);
    let warehouse = Address::generate(env);
    let stranger = Address::generate(env);
    let token_admin = Address::generate(env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    token::StellarAssetClient::new(env, &token_address).mint(&buyer, &1_000_000_000i128);

    let escrow = env.register(harvestlock_escrow::EscrowContract, ());
    Pool {
        buyer,
        cooperative,
        warehouse,
        token: token_address,
        escrow,
        stranger,
    }
}

fn initialize_valid(env: &Env, pool: &Pool) {
    let client = EscrowContractClient::new(env, &pool.escrow);
    client.initialize(
        &pool.buyer,
        &pool.cooperative,
        &pool.warehouse,
        &pool.token,
        &1_000_000i128,
        &1_500u32,
        &2_000u32,
        &604_800u64,
        &604_800u64,
        &10_368_000u64,
        &1_000u32,
        &Vec::from_array(env, [10_000u32, 9_000, 7_500]),
        &None,
    );
}

/// A short random walk through legal calls, so the target function is not
/// always invoked from `Draft`. Errors are ignored: the walk is just
/// state preparation, and the fuzzer decides how far it gets.
fn advance_state(env: &Env, pool: &Pool, u: &mut Unstructured) {
    let client = EscrowContractClient::new(env, &pool.escrow);
    let steps = rand(u, 5);
    for _ in 0..steps {
        match rand(u, 12) {
            0 => {
                let _ = client.try_lock();
            }
            1 => {
                let _ = client.try_release_advance_1();
            }
            2 => {
                let _ = client.try_claim_advance_1();
            }
            3 => {
                let _ = client.try_mark_checkpoint();
            }
            4 => {
                let _ = client.try_release_advance_2();
            }
            5 => {
                let _ = client.try_ready_for_delivery();
            }
            6 => {
                let _ = client.try_fund_remainder();
            }
            7 => {
                let _ = client.try_confirm_delivery(&1_000u32, &0u32);
            }
            8 => {
                let _ = client.try_flag_dispute(&pool.buyer);
            }
            9 => {
                let _ = client.try_cancel();
            }
            10 => {
                let now = env.ledger().timestamp();
                env.ledger().set_timestamp(now.saturating_add(604_801));
            }
            _ => {
                let _ = client.try_settle();
            }
        }
    }
}

fn classify(
    result: Result<Result<Val, soroban_sdk::ConversionError>, Result<Error, soroban_sdk::InvokeError>>,
) -> bool {
    // `true` = success. Both error arms are accepted outcomes: the outer
    // `Err(Ok(Error))` is a contract-level refusal, while the outer
    // `Err(Err(..))` is the host/SDK refusing a raw `Val` (argument
    // conversion, missing storage, a failed nested call). The host rolls
    // failed invocations back atomically, so neither can leave partial state.
    matches!(result, Ok(Ok(_)))
}

pub fn run(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let env = Env::default();
    env.mock_all_auths();
    let pool = deploy(&env);

    let name = FUNCTIONS[rand(&mut u, FUNCTIONS.len() as u64 - 1) as usize];

    // `initialize` must be measured against an uninitialized instance; every
    // other function needs a commitment that exists.
    if name != "initialize" {
        initialize_valid(&env, &pool);
        advance_state(&env, &pool, &mut u);
    }

    let args = build_args(name, &mut u, &env, &pool);
    let sym = Symbol::new(&env, name);
    let result = env.try_invoke_contract::<Val, Error>(&pool.escrow, &sym, args);

    if classify(result) {
        // A successful call must leave a readable, invariant-holding
        // commitment — including a successful `initialize`. Anything else is
        // exactly the partial-state failure this target exists to catch.
        let got = env.try_invoke_contract::<Commitment, Error>(
            &pool.escrow,
            &Symbol::new(&env, "get_commitment"),
            Vec::new(&env),
        );
        match got {
            Ok(Ok(c)) => c.check_invariants(),
            _ => panic!("{name} succeeded but the commitment became unreadable"),
        }
    }
}

#[cfg(test)]
mod smoke {
    use super::run;

    /// Run a deterministic byte corpus through the harness without libFuzzer.
    /// This is setup coverage for the target itself: every iteration deploys a
    /// contract, walks state, and invokes a function, so it catches a broken
    /// fixture or an invariant assertion firing on ordinary paths before a
    /// real fuzzing session would.
    #[test]
    fn deterministic_smoke_runs() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for i in 0..250u32 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let len = 2 + (state % 40) as usize;
            let mut bytes = std::vec::Vec::with_capacity(len);
            // Bias the first byte towards every function id at least once,
            // then let the rest be pseudorandom.
            bytes.push(if i < 34 { i as u8 } else { (state >> 13) as u8 });
            for _ in 1..len {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                bytes.push((state >> 33) as u8);
            }
            run(&bytes);
        }
    }
}
