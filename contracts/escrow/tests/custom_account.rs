//! #223 — the full lifecycle with a smart-contract account (2-of-3
//! ed25519 `__check_auth`) as the cooperative and separately as the buyer.
//!
//! Every other suite authenticates with generated **contract** addresses whose
//! auth is mocked (`mock_all_auths`) or staged (`mock_auths`), so the host
//! never actually invokes a `__check_auth` implementation. That leaves the
//! most likely real deployment shape — a cooperative shared wallet, or a
//! buyer treasury, that is itself a contract account — untested end to end.
//!
//! This suite deploys [`TwoOfThreeAccount`], a minimal test-only account
//! contract that:
//!
//! * stores three ed25519 signer public keys and a threshold (2 in the
//!   account-under-test, 1 for the other parties so their signatures do not
//!   obscure the behaviour being measured);
//! * verifies each supplied signature over the host-provided
//!   `signature_payload` with `env.crypto().ed25519_verify`;
//! * requires distinct known signers and at least `threshold` of them;
//! * rejects anything that is not a contract invocation context; and
//! * records the [`Context`] list the host handed it, so the suite can prove
//!   the account really received the full auth tree (`lock` is two contexts:
//!   `escrow.lock` plus the nested `token.transfer`, not just one).
//!
//! Auth entries are built and signed exactly the way a wallet would: the
//! `HashIdPreimage::SorobanAuthorization` preimage is XDR-encoded and hashed
//! (network id, nonce, expiration ledger and the whole invocation tree), then
//! signed with ed25519. Nothing is mocked; `env.set_auths` disables the test
//! env's auth mocking, so the escrow contract's `require_auth` calls really do
//! reach `__check_auth`.
//!
//! Covered paths:
//!
//! | account-under-test | methods |
//! |--------------------|---------|
//! | cooperative        | `initialize`, `set_allocation`, `lock`, `claim_advance_1/2`, `mark_checkpoint`, `ready_for_delivery`, `confirm_delivery`, `settle` |
//! | cooperative        | `cancel` (buyer + cooperative) |
//! | cooperative        | `reassign_buyer`, `flag_dispute`, `resolve_dispute` |
//! | buyer              | `initialize`, `lock`, `claim_advance_1/2`, `fund_remainder` (nested token pull), `confirm_delivery`, `settle` |
//! | buyer              | `cancel`, then confirm the below-threshold rejection and the 2-of-3 acceptance |
//!
//! The rejection cases are not simulated contract errors: the host aborts the
//! invocation because `__check_auth` returns `ThresholdNotMet`, and the suite
//! asserts `Err(Err(_))` plus unchanged state. The accept case then runs the
//! same call with the second signature.

use ed25519_dalek::{Signer, SigningKey};
use harvestlock_escrow::{
    AllocationMember, CommitmentTerms, EscrowContract, EscrowContractClient, OracleConfig, Status,
};
use soroban_sdk::auth::{Context, ContractContext, CustomAccountInterface};
use soroban_sdk::crypto::Hash;
use soroban_sdk::testutils::{Address as _, EnvTestConfig, MockAuthInvoke};
use soroban_sdk::xdr::{
    HashIdPreimage, HashIdPreimageSorobanAuthorization, Limits, ScVal, SorobanAddressCredentials,
    SorobanAuthorizationEntry, SorobanAuthorizedInvocation, SorobanCredentials, WriteXdr,
};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Bytes, BytesN, Env,
    IntoVal, TryFromVal, Val, Vec,
};

// ---------- test-only 2-of-3 account contract ----------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AccountError {
    NotInitialized = 1,
    UnknownSigner = 2,
    ThresholdNotMet = 3,
    UnsupportedContext = 4,
}

