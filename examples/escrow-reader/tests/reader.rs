//! Proves the supported integration pattern end to end (#276): the escrow is
//! registered from its **built WASM**, the reader from its own source, and
//! `get_status` / `get_commitment` are read across the contract boundary.
//!
//! Requires the escrow's WASM artifact, which is why this crate is its own
//! workspace:
//!
//! ```bash
//! # from the repo root
//! stellar contract build --package harvestlock-escrow
//! cargo test --manifest-path examples/escrow-reader/Cargo.toml
//! ```
//!
//! Registering the escrow from `escrow_wasm::WASM` rather than from an escrow
//! Rust type is the point of the test, not a shortcut: it deploys exactly the
//! artifact a real deployment uploads, so the spec this reader was compiled
//! against and the spec the "deployed" escrow serves are the same one.

use escrow_reader::escrow::Status;
use escrow_reader::{EscrowReader, EscrowReaderClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

/// A second import of the same WASM, for *driving* the escrow in this test.
/// The reader's own copy lives in `escrow_reader::escrow`; both are generated
/// from the one artifact, so the two sides can only agree.
mod escrow_wasm {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/harvestlock_escrow.wasm"
    );
}

const CLAIM_WINDOW: u64 = 7 * 24 * 60 * 60;
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
const CONTRACTED_QUANTITY: u32 = 1_000;
const TOTAL_AMOUNT: i128 = 1_000_000;

#[test]
fn reads_status_and_commitment_across_contracts() {
    let env = Env::default();
    env.mock_all_auths();

    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = sac.address();
    token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &TOTAL_AMOUNT);

    // The escrow under test: the built WASM, registered as a contract.
    let escrow_id = env.register(escrow_wasm::WASM, ());
    let escrow = escrow_wasm::Client::new(&env, &escrow_id);
    escrow.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &TOTAL_AMOUNT,
        &1_500,
        &1_500,
        &CLAIM_WINDOW,
        &REMAINDER_WINDOW,
        &DELIVERY_WINDOW,
        &CONTRACTED_QUANTITY,
        &Vec::from_array(&env, [10_000u32, 9_000, 7_500]),
        &None,
    );

    // The reader: this crate's own contract.
    let reader_id = env.register(EscrowReader, ());
    let reader = EscrowReaderClient::new(&env, &reader_id);

    // A unit-variant enum across the boundary.
    assert_eq!(reader.status_of(&escrow_id), Status::Draft);

    // A whole struct across the boundary.
    let commitment = reader.commitment_of(&escrow_id);
    assert_eq!(commitment.status, Status::Draft);
    assert_eq!(commitment.buyer, buyer);
    assert_eq!(commitment.cooperative, cooperative);
    assert_eq!(commitment.warehouse_operator, warehouse);
    assert_eq!(commitment.token, token_address);
    assert_eq!(commitment.total_amount, TOTAL_AMOUNT);

    // A real transition on the escrow is visible through the reader, which is
    // what makes this a cross-contract read rather than a cached snapshot.
    escrow.lock();
    assert_eq!(reader.status_of(&escrow_id), Status::Locked);
    assert_eq!(reader.commitment_of(&escrow_id).status, Status::Locked);
}
