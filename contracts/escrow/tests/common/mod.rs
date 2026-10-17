//! Shared fixtures/helpers for the issue-specific regression suites under
//! `contracts/escrow/tests/`.
//!
//! Deliberately kept in a subdirectory (`tests/common/mod.rs`) rather than
//! `tests/common.rs` so cargo does not treat it as its own test target, and
//! each suite pulls it in with `#[path = "common/mod.rs"] mod common;`.
//!
//! It mirrors the conventions of the in-crate `src/test.rs` harness (same
//! token minting, same default windows, same "advance the simulated clock"
//! helper) so the two suites read alike — but it is *not* a copy of the
//! hand-written unit tests: these files exercise the suites each of issues
//! #111/#122/#124/#125 asked for, and CI runs them as separate jobs.

#![allow(dead_code)]

use harvestlock_escrow::{EscrowContract, EscrowContractClient, Status};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, Env, Vec};

pub const WINDOW: u64 = 7 * 24 * 60 * 60;
pub const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
pub const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
pub const CONTRACTED_QUANTITY: u32 = 1_000;
pub const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];
pub const TOTAL_AMOUNT: i128 = 1_000_000;

pub struct World {
    pub env: Env,
    pub contract: EscrowContractClient<'static>,
    pub token: token::Client<'static>,
    pub buyer: Address,
    pub cooperative: Address,
    pub warehouse: Address,
    pub total_amount: i128,
}

/// A fresh, initialized (`Status::Draft`) commitment at ledger timestamp 0.
pub fn world(advance1_bps: u32, advance2_bps: u32) -> World {
    world_with_window(advance1_bps, advance2_bps, WINDOW)
}

pub fn world_with_window(advance1_bps: u32, advance2_bps: u32, window: u64) -> World {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    world_in(
        &env,
        buyer,
        cooperative,
        warehouse,
        advance1_bps,
        advance2_bps,
        window,
    )
}

/// The identity-agnostic half — tests that need a specific (including
/// degenerate, two-roles-one-address) set of named parties pass their own
/// addresses in, exactly like `src/test.rs`'s `setup_in`.
pub fn world_in(
    env: &Env,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    advance1_bps: u32,
    advance2_bps: u32,
    window: u64,
) -> World {
    let token_admin = Address::generate(env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    let token = token::Client::new(env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(env, &token_address);
    token_admin_client.mint(&buyer, &TOTAL_AMOUNT);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &TOTAL_AMOUNT,
        &advance1_bps,
        &advance2_bps,
        &window,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(env, GRADE_PRICE_BPS),
        &None,
    );

    World {
        env: env.clone(),
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        total_amount: TOTAL_AMOUNT,
    }
}

impl World {
    pub fn now(&self) -> u64 {
        self.env.ledger().timestamp()
    }

    pub fn set_time(&self, t: u64) {
        self.env.ledger().set_timestamp(t);
    }

    pub fn advance(&self, secs: u64) {
        let now = self.env.ledger().timestamp();
        self.env.ledger().set_timestamp(now + secs);
    }

    /// `total_amount * bps / 10_000`, mirroring the contract's internal
    /// `bps_amount` so money assertions read plainly.
    pub fn bps(&self, bps: u32) -> i128 {
        self.total_amount * i128::from(bps) / 10_000
    }

    /// Locked, tranche 1 released: `advance1_deadline` is now set.
    pub fn locked_tranche1_open(&self) -> u64 {
        self.contract.lock();
        self.contract.release_advance_1();
        self.contract.get_commitment().advance1_deadline
    }

    /// Locked, both tranches released, tranche 1 claimed and the checkpoint
    /// marked: `advance2_deadline` is now set.
    pub fn tranche2_open(&self) -> u64 {
        self.contract.lock();
        self.contract.release_advance_1();
        self.contract.mark_checkpoint();
        self.contract.release_advance_2();
        self.contract.get_commitment().advance2_deadline
    }

    /// Straight through to `ReadyForDelivery` with the remainder funded,
    /// both tranches claimed — the state `confirm_delivery`/`settle` need.
    pub fn drive_to_remainder_funded(&self) {
        self.contract.lock();
        self.contract.release_advance_1();
        self.contract.claim_advance_1();
        self.contract.mark_checkpoint();
        self.contract.release_advance_2();
        self.contract.claim_advance_2();
        self.contract.ready_for_delivery();
        self.contract.fund_remainder();
    }

    pub fn status(&self) -> Status {
        self.contract.get_status()
    }

    /// `Status::Delivered` with both tranches claimed and the remainder
    /// funded, at full quantity and full grade — the earliest state
    /// `resolve_fx_shortfall`/`settle` accept, and what the oracle suites
    /// need in order to reach the oracle read at all.
    pub fn drive_to_delivered(&self) {
        self.drive_to_remainder_funded();
        self.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    }
}

// Assertions on a `try_*` client call in these suites are written plainly as
// `assert_eq!(call, Err(Ok(Error::X)))` — the shape the generated client
// returns for a contract function whose own return type is a `Result` (outer
// `Err` carrying `Ok(contract Error)`, exactly as `src/test.rs` asserts),
// which keeps a failing assertion readable without a helper in between.