#[contracttype]
#[derive(Clone)]
pub enum AccountKey {
    Signers,
    Threshold,
    LastContexts,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApproval {
    pub signer: BytesN<32>,
    pub signature: BytesN<64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountApprovals {
    pub approvals: Vec<AccountApproval>,
}

/// A shared wallet: `threshold` distinct signers out of the `signers` list must
/// approve the exact host-computed payload. Test-only — it never handles funds
/// of its own, it only answers `__check_auth`.
#[contract]
pub struct TwoOfThreeAccount;

#[contractimpl]
impl TwoOfThreeAccount {
    pub fn __constructor(env: Env, signers: Vec<BytesN<32>>, threshold: u32) {
        env.storage().instance().set(&AccountKey::Signers, &signers);
        env.storage()
            .instance()
            .set(&AccountKey::Threshold, &threshold);
    }

    pub fn signers(env: Env) -> Vec<BytesN<32>> {
        env.storage()
            .instance()
            .get(&AccountKey::Signers)
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&AccountKey::Threshold)
            .unwrap_or(0)
    }

    /// The `Context` list from the most recent successful `__check_auth`.
    /// Read-only and auth-free; used only by this suite to prove the account
    /// received the whole tree, not just the outer call.
    pub fn last_contexts(env: Env) -> Vec<Context> {
        env.storage()
            .instance()
            .get(&AccountKey::LastContexts)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

#[contractimpl]
impl CustomAccountInterface for TwoOfThreeAccount {
    type Signature = AccountApprovals;
    type Error = AccountError;

    fn __check_auth(
        env: Env,
        signature_payload: Hash<32>,
        signatures: AccountApprovals,
        auth_contexts: Vec<Context>,
    ) -> Result<(), AccountError> {
        let signers: Vec<BytesN<32>> = env
            .storage()
            .instance()
            .get(&AccountKey::Signers)
            .ok_or(AccountError::NotInitialized)?;
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&AccountKey::Threshold)
            .ok_or(AccountError::NotInitialized)?;

        // A custom account is offered the full tree it is being asked to
        // authorize. This one only signs ordinary contract invocations; a
        // create-contract host function is refused outright rather than
        // silently approved.
        if auth_contexts.is_empty() {
            return Err(AccountError::UnsupportedContext);
        }
        for context in auth_contexts.iter() {
            match context {
                Context::Contract(_) => (),
                Context::CreateContractHostFn(_) | Context::CreateContractWithCtorHostFn(_) => {
                    return Err(AccountError::UnsupportedContext)
                }
            }
        }

        let payload: BytesN<32> = signature_payload.into();
        let mut approved: Vec<BytesN<32>> = Vec::new(&env);
        for approval in signatures.approvals.iter() {
            // Only configured signers count. Without this an arbitrary key
            // could supply its own valid signature and meet the threshold.
            if !signers.contains(&approval.signer) {
                return Err(AccountError::UnknownSigner);
            }
            // One signer cannot count twice by repeating its approval.
            if approved.contains(&approval.signer) {
                continue;
            }
            env.crypto().ed25519_verify(
                &approval.signer,
                &payload.clone().into(),
                &approval.signature,
            );
            approved.push_back(approval.signer);
        }

        if approved.len() < threshold {
            return Err(AccountError::ThresholdNotMet);
        }

        env.storage()
            .instance()
            .set(&AccountKey::LastContexts, &auth_contexts);
        Ok(())
    }
}

// ---------- wallet-side harness ----------

/// One on-chain party: its deployed account address, the off-chain signing
/// keys, and the threshold encoded in the account contract.
struct Party {
    account: Address,
    keys: std::vec::Vec<SigningKey>,
    threshold: u32,
}

impl Party {
    fn deploy(env: &Env, seed_base: u8, count: u8, threshold: u32) -> Self {
        let keys: std::vec::Vec<SigningKey> = (0..count)
            .map(|i| SigningKey::from_bytes(&[seed_base + i; 32]))
            .collect();
        let mut signers: Vec<BytesN<32>> = Vec::new(env);
        for key in &keys {
            signers.push_back(BytesN::from_array(env, &key.verifying_key().to_bytes()));
        }
        let account = env.register(TwoOfThreeAccount, (signers, threshold));
        Self {
            account,
            keys,
            threshold,
        }
    }

    fn client<'a>(&self, env: &'a Env) -> TwoOfThreeAccountClient<'a> {
        TwoOfThreeAccountClient::new(env, &self.account)
    }
}

