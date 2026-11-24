//! The one test harness — shared by the in-crate unit tests (`src/test.rs`)
//! and every integration suite under `contracts/escrow/tests/` (#274).
//!
//! Before this module existed there were two parallel copies of the same
//! fixtures: `src/test.rs` had `setup`/`setup_in`/`create_token`/
//! `advance_time`/`deposit_amount` and `tests/common/mod.rs` had
//! `world`/`world_in` with the same token minting, the same default windows
//! and the same grade schedule, documented as "mirroring the conventions" of
//! the other. Every new suite (event sourcing, budget, token edge cases) was
//! one more chance to pick a copy or add a third, and the constants — the
//! claim/remainder/delivery windows and the grade schedule — were already
//! stored twice, free to drift apart. There is now one copy of each.
//!
//! ## How both suites reach it
//!
//! * `src/test.rs` (the unit tests, compiled under `cfg(test)`) writes
//!   `use crate::testutils::*;`.
//! * The integration suites link this crate as a normal dependency, so they
//!   reach the same module as `harvestlock_escrow::testutils`.
//!   `tests/common/mod.rs` is a one-line re-export of it and nothing else.
//!
//! The declaration in `lib.rs` is
//! `#[cfg(any(test, feature = "testutils"))]`. Integration tests don't
//! compile the crate with `cfg(test)`, so they need the crate's `testutils`
//! feature; `Cargo.toml`'s self dev-dependency
//! (`harvestlock-escrow = { path = ".", features = ["testutils"] }`) enables
//! it for every test build, which is what keeps a plain `cargo test` working
//! with no extra flags — the `--features testutils` a reader might expect is
//! not needed. It is absent from the release WASM: neither cfg holds there.
//!
//! That self dev-dependency also unifies `soroban-sdk`'s own `testutils`
//! feature into the library build the integration tests link against, which
//! is what makes `Env::register`, `Address::generate` and `mock_all_auths`
//! available here in both builds.
//!
//! ## Terms are built, not repeated
//!
//! [`Terms`] is the builder every constructor funnels through. Its defaults
//! are exactly what the two old harnesses hard-coded — 7-day claim and
//! remainder windows, a 120-day delivery window, 1 000 contracted units, the
//! A/B/C `10 000 / 9 000 / 7 500` grade schedule, and 1 000 000 units of
//! settlement token — so `world(1_500, 1_500)` and
//! `world_with_terms(Terms::default().advances(1_500, 1_500))` build the same
//! commitment. A test that cares about one window overrides only that field.
//!
//! ## Names
//!
//! [`World`] is the harness struct; [`Setup`] is an alias for it, kept
//! because `src/test.rs`'s thousands of existing assertions predate the
//! unification and there is no behavior worth changing in renaming them.

use crate::{CommitmentTerms, EscrowContract, EscrowContractClient, Status};
use soroban_sdk::testutils::{Address as _, Ledger as _, MockAuthInvoke};
use soroban_sdk::{token, Address, Env, Val, Vec};

/// Default claim window: 7 days, arbitrary but realistic.
pub const WINDOW: u64 = 7 * 24 * 60 * 60;
/// Default remainder-payment window: 7 days — matches the app-level default.
pub const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
/// Default delivery window: 120 days — matches the app-level default.
pub const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
/// Default contracted quantity, in arbitrary "kg" units — the same framing as
/// `total_amount`'s stroops-equivalent units.
pub const CONTRACTED_QUANTITY: u32 = 1_000;
/// Full price at grade 0, then two discounted tiers — a plausible pre-agreed
/// grade schedule for tests that don't care about the exact numbers, just
/// that a schedule exists.
pub const GRADE_PRICE_BPS: [u32; 3] = [10_000, 9_000, 7_500];
/// How many grade entries [`Terms`] can carry. The contract itself accepts
/// more (`MAX_GRADE_ENTRIES`); a test that needs a longer or deliberately
/// invalid schedule calls `initialize` directly, which is also the only way
/// to exercise the rejection paths.
pub const GRADE_SCHEDULE_LEN: usize = 3;
/// The grade index tests default to unless they're specifically exercising
/// grade adjustment — index 0 is [`GRADE_PRICE_BPS`]'s full-price tier.
pub const FULL_PRICE_GRADE: u32 = 0;
/// The settlement-token amount every default commitment is funded with.
pub const TOTAL_AMOUNT: i128 = 1_000_000;

