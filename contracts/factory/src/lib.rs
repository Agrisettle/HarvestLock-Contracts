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
//!
//! ## Why the escrow types are restated here
//!
//! Linking the `harvestlock-escrow` crate into this cdylib would also link
//! every exported escrow entry point, so the factory WASM would itself expose
//! `initialize`, `lock`, `settle`, … as an uninitialized escrow. The factory
//! therefore talks to the escrow through its own `#[contractclient]` and
//! restates the two argument types it forwards. `CommitmentTerms` and
//! `OracleConfig` here must stay field-for-field identical to the escrow's;
//! the tests encode both to XDR and compare the bytes.

#![no_std]

use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, Address, BytesN, ContractExecutable, Env,
    Symbol, Vec,
};

/// Same fields, order and types as `harvestlock_escrow::CommitmentTerms`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentTerms {
    pub total_amount: i128,
    pub advance1_bps: u32,
    pub advance2_bps: u32,
    pub claim_window_secs: u64,
    pub remainder_window_secs: u64,
    pub delivery_window_secs: u64,
    pub contracted_quantity: u32,
    pub grade_price_bps: Vec<u32>,
}

/// Same fields, order and types as `harvestlock_escrow::OracleConfig`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleConfig {
    pub oracle_contract: Address,
    pub price_asset: Symbol,
    pub max_age_secs: u64,
    pub denominated_amount: i128,
}

/// The one escrow entry point the factory calls. A failing `initialize`
/// aborts the whole invocation, so the deployment is rolled back with it.
#[allow(dead_code)]
#[contractclient(name = "EscrowClient")]
trait Escrow {
    fn initialize(
        env: Env,
        buyer: Address,
        cooperative: Address,
        warehouse_operator: Address,
        token: Address,
        terms: CommitmentTerms,
        oracle_config: Option<OracleConfig>,
    );
}

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
    /// that function's documentation for their semantics. Auth: none of its
    /// own; `initialize` requires the buyer's.
    #[allow(clippy::too_many_arguments)]
    pub fn deploy_and_initialize(
        env: Env,
        wasm_hash: BytesN<32>,
        salt: BytesN<32>,
        buyer: Address,
        cooperative: Address,
        warehouse_operator: Address,
        token: Address,
        terms: CommitmentTerms,
        oracle_config: Option<OracleConfig>,
    ) -> Address {
        let contract_address = env
            .deployer()
            .with_current_contract(salt)
            .deploy_contract(ContractExecutable::Wasm(wasm_hash), ());

        EscrowClient::new(&env, &contract_address).initialize(
            &buyer,
            &cooperative,
            &warehouse_operator,
            &token,
            &terms,
            &oracle_config,
        );

        contract_address
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use harvestlock_escrow::{EscrowContract, EscrowContractClient};
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::xdr::{Limits, ScVal, WriteXdr};
    use soroban_sdk::{token, IntoVal, TryFromVal, Val};

    fn terms(env: &Env) -> CommitmentTerms {
        CommitmentTerms {
            total_amount: 1_000_000,
            advance1_bps: 1_500,
            advance2_bps: 2_000,
            claim_window_secs: 7 * 24 * 60 * 60,
            remainder_window_secs: 7 * 24 * 60 * 60,
            delivery_window_secs: 120 * 24 * 60 * 60,
            contracted_quantity: 1_000,
            grade_price_bps: Vec::from_array(env, [10_000u32, 9_000, 7_500]),
        }
    }

    fn escrow_terms(env: &Env) -> harvestlock_escrow::CommitmentTerms {
        let t = terms(env);
        harvestlock_escrow::CommitmentTerms {
            total_amount: t.total_amount,
            advance1_bps: t.advance1_bps,
            advance2_bps: t.advance2_bps,
            claim_window_secs: t.claim_window_secs,
            remainder_window_secs: t.remainder_window_secs,
            delivery_window_secs: t.delivery_window_secs,
            contracted_quantity: t.contracted_quantity,
            grade_price_bps: t.grade_price_bps,
        }
    }

    fn xdr<T: IntoVal<Env, Val>>(env: &Env, v: &T) -> std::vec::Vec<u8> {
        let val: Val = v.into_val(env);
        ScVal::try_from_val(env, &val)
            .unwrap()
            .to_xdr(Limits::none())
            .unwrap()
    }

    #[test]
    fn factory_can_be_instantiated() {
        let env = Env::default();
        env.mock_all_auths();
        // Smoke test: the factory compiles and can be registered.
        let _factory_id = env.register(FactoryContract, ());
    }

    /// The restated types must encode exactly like the escrow's, or
    /// `initialize` would reject what the factory forwards.
    #[test]
    fn restated_types_encode_identically_to_the_escrows() {
        let env = Env::default();
        assert_eq!(xdr(&env, &terms(&env)), xdr(&env, &escrow_terms(&env)));

        let oracle_contract = Address::generate(&env);
        let ours = OracleConfig {
            oracle_contract: oracle_contract.clone(),
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 300,
            denominated_amount: 500_000,
        };
        let theirs = harvestlock_escrow::OracleConfig {
            oracle_contract,
            price_asset: Symbol::new(&env, "NGN"),
            max_age_secs: 300,
            denominated_amount: 500_000,
        };
        assert_eq!(xdr(&env, &Some(ours)), xdr(&env, &Some(theirs)));
    }

    /// The forwarded arguments are exactly what the escrow's own
    /// `initialize` accepts (the old 3-call path, called directly).
    #[test]
    fn escrow_accepts_the_arguments_the_factory_forwards() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let cooperative = Address::generate(&env);
        let warehouse = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token_address = sac.address();
        token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &1_000_000i128);

        let escrow_id = env.register(EscrowContract, ());
        let escrow = EscrowContractClient::new(&env, &escrow_id);
        escrow.initialize(
            &buyer,
            &cooperative,
            &warehouse,
            &token_address,
            &escrow_terms(&env),
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

    /// `deploy_and_initialize` type-checks end to end; with an unregistered
    /// WASM hash the deploy itself fails, and nothing is left behind.
    #[test]
    fn deploy_with_an_unknown_wasm_hash_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let factory_id = env.register(FactoryContract, ());
        let factory = FactoryContractClient::new(&env, &factory_id);
        let token = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        let result = factory.try_deploy_and_initialize(
            &BytesN::from_array(&env, &[0u8; 32]),
            &BytesN::from_array(&env, &[1u8; 32]),
            &Address::generate(&env),
            &Address::generate(&env),
            &Address::generate(&env),
            &token,
            &terms(&env),
            &None,
        );
        assert!(
            result.is_err(),
            "expected runtime error for unregistered wasm_hash"
        );
    }
}