/// Builds the authorization entry a real wallet would attach: hashes the
/// `HashIdPreimage::SorobanAuthorization` preimage (network id, nonce,
/// expiration ledger, full invocation tree) and signs it with the first
/// `approvals` keys. Passing fewer keys than the account's threshold is how
/// the rejection cases deliberately under-sign.
fn authorization_entry(
    env: &Env,
    party: &Party,
    invoke: &MockAuthInvoke,
    nonce: i64,
    approvals: u32,
) -> SorobanAuthorizationEntry {
    let keys: std::vec::Vec<&SigningKey> = party.keys.iter().take(approvals as usize).collect();
    authorization_entry_with_keys(env, &party.account, invoke, nonce, &keys)
}

/// The general form: any mix of keys signs the same host payload, which lets
/// the negative test include a key the account does not recognise.
fn authorization_entry_with_keys(
    env: &Env,
    account: &Address,
    invoke: &MockAuthInvoke,
    nonce: i64,
    keys: &[&SigningKey],
) -> SorobanAuthorizationEntry {
    let invocation = SorobanAuthorizedInvocation::from(invoke);
    let expiration = env.ledger().sequence() + env.storage().max_ttl();
    let preimage = HashIdPreimage::SorobanAuthorization(HashIdPreimageSorobanAuthorization {
        network_id: soroban_sdk::xdr::Hash(env.ledger().network_id().to_array()),
        nonce,
        signature_expiration_ledger: expiration,
        invocation: invocation.clone(),
    });
    let preimage_xdr = preimage
        .to_xdr(Limits::none())
        .expect("the auth preimage must encode");
    let payload = env.crypto().sha256(&Bytes::from_slice(env, &preimage_xdr));

    let mut signed: Vec<AccountApproval> = Vec::new(env);
    for key in keys {
        signed.push_back(AccountApproval {
            signer: BytesN::from_array(env, &key.verifying_key().to_bytes()),
            signature: BytesN::from_array(env, &key.sign(&payload.to_array()).to_bytes()),
        });
    }
    let signature_val: Val = AccountApprovals { approvals: signed }.into_val(env);
    let signature = ScVal::try_from_val(env, &signature_val).expect("signature must convert");

    SorobanAuthorizationEntry {
        credentials: SorobanCredentials::Address(SorobanAddressCredentials {
            address: account
                .clone()
                .try_into()
                .expect("a contract address must convert to ScAddress"),
            nonce,
            signature_expiration_ledger: expiration,
            signature,
        }),
        root_invocation: invocation,
    }
}

struct Scene {
    env: Env,
    contract_id: Address,
    contract: EscrowContractClient<'static>,
    token: token::Client<'static>,
    buyer: Party,
    cooperative: Party,
    warehouse: Party,
    nonce: std::cell::Cell<i64>,
}

impl Scene {
    /// Deploys all three parties as account contracts. `buyer_threshold` and
    /// `cooperative_threshold` let each test put the 2-of-3 policy on the
    /// party under test while the others use a simple 1-of-1 wallet.
    fn new(buyer_threshold: u32, cooperative_threshold: u32) -> Self {
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        // Only used to mint and register; every escrow `require_auth` below
        // runs after `set_auths`, which disables this mocking.
        env.mock_all_auths();

        let buyer = Party::deploy(&env, 1, 3, buyer_threshold);
        let cooperative = Party::deploy(&env, 11, 3, cooperative_threshold);
        let warehouse = Party::deploy(&env, 21, 1, 1);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token_address = sac.address();
        let token = token::Client::new(&env, &token_address);
        // Exactly enough for deposit + remainder; FX top-ups are not used
        // here (the oracle path has its own suites).
        token::StellarAssetClient::new(&env, &token_address).mint(&buyer.account, &TOTAL_AMOUNT);

        let contract_id = env.register(EscrowContract, ());
        let contract = EscrowContractClient::new(&env, &contract_id);

        Self {
            env,
            contract_id,
            contract,
            token,
            buyer,
            cooperative,
            warehouse,
            nonce: std::cell::Cell::new(1),
        }
    }