/// A set of `initialize` arguments, with the values both old harnesses used
/// as defaults. Every field is public and every builder method returns
/// `Self`, so a test can override exactly the term it is about:
///
/// ```
/// let terms = Terms::default().advances(5_000, 5_000);
/// let world = world_with_terms(terms);
/// ```
///
/// Deliberately holds the grade schedule as a fixed-size array rather than a
/// `soroban_sdk::Vec<u32>`: building that `Vec` needs an `Env`, and a builder
/// that needed an `Env` before it could describe an `Env`-independent set of
/// terms wouldn't be a builder.
///
/// Every field is public, so a term with no builder method below is still a
/// struct-update away:
/// `Terms { contracted_quantity: 5, ..Terms::default() }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Terms {
    pub total_amount: i128,
    pub advance1_bps: u32,
    pub advance2_bps: u32,
    pub claim_window_secs: u64,
    pub remainder_window_secs: u64,
    pub delivery_window_secs: u64,
    pub contracted_quantity: u32,
    pub grade_price_bps: [u32; GRADE_SCHEDULE_LEN],
}

impl Default for Terms {
    fn default() -> Self {
        Self {
            total_amount: TOTAL_AMOUNT,
            advance1_bps: 1_500,
            advance2_bps: 1_500,
            claim_window_secs: WINDOW,
            remainder_window_secs: REMAINDER_WINDOW,
            delivery_window_secs: DELIVERY_WINDOW,
            contracted_quantity: CONTRACTED_QUANTITY,
            grade_price_bps: GRADE_PRICE_BPS,
        }
    }
}

impl Terms {
    /// Same as [`Terms::default`], readable at a call site.
    pub fn new() -> Self {
        Self::default()
    }

    /// The two advance tranches, in basis points of `total_amount`. Their sum
    /// is also the deposit `lock` escrows.
    pub fn advances(mut self, advance1_bps: u32, advance2_bps: u32) -> Self {
        self.advance1_bps = advance1_bps;
        self.advance2_bps = advance2_bps;
        self
    }

    pub fn claim_window(mut self, claim_window_secs: u64) -> Self {
        self.claim_window_secs = claim_window_secs;
        self
    }

    pub fn remainder_window(mut self, remainder_window_secs: u64) -> Self {
        self.remainder_window_secs = remainder_window_secs;
        self
    }

    pub fn delivery_window(mut self, delivery_window_secs: u64) -> Self {
        self.delivery_window_secs = delivery_window_secs;
        self
    }

    /// The settlement-token amount. The default (1 000 000) funds a default
    /// deal exactly — deposit plus remainder; a test that needs an FX top-up
    /// or a partial-funding shape raises it here.
    pub fn with_total(mut self, total_amount: i128) -> Self {
        self.total_amount = total_amount;
        self
    }
}

/// [`Terms`] as the single agreed-terms struct `initialize` takes (#270) —
/// the grade schedule becomes an on-chain `Vec` here, which is why this needs
/// an `Env` and [`Terms`] itself does not.
pub fn commitment_terms(env: &Env, terms: &Terms) -> CommitmentTerms {
    CommitmentTerms {
        total_amount: terms.total_amount,
        advance1_bps: terms.advance1_bps,
        advance2_bps: terms.advance2_bps,
        claim_window_secs: terms.claim_window_secs,
        remainder_window_secs: terms.remainder_window_secs,
        delivery_window_secs: terms.delivery_window_secs,
        contracted_quantity: terms.contracted_quantity,
        grade_price_bps: Vec::from_array(env, terms.grade_price_bps),
    }
}

/// [`CommitmentTerms`] at this harness's default remainder/delivery windows,
/// quantity and grade schedule, with the four terms a suite usually varies
/// passed in explicitly — the compact form for suites whose subject is one
/// specific term rather than the terms struct itself.
pub fn terms(
    env: &Env,
    total_amount: i128,
    advance1_bps: u32,
    advance2_bps: u32,
    claim_window_secs: u64,
) -> CommitmentTerms {
    commitment_terms(
        env,
        &Terms::default()
            .advances(advance1_bps, advance2_bps)
            .claim_window(claim_window_secs)
            .with_total(total_amount),
    )
}

/// A minimal Reflector-shaped oracle for suites that need a *working* read.
///
/// Same XDR wire shape as `reflector.rs`'s `Asset`/`PriceData`, and as the
/// deliberately-broken `FaultyOracle` in `tests/oracle_fault_injection.rs`
/// (Soroban matches cross-contract calls on XDR shape, not Rust type
/// identity), but with no fault-injection hooks: `tests/window_overflow.rs`
/// only needs `resolve_fx_shortfall` to reach its deadline write, and with the
/// mock here every suite that wants that read shares one definition instead of
/// carrying its own copy (#271).
pub mod mock_oracle {
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
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        /// Records `price` as quoted at `timestamp`. The escrow contract
        /// checks that timestamp against the ledger clock at read time, so
        /// a caller testing a large `now` passes that same `now` here.
        pub fn set_price(env: Env, price: i128, timestamp: u64) {
            env.storage()
                .instance()
                .set(&symbol_short!("price"), &(price, timestamp));
        }

