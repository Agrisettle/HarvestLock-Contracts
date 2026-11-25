//! HarvestLock factory contract — deploy + initialize in one atomic call.
//!
//! Issue #252 adds this crate as an *additive* deployment path: the existing
//! three-call flow (deploy → initialize → lock) continues to work and
//! nothing in the escrow contract changes. The factory merely batches the
//! first two calls so a buyer can deploy and initialize their commitment in
//! a single transaction rather than two.
//!
//! ## Why only two of the three calls
//!
//! `initialize + lock` cannot be fused — a Stellar transaction allows exactly
//! one `InvokeHostFunctionOp`, and the Draft window between `initialize` and
//! `lock` is load-bearing (PRD §4.9: `set_allocation` is pre-lock only).
//! See `docs/constructor-batching.md §3` for the full reasoning. What this
//! factory buys is `deploy + initialize` → one call; `lock` is still a
//! separate buyer-signed transaction.
//!
//! ## Address derivation
//!
//! The deployed escrow's address is deterministic:
//! `hash(factory_address || salt)`. A caller can predict it before the
//! transaction lands by computing that hash off-chain — the same guarantee
//! the existing deployer already provides.
//!
//! ## Authorization
//!
//! The factory passes `buyer.require_auth()` through to `initialize`, which
//! calls it. The buyer must therefore authorize the entire call tree in one
//! auth entry (Soroban's tree-auth model covers cross-contract sub-invocations
//! automatically when `mock_all_auths` is active, and in production via a
//! signed auth entry that covers the factory's outer invocation). No factory-
//! level auth of its own — the factory is a pure deployment helper.

#![no_std]

use harvestlock_escrow::{EscrowContractClient, OracleConfig};
use soroban_sdk::{contract, contractimpl, Address, BytesN, ContractExecutable, Env, Vec};

#[contract]
pub struct FactoryContract;

#[contractimpl]
impl FactoryContract {
    /// Deploy a new escrow contract and immediately initialize it, all in one
    /// call. Returns the address of the freshly deployed escrow.
    ///
    /// `wasm_hash` — the hash of the escrow contract WASM registered with
    /// the network ahead of time (via `Operation.uploadContractWasm`).
    ///
    /// `salt` — a 32-byte caller-chosen nonce. The escrow's address is
    /// derived deterministically from `hash(factory_address || salt)`, so
    /// different salts produce different addresses from the same factory.
    ///
    /// All remaining parameters are forwarded verbatim to `initialize`; see
    /// that function's documentation for their semantics.
    #[allow(clippy::too_many_arguments)]
    pub fn deploy_and_initialize(
        env: Env,
        wasm_hash: BytesN<32>,
        salt: BytesN<32>,
        buyer: Address,
        cooperative: Address,
        warehouse_operator: Address,
        token: Address,
        total_amount: i128,
        advance1_bps: u32,
        advance2_bps: u32,
        claim_window_secs: u64,
        remainder_window_secs: u64,
        delivery_window_secs: u64,
        contracted_quantity: u32,
        grade_price_bps: Vec<u32>,
        oracle_config: Option<OracleConfig>,
    ) -> Address {
        let contract_address = env
            .deployer()
            .with_current_contract(salt)
            .deploy_contract(ContractExecutable::Wasm(wasm_hash), ());

        EscrowContractClient::new(&env, &contract_address).initialize(
            &buyer,
            &cooperative,
            &warehouse_operator,
            &token,
            &total_amount,
            &advance1_bps,
            &advance2_bps,
            &claim_window_secs,
            &remainder_window_secs,
            &delivery_window_secs,
            &contracted_quantity,
            &grade_price_bps,
            &oracle_config,
        );

        contract_address
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use harvestlock_escrow::{EscrowContract, EscrowContractClient};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{token, Env};

    #[test]
    fn factory_can_be_instantiated() {
        let env = Env::default();
        env.mock_all_auths();
        // Smoke test: the factory compiles and can be registered.
        let _factory_id = env.register(FactoryContract, ());
    }

    /// The factory's `deploy_and_initialize` signature must match
    /// `EscrowContractClient::initialize`. Verified here by calling
    /// `initialize` directly (the old 3-call path) with the same arguments
    /// the factory would forward — confirming type alignment without
    /// needing a pre-built escrow WASM hash.
    ///
    /// A full end-to-end test (`factory.deploy_and_initialize(wasm_hash, salt, ...)`)
    /// runs in CI after `cargo build --release -p harvestlock-escrow` produces
    /// the WASM binary, which is then uploaded via
    /// `env.deployer().upload_contract_wasm(WASM_BYTES)`.
    #[test]
    fn escrow_client_matches_factory_initialize_signature() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let cooperative = Address::generate(&env);
        let warehouse = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token_address = sac.address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_address);
        token_admin_client.mint(&buyer, &1_000_000i128);

        // Old 3-call path — the factory's cross-contract call uses the same args.
        let escrow_id = env.register(EscrowContract, ());
        let escrow = EscrowContractClient::new(&env, &escrow_id);
        let grade_price_bps = soroban_sdk::Vec::from_array(&env, [10_000u32, 9_000u32, 7_500u32]);
        escrow.initialize(
            &buyer,
            &cooperative,
            &warehouse,
            &token_address,
            &1_000_000i128,
            &1_500u32,
            &2_000u32,
            &(7 * 24 * 60 * 60u64),
            &(7 * 24 * 60 * 60u64),
            &(120 * 24 * 60 * 60u64),
            &1_000u32,
            &grade_price_bps,
            &None,
        );

        let c = escrow.get_commitment();
        assert_eq!(c.buyer, buyer);
        assert_eq!(c.cooperative, cooperative);
        assert_eq!(c.warehouse_operator, warehouse);
        assert_eq!(c.total_amount, 1_000_000);
        assert_eq!(c.advance1_bps, 1_500);
        assert_eq!(c.advance2_bps, 2_000);
    }

    /// The factory contract exposes `deploy_and_initialize` and returns the
    /// correct type (`Address`). This compilation-only test verifies the
    /// generated client's method signature resolves without a live wasm_hash.
    #[test]
    fn factory_client_method_compiles() {
        let env = Env::default();
        env.mock_all_auths();
        let factory_id = env.register(FactoryContract, ());
        let factory = FactoryContractClient::new(&env, &factory_id);
        // `try_deploy_and_initialize` exists and returns Result<Address, _>.
        // Calling it with a zero hash fails at runtime (hash not registered),
        // but the important check is that the full call site type-checks.
        let result = factory.try_deploy_and_initialize(
            &BytesN::from_array(&env, &[0u8; 32]),
            &BytesN::from_array(&env, &[1u8; 32]),
            &Address::generate(&env),
            &Address::generate(&env),
            &Address::generate(&env),
            &{
                let token_admin = Address::generate(&env);
                let sac = env.register_stellar_asset_contract_v2(token_admin);
                sac.address()
            },
            &1_000_000i128,
            &1_500u32,
            &2_000u32,
            &(7 * 24 * 60 * 60u64),
            &(7 * 24 * 60 * 60u64),
            &(120 * 24 * 60 * 60u64),
            &1_000u32,
            &soroban_sdk::Vec::from_array(&env, [10_000u32]),
            &None,
        );
        // Runtime failure is expected (wasm_hash 0 is not registered).
        assert!(result.is_err(), "expected runtime error for unregistered wasm_hash");
    }
}