    fn next_nonce(&self) -> i64 {
        let nonce = self.nonce.get();
        self.nonce.set(nonce + 1);
        nonce
    }

    fn entry(
        &self,
        party: &Party,
        fn_name: &str,
        args: Vec<Val>,
        sub_invokes: &[MockAuthInvoke],
        approvals: u32,
    ) -> SorobanAuthorizationEntry {
        let invoke = MockAuthInvoke {
            contract: &self.contract_id,
            fn_name,
            args,
            sub_invokes,
        };
        authorization_entry(&self.env, party, &invoke, self.next_nonce(), approvals)
    }

    fn terms(&self) -> CommitmentTerms {
        CommitmentTerms {
            total_amount: TOTAL_AMOUNT,
            advance1_bps: 1_500,
            advance2_bps: 1_500,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: Vec::from_array(&self.env, GRADE_PRICE_BPS),
        }
    }

    /// `initialize` with a 2-of-3 buyer. `approvals` is parameterised so the
    /// same method can build both the accepted and the rejected entry.
    fn initialize(&self, approvals: u32) {
        let terms = self.terms();
        let args = (
            self.buyer.account.clone(),
            self.cooperative.account.clone(),
            self.warehouse.account.clone(),
            self.token.address.clone(),
            terms.clone(),
            Option::<OracleConfig>::None,
        )
            .into_val(&self.env);
        let entry = self.entry(&self.buyer, "initialize", args, &[], approvals);
        self.env.set_auths(&[entry]);
        self.contract.initialize(
            &self.buyer.account,
            &self.cooperative.account,
            &self.warehouse.account,
            &self.token.address,
            &terms,
            &None,
        );
    }

    fn set_allocation(&self, members: &Vec<AllocationMember>, approvals: u32) {
        let entry = self.entry(
            &self.cooperative,
            "set_allocation",
            (members.clone(),).into_val(&self.env),
            &[],
            approvals,
        );
        self.env.set_auths(&[entry]);
        self.contract.set_allocation(members);
    }

    fn lock(&self, approvals: u32) {
        let deposit = deposit_amount();
        let transfer = MockAuthInvoke {
            contract: &self.token.address,
            fn_name: "transfer",
            args: (
                self.buyer.account.clone(),
                self.contract_id.clone(),
                deposit,
            )
                .into_val(&self.env),
            sub_invokes: &[],
        };
        let entry = self.entry(
            &self.buyer,
            "lock",
            Vec::new(&self.env),
            &[transfer],
            approvals,
        );
        self.env.set_auths(&[entry]);
        self.contract.lock();
    }

    fn release_advance_1(&self) {
        self.env.set_auths(&[]);
        self.contract.release_advance_1();
    }

    fn claim_advance_1(&self) {
        let entry = self.entry(
            &self.cooperative,
            "claim_advance_1",
            Vec::new(&self.env),
            &[],
            self.cooperative.threshold,
        );
        self.env.set_auths(&[entry]);
        self.contract.claim_advance_1();
    }

    fn mark_checkpoint(&self) {
        let entry = self.entry(
            &self.warehouse,
            "mark_checkpoint",
            Vec::new(&self.env),
            &[],
            self.warehouse.threshold,
        );
        self.env.set_auths(&[entry]);
        self.contract.mark_checkpoint();
    }

    fn release_advance_2(&self) {
        self.env.set_auths(&[]);
        self.contract.release_advance_2();
    }

    fn claim_advance_2(&self) {
        let entry = self.entry(
            &self.cooperative,
            "claim_advance_2",
            Vec::new(&self.env),
            &[],
            self.cooperative.threshold,
        );
        self.env.set_auths(&[entry]);
        self.contract.claim_advance_2();
    }

    fn ready_for_delivery(&self) {
        let entry = self.entry(
            &self.cooperative,
            "ready_for_delivery",
            Vec::new(&self.env),
            &[],
            self.cooperative.threshold,
        );
        self.env.set_auths(&[entry]);
        self.contract.ready_for_delivery();
    }