        pub fn lastprice(env: Env, _asset: Asset) -> Option<PriceData> {
            env.storage()
                .instance()
                .get::<_, (i128, u64)>(&symbol_short!("price"))
                .map(|(price, timestamp)| PriceData { price, timestamp })
        }

        /// The real fiat oracle quotes 14 decimals; the escrow contract reads
        /// this live and uses it to rescale the conversion.
        pub fn decimals(_env: Env) -> u32 {
            14
        }
    }
}

/// A fresh, initialized commitment plus everything a test needs to drive it:
/// the contract client, the settlement token's client, and the three named
/// parties. Mirrors what the two pre-#274 harnesses each built separately.
pub struct World {
    pub env: Env,
    pub contract: EscrowContractClient<'static>,
    pub token: token::Client<'static>,
    pub buyer: Address,
    pub cooperative: Address,
    pub warehouse: Address,
    pub total_amount: i128,
}

/// The name `src/test.rs`'s existing tests use. Same type, not a wrapper.
pub type Setup = World;

/// Registers a Stellar Asset Contract, minted by `admin`. Shared so the unit
/// tests and every integration suite mint tokens the same way.
pub fn create_token<'a>(
    env: &Env,
    admin: &Address,
) -> (Address, token::Client<'a>, token::StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let address = sac.address();
    (
        address.clone(),
        token::Client::new(env, &address),
        token::StellarAssetClient::new(env, &address),
    )
}

/// A default-terms commitment with the given advance split, at ledger
/// timestamp 0, with every auth mocked.
pub fn world(advance1_bps: u32, advance2_bps: u32) -> World {
    world_with_terms(Terms::default().advances(advance1_bps, advance2_bps))
}

/// Like [`world`], with the claim window overridden — the shape the
/// claim/reclaim boundary suites use.
pub fn world_with_window(advance1_bps: u32, advance2_bps: u32, window: u64) -> World {
    world_with_terms(
        Terms::default()
            .advances(advance1_bps, advance2_bps)
            .claim_window(window),
    )
}

/// [`world`], but built from an explicit [`Terms`] — the entry point for
/// anything that cares about more than the advance split, e.g. the
/// fully-prepaid (#275) and delivery-window (#277) cases.
pub fn world_with_terms(terms: Terms) -> World {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    world_in_with_terms(&env, buyer, cooperative, warehouse, terms)
}

/// The identity-agnostic half — generates nothing, so tests that need a
/// specific (including degenerate, two-roles-one-address) set of named
/// parties can pass their own addresses in. See `setup_shaped` and the
/// auth-model tests. Auth mocking is the caller's business here, exactly as
/// it was for both old harnesses, since this is also the constructor the
/// suites that mint their own auth trees use.
pub fn world_in(
    env: &Env,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    advance1_bps: u32,
    advance2_bps: u32,
    window: u64,
) -> World {
    world_in_with_terms(
        env,
        buyer,
        cooperative,
        warehouse,
        Terms::default()
            .advances(advance1_bps, advance2_bps)
            .claim_window(window),
    )
}

/// [`world_in`] taking the terms as a builder instead of loose arguments.
/// Mints exactly `terms.total_amount` to the buyer, which is enough for a
/// default deal (deposit + remainder == `total_amount`) but not for tests
/// that need an FX top-up on top — those mint more and call `initialize`
/// themselves.
pub fn world_in_with_terms(
    env: &Env,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    terms: Terms,
) -> World {
    let token_admin = Address::generate(env);
    let (token_address, token, token_admin_client) = create_token(env, &token_admin);
    token_admin_client.mint(&buyer, &terms.total_amount);

    let contract_id = env.register(EscrowContract, ());
    let contract = EscrowContractClient::new(env, &contract_id);
    contract.initialize(
        &buyer,
        &cooperative,
        &warehouse,
        &token_address,
        &commitment_terms(env, &terms),
        &None,
    );

    World {
        env: env.clone(),
        contract,
        token,
        buyer,
        cooperative,
        warehouse,
        total_amount: terms.total_amount,
    }
}