    fn fund_remainder(&self, approvals: u32) {
        let remainder = TOTAL_AMOUNT - deposit_amount();
        let transfer = MockAuthInvoke {
            contract: &self.token.address,
            fn_name: "transfer",
            args: (
                self.buyer.account.clone(),
                self.contract_id.clone(),
                remainder,
            )
                .into_val(&self.env),
            sub_invokes: &[],
        };
        let entry = self.entry(
            &self.buyer,
            "fund_remainder",
            Vec::new(&self.env),
            &[transfer],
            approvals,
        );
        self.env.set_auths(&[entry]);
        self.contract.fund_remainder();
    }

    fn confirm_delivery(&self) {
        let entry = self.entry(
            &self.warehouse,
            "confirm_delivery",
            (CONTRACTED_QUANTITY, 0u32).into_val(&self.env),
            &[],
            self.warehouse.threshold,
        );
        self.env.set_auths(&[entry]);
        self.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    }

    /// `cancel` is the two-party method: buyer and cooperative each have their
    /// own auth entry for the same invocation. `coop_approvals` controls the
    /// cooperative entry only, so the test can under-sign exactly one party.
    fn cancel(&self, buyer_approvals: u32, coop_approvals: u32) {
        let args = Vec::new(&self.env);
        let buyer = self.entry(&self.buyer, "cancel", args.clone(), &[], buyer_approvals);
        let coop = self.entry(&self.cooperative, "cancel", args, &[], coop_approvals);
        self.env.set_auths(&[buyer, coop]);
        self.contract.cancel();
    }

    fn reassign_buyer(&self, new_buyer: &Party) {
        let args: Vec<Val> = (new_buyer.account.clone(),).into_val(&self.env);
        let buyer = self.entry(&self.buyer, "reassign_buyer", args.clone(), &[], 3);
        let coop = self.entry(
            &self.cooperative,
            "reassign_buyer",
            args.clone(),
            &[],
            self.cooperative.threshold,
        );
        let new = self.entry(new_buyer, "reassign_buyer", args, &[], new_buyer.threshold);
        self.env.set_auths(&[buyer, coop, new]);
        self.contract.reassign_buyer(&new_buyer.account);
    }

    fn flag_dispute(&self, flagger: &Party) {
        let entry = self.entry(
            flagger,
            "flag_dispute",
            (flagger.account.clone(),).into_val(&self.env),
            &[],
            flagger.threshold,
        );
        self.env.set_auths(&[entry]);
        self.contract.flag_dispute(&flagger.account);
    }

    fn resolve_dispute_as(&self, buyer: &Party) {
        let args = Vec::new(&self.env);
        let buyer = self.entry(buyer, "resolve_dispute", args.clone(), &[], buyer.threshold);
        let coop = self.entry(
            &self.cooperative,
            "resolve_dispute",
            args.clone(),
            &[],
            self.cooperative.threshold,
        );
        let warehouse = self.entry(
            &self.warehouse,
            "resolve_dispute",
            args,
            &[],
            self.warehouse.threshold,
        );
        self.env.set_auths(&[buyer, coop, warehouse]);
        self.contract.resolve_dispute();
    }

    fn settle(&self) {
        self.env.set_auths(&[]);
        self.contract.settle();
    }
}

// ---------- shared constants ----------

const TOTAL_AMOUNT: i128 = 1_000_000;
const WINDOW: u64 = 7 * 24 * 60 * 60;
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
const CONTRACTED_QUANTITY: u32 = 1_000;
const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];

/// `Context` deliberately does not implement `PartialEq` in the SDK, so the
/// suite compares the pieces the escrow contract promised: which contract,
/// which function, and the exact arguments the host reported.
fn assert_context(
    context: &Context,
    expected_contract: &Address,
    expected_fn_name: &str,
    expected_args: Vec<Val>,
    env: &Env,
) {
    let Context::Contract(ContractContext {
        contract,
        fn_name,
        args,
    }) = context
    else {
        panic!("expected a contract invocation context, got {context:?}");
    };
    assert_eq!(contract, expected_contract);
    assert_eq!(fn_name, &soroban_sdk::Symbol::new(env, expected_fn_name));
    assert_eq!(args, &expected_args);
}

fn deposit_amount() -> i128 {
    TOTAL_AMOUNT * 3_000 / 10_000
}

fn remainder_amount() -> i128 {
    TOTAL_AMOUNT - deposit_amount()
}

fn allocation(env: &Env, count: usize) -> Vec<AllocationMember> {
    let mut members = Vec::new(env);
    for i in 0..count {
        let share = if count == 0 {
            0
        } else {
            (10_000 / count as u32) as u32
        };
        members.push_back(AllocationMember {
            member_hash: BytesN::from_array(env, &[i as u8; 32]),
            share_bps: share,
        });
    }
    members
}

// ---------- cooperative account under test ----------

/// The cooperative is a 2-of-3 shared wallet and drives every cooperative
/// action through `__check_auth`, including the multi-party `resolve_dispute`
/// and the two-party `cancel`. Ends in `Settled`, so `settle` (public) and the
/// payout path are covered too.
#[test]
fn two_of_three_cooperative_completes_the_lifecycle() {
    let s = Scene::new(1, 2);
    s.initialize(1);

    let members = allocation(&s.env, 2);
    s.set_allocation(&members, 2);
    assert_eq!(s.contract.get_allocation(), members);

    s.lock(1);
    s.release_advance_1();

    // `lock` asked the buyer (not the cooperative) to auth, so the account
    // under test has not seen a context yet. After the first cooperative
    // claim it must have seen exactly `claim_advance_1`.
    s.claim_advance_1();
    let contexts = s.cooperative.client(&s.env).last_contexts();
    assert_eq!(contexts.len(), 1, "one require_auth call, one context");
    assert_context(
        &contexts.get(0).unwrap(),
        &s.contract_id,
        "claim_advance_1",
        Vec::new(&s.env),
        &s.env,
    );

    s.mark_checkpoint();
    s.release_advance_2();
    s.claim_advance_2();
    s.ready_for_delivery();
    s.fund_remainder(1);
    s.confirm_delivery();
    s.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(
        s.token.balance(&s.cooperative.account),
        2 * (TOTAL_AMOUNT * 1_500 / 10_000) + remainder_amount()
    );
    assert_eq!(s.token.balance(&s.buyer.account), 0);
}

/// The cooperative's single-party `set_allocation`: one of two required
/// signatures is rejected with no allocation recorded; the second signature
/// then records it. This is the cooperative-side single-party accept/reject
/// pair the issue asks for, complementing the buyer's `lock` pair below.
#[test]
fn two_of_three_cooperative_set_allocation_rejects_below_threshold_then_accepts() {
    let s = Scene::new(1, 2);
    s.initialize(1);
    let members = allocation(&s.env, 2);

    let entry = s.entry(
        &s.cooperative,
        "set_allocation",
        (members.clone(),).into_val(&s.env),
        &[],
        1,
    );
    s.env.set_auths(&[entry]);
    let rejected = s.contract.try_set_allocation(&members);
    assert!(
        matches!(rejected, Err(Err(_))),
        "one of two cooperative signatures must be rejected, got {rejected:?}"
    );
    assert!(
        s.contract.try_get_allocation().is_err(),
        "a rejected auth must not record the allocation"
    );

    s.set_allocation(&members, 2);
    assert_eq!(s.contract.get_allocation(), members);
}