/// Which of the three named parties deliberately share an address, for the
/// auth-model tests. `initialize` does not reject any of these shapes (a
/// cooperative that also runs the warehouse is a realistic single operator;
/// the all-identical case is covered by its own companion issue) — these
/// exist so the auth checks' behavior under them is proven, not assumed.
#[derive(Clone, Copy)]
pub enum PartyShape {
    /// Three genuinely distinct addresses.
    Distinct,
    /// One address is both buyer and warehouse operator.
    BuyerIsWarehouse,
    /// One address is both buyer and cooperative.
    BuyerIsCooperative,
    /// One address is all three named parties.
    AllSame,
}

pub fn setup_shaped(shape: PartyShape) -> World {
    let env = Env::default();
    env.mock_all_auths();

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let (buyer, cooperative, warehouse) = match shape {
        PartyShape::Distinct => (a, b, c),
        PartyShape::BuyerIsWarehouse => (a.clone(), b, a),
        PartyShape::BuyerIsCooperative => (a.clone(), a, c),
        PartyShape::AllSame => (a.clone(), a.clone(), a),
    };
    world_in(&env, buyer, cooperative, warehouse, 1_500, 1_500, WINDOW)
}

/// A `MockAuth`-compatible invocation descriptor: the exact contract, method,
/// and serialized arguments that an authorization entry is scoped to. Soroban
/// binds each auth entry to a specific invocation tree (contract + fn_name +
/// args, plus sub-invocations), which is what makes replaying a signature
/// against a different method or different arguments impossible (#141).
pub fn mock_invoke<'a>(
    contract: &'a Address,
    fn_name: &'a str,
    args: Vec<Val>,
) -> MockAuthInvoke<'a> {
    MockAuthInvoke {
        contract,
        fn_name,
        args,
        sub_invokes: &[],
    }
}

/// The deposit-only amount `lock` actually escrows, for a given bps split —
/// mirrors `EscrowContract::deposit_amount` so tests can assert against it
/// without reaching into contract internals.
pub fn deposit_amount(total: i128, advance1_bps: u32, advance2_bps: u32) -> i128 {
    total * i128::from(advance1_bps + advance2_bps) / 10_000
}

/// Advances the simulated ledger clock forward by `secs`. Used to cross a
/// claim-window deadline deterministically instead of relying on wall time.
pub fn advance_time(s: &World, secs: u64) {
    let now = s.env.ledger().timestamp();
    s.env.ledger().set_timestamp(now + secs);
}

/// Sets the simulated ledger clock to an absolute timestamp. The
/// deadline-boundary tests read the deadline straight out of contract state
/// and jump to exactly that value: `advance_time`'s `now + secs` expresses
/// "clearly before" / "clearly after" fine, but re-deriving the deadline by
/// hand to land on it exactly is the same hand-arithmetic those tests exist
/// to avoid (and would let a wrong window-length constant hide).
pub fn set_time(s: &World, timestamp: u64) {
    s.env.ledger().set_timestamp(timestamp);
}

/// Drives a fresh, locked commitment all the way to `ReadyForDelivery` with
/// the remainder funded — both tranches claimed normally along the way.
/// Shared by the several tests below that only care about what happens
/// *after* this point (confirm_delivery, settle) — the tranche mechanics
/// themselves already have their own dedicated coverage above.
pub fn advance_to_remainder_funded(s: &World) {
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
}

/// The same drive, stopping at `ReadyForDelivery` with the remainder
/// deliberately left **unfunded** — the state every `remainder_deadline`
/// boundary assertion starts from.
pub fn advance_to_ready_for_delivery(s: &World) {
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
}

// The pre-#274 in-crate names, kept as thin aliases so `src/test.rs` reads
// exactly as it did. New code should use `world*`.
pub fn setup(advance1_bps: u32, advance2_bps: u32) -> World {
    world(advance1_bps, advance2_bps)
}

pub fn setup_with_window(advance1_bps: u32, advance2_bps: u32, window: u64) -> World {
    world_with_window(advance1_bps, advance2_bps, window)
}

pub fn setup_in(
    env: &Env,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    advance1_bps: u32,
    advance2_bps: u32,
    window: u64,
) -> World {
    world_in(
        env,
        buyer,
        cooperative,
        warehouse,
        advance1_bps,
        advance2_bps,
        window,
    )
}

impl World {
    pub fn now(&self) -> u64 {
        self.env.ledger().timestamp()
    }

    pub fn set_time(&self, timestamp: u64) {
        self.env.ledger().set_timestamp(timestamp);
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