/// Two-party `cancel`, rejected first with one cooperative signature and then
/// accepted with two. The buyer's 1-of-1 signature is valid in both attempts,
/// so the failure is unambiguously the cooperative's threshold.
#[test]
fn two_of_three_cooperative_cancel_rejects_below_threshold_then_accepts() {
    let s = Scene::new(1, 2);
    s.initialize(1);
    s.lock(1);

    let before = s.contract.get_commitment();
    assert_eq!(before.status, Status::Locked);

    // Install a fresh set where only one cooperative signature is present,
    // then call through `try_` so a host auth abort is observable.
    let args = Vec::new(&s.env);
    let buyer = s.entry(&s.buyer, "cancel", args.clone(), &[], 1);
    let coop = s.entry(&s.cooperative, "cancel", args, &[], 1);
    s.env.set_auths(&[buyer, coop]);
    let below_threshold = s.contract.try_cancel();
    assert!(
        matches!(below_threshold, Err(Err(_))),
        "one of two cooperative signatures must abort the host auth check, got {below_threshold:?}"
    );
    let after = s.contract.get_commitment();
    assert_eq!(
        after.status,
        Status::Locked,
        "a rejected auth must roll back"
    );
    assert_eq!(
        s.token.balance(&s.buyer.account),
        TOTAL_AMOUNT - deposit_amount()
    );
    assert_eq!(
        s.token.balance(&s.contract_id),
        deposit_amount(),
        "the escrow still holds the deposit"
    );

    // Same call, second cooperative signature present.
    s.cancel(1, 2);
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.buyer.account), TOTAL_AMOUNT);
    assert_eq!(s.token.balance(&s.contract_id), 0);
}

/// `reassign_buyer` (three parties) and the full dispute round-trip, all with
/// the cooperative account's 2-of-3 policy. `resolve_dispute` is the widest
/// auth tree in the contract; if a custom account were going to be handed an
/// incomplete context list, this is where it would show.
#[test]
fn two_of_three_cooperative_reassigns_and_resolves_a_dispute() {
    let s = Scene::new(1, 2);
    s.initialize(1);
    s.lock(1);

    let replacement = Party::deploy(&s.env, 31, 1, 1);
    s.reassign_buyer(&replacement);
    assert_eq!(s.contract.get_commitment().buyer, replacement.account);

    s.flag_dispute(&s.cooperative);
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // The buyer is now the replacement, but this test still signs with the
    // original buyer key: prove the stale signer is rejected rather than
    // silently accepted because it was once the buyer.
    let stale = s.entry(&s.buyer, "resolve_dispute", Vec::new(&s.env), &[], 3);
    let coop = s.entry(&s.cooperative, "resolve_dispute", Vec::new(&s.env), &[], 2);
    let warehouse = s.entry(&s.warehouse, "resolve_dispute", Vec::new(&s.env), &[], 1);
    s.env.set_auths(&[stale, coop, warehouse]);
    let stale_result = s.contract.try_resolve_dispute();
    assert!(
        matches!(stale_result, Err(Err(_))),
        "the previous buyer's auth entry must not satisfy the new buyer's require_auth, got {stale_result:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Disputed);

    // Correct signer set: replacement buyer (1-of-1), cooperative (2-of-3),
    // warehouse (1-of-1).
    s.resolve_dispute_as(&replacement);
    assert_eq!(s.contract.get_status(), Status::Locked);
}

// ---------- buyer account under test ----------

/// The buyer is a 2-of-3 treasury. `initialize`, `lock` and `fund_remainder`
/// are the buyer's auth points; the first two of those are single-party
/// methods, and `lock`/`fund_remainder` additionally pull tokens, so the
/// nested `token.transfer` context is part of the buyer's auth tree. The
/// suite asserts the account received both contexts.
#[test]
fn two_of_three_buyer_completes_the_lifecycle() {
    let s = Scene::new(2, 1);
    s.initialize(2);

    s.lock(2);
    let contexts = s.buyer.client(&s.env).last_contexts();
    assert_eq!(
        contexts.len(),
        2,
        "buyer auth covers escrow.lock and the nested token.transfer"
    );
    assert_context(
        &contexts.get(0).unwrap(),
        &s.contract_id,
        "lock",
        Vec::new(&s.env),
        &s.env,
    );
    assert_context(
        &contexts.get(1).unwrap(),
        &s.token.address,
        "transfer",
        (
            s.buyer.account.clone(),
            s.contract_id.clone(),
            deposit_amount(),
        )
            .into_val(&s.env),
        &s.env,
    );

    s.release_advance_1();
    s.claim_advance_1();
    s.mark_checkpoint();
    s.release_advance_2();
    s.claim_advance_2();
    s.ready_for_delivery();
    s.fund_remainder(2);
    s.confirm_delivery();
    s.settle();

    assert_eq!(s.contract.get_status(), Status::Settled);
    assert_eq!(s.token.balance(&s.buyer.account), 0);
    assert_eq!(
        s.token.balance(&s.cooperative.account),
        2 * (TOTAL_AMOUNT * 1_500 / 10_000) + remainder_amount()
    );
}

/// The buyer's single-party `lock` with one of three signatures is rejected
/// before any tokens move; the same call with two signatures succeeds and the
/// deposit lands in escrow. This is the buyer-side threshold case the issue
/// calls for, and it also proves no partial token pull survives a rejected
/// `__check_auth`.
#[test]
fn two_of_three_buyer_lock_rejects_below_threshold_then_accepts() {
    let s = Scene::new(2, 1);
    s.initialize(2);

    let deposit = deposit_amount();
    let transfer = MockAuthInvoke {
        contract: &s.token.address,
        fn_name: "transfer",
        args: (s.buyer.account.clone(), s.contract_id.clone(), deposit).into_val(&s.env),
        sub_invokes: &[],
    };
    let entry = s.entry(&s.buyer, "lock", Vec::new(&s.env), &[transfer], 1);
    s.env.set_auths(&[entry]);
    let rejected = s.contract.try_lock();
    assert!(
        matches!(rejected, Err(Err(_))),
        "one of three buyer signatures must be rejected, got {rejected:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Draft);
    assert_eq!(s.token.balance(&s.buyer.account), TOTAL_AMOUNT);
    assert_eq!(s.token.balance(&s.contract_id), 0);

    s.lock(2);
    assert_eq!(s.contract.get_status(), Status::Locked);
    assert_eq!(s.token.balance(&s.buyer.account), TOTAL_AMOUNT - deposit);
    assert_eq!(s.token.balance(&s.contract_id), deposit);
}

/// Buyer-side cancel: the same two-party method as the cooperative test, but
/// with the 2-of-3 policy on the buyer instead. One buyer signature is
/// rejected; two succeed.
#[test]
fn two_of_three_buyer_cancel_rejects_below_threshold_then_accepts() {
    let s = Scene::new(2, 1);
    s.initialize(2);
    s.lock(2);

    let args = Vec::new(&s.env);
    let buyer = s.entry(&s.buyer, "cancel", args.clone(), &[], 1);
    let coop = s.entry(&s.cooperative, "cancel", args, &[], 1);
    s.env.set_auths(&[buyer, coop]);
    let rejected = s.contract.try_cancel();
    assert!(
        matches!(rejected, Err(Err(_))),
        "one of three buyer signatures must be rejected, got {rejected:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Locked);

    s.cancel(2, 1);
    assert_eq!(s.contract.get_status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.buyer.account), TOTAL_AMOUNT);
}

/// Unknown keys must not count toward the threshold even when they produce a
/// cryptographically valid signature over the same payload. The account checks
/// membership before verifying, so an attacker's own keypair cannot stand in
/// for a missing signer.
#[test]
fn unknown_signer_is_rejected_even_with_a_valid_signature() {
    let s = Scene::new(2, 1);
    s.initialize(2);

    let outsider = Party::deploy(&s.env, 41, 1, 1);
    let invoke = MockAuthInvoke {
        contract: &s.contract_id,
        fn_name: "lock",
        args: Vec::new(&s.env),
        sub_invokes: &[],
    };
    // One configured signer plus one outsider, both signing the real payload:
    // two valid signatures, but only one known signer, so the threshold is not
    // met and the outsider must not be treated as the second approval.
    let forged = authorization_entry_with_keys(
        &s.env,
        &s.buyer.account,
        &invoke,
        s.next_nonce(),
        &[&s.buyer.keys[0], &outsider.keys[0]],
    );
    s.env.set_auths(&[forged]);
    let rejected = s.contract.try_lock();
    assert!(
        matches!(rejected, Err(Err(_))),
        "an unconfigured signer must not count toward the 2-of-3 threshold, got {rejected:?}"
    );
    assert_eq!(s.contract.get_status(), Status::Draft);
    assert_eq!(s.token.balance(&s.buyer.account), TOTAL_AMOUNT);
}
