//! HarvestLock escrow contract.
//!
//! One instance per commitment (PRD §4.8). Implements the state machine:
//!
//!   Draft -> Locked -> Advance1Released -> CheckpointPassed
//!         -> Advance2Released -> ReadyForDelivery -> Delivered -> Settled
//!
//! **Two-phase funding**, not one: `lock` escrows only the deposit
//! (`advance1_bps + advance2_bps` of `total_amount`) — not the whole
//! amount the way earlier versions of this contract did. The remainder
//! is escrowed later, at `fund_remainder`, once the cooperative signals
//! `ready_for_delivery`. This is a deliberate redesign, not a bug fix:
//! it's what makes "buyer default" a clean, contract-enforceable
//! deadline (`expire_remainder_window`) instead of something that has to
//! be asserted by a person. The mirror case — the cooperative never
//! delivering at all — gets its own deadline (`reclaim_on_nondelivery`),
//! since a contract can't detect "nobody showed up" the way it can
//! detect "nobody paid by the deadline."
//!
//! `cancel` is a mutual-consent unwind (PRD §7) reachable from any state
//! up through `ReadyForDelivery` — see its doc comment. `reassign_buyer`
//! (PRD §4.8) transfers the buyer position, three-party-consented, over
//! the same range. `Defaulted` and `Forfeited` cover the *uncontested*,
//! deadline-triggered failure cases only.
//!
//! `flag_dispute` (PRD's must-have "dispute flagging with defined
//! escalation") lets any one of the three named parties freeze a
//! commitment into `Disputed` from `Locked` through `Delivered`, which
//! blocks every other state-changing call for free (none of them ever
//! accept `Disputed` as a required status). `resolve_dispute` (unanimous
//! three-party consent) or `expire_dispute_window` (permissionless, once
//! `dispute_deadline` passes) both restore the exact pre-dispute status —
//! this still isn't arbitration of *who was right*, just a bounded pause,
//! same non-goal as before; see `flag_dispute`'s own doc comment for the
//! full reasoning and its one known limitation (other deadlines on the
//! commitment don't pause while a dispute is open).
//!
//! All absolute deadlines here follow one boundary rule: the recorded
//! deadline timestamp is the *last* second on which the original window
//! is still open, and every deadline-triggered failure path opens
//! strictly after it. So at `now == deadline`, `lock`, `fund_remainder`,
//! `fund_fx_shortfall` and `resolve_dispute` still succeed, while
//! `expire_remainder_window`, `expire_fx_shortfall_window`,
//! `reclaim_on_nondelivery` and `expire_dispute_window` still refuse — a
//! payment and a default can never both be legal in the same second.
//! Each of those four boundaries is pinned at the exact timestamp by its
//! own test (#112–#115), not just by a clearly-before/clearly-after pair.
//!
//! Advance tranches use claimable-balance-equivalent semantics, built
//! natively in this contract rather than via classic Stellar
//! `ClaimableBalanceEntry` interop (HANDOFF.md explains why): opening a
//! tranche starts a claim window; the cooperative can `claim_*` within it;
//! the buyer can `reclaim_*` once it's passed and nobody claimed. Both
//! tranches must be resolved (claimed or expired) before `settle` will
//! run — see `settle`'s doc comment for why that's required rather than
//! `settle` inferring an outcome for whatever's left unresolved.
//!
//! `set_allocation` records each member farmer's entitlement share as a
//! salted hash — PRD's allocation ledger, record-only in v1 (Transparency
//! Ladder Rung 1, PRD §4.9's own stated default): `settle` doesn't
//! pro-rate a payout across members, it stays a lump sum to the
//! cooperative wallet, with the ledger providing on-chain transparency
//! into what each member is owed off-chain. See its doc comment for the
//! NDPA-erasability reasoning behind the salted-hash design.
//!
//! `oracle_rate` reads a live FX rate from a [Reflector](https://reflector.network)
//! SEP-40 oracle (`reflector.rs`), with a caller-configured staleness
//! bound (PRD §16.3) — a stale or missing quote is a hard error, never a
//! silently-old number. `initialize`'s `oracle_config` is `Option`al:
//! `None` for a plain deal that needs no conversion, `Some` for a deal
//! denominated in a currency the settlement token doesn't natively track
//! (PRD §4.2's NGN-unit-of-account design). Reflector's live testnet
//! fiat-rate oracle does not currently quote NGN at all (verified via a
//! real `stellar contract invoke ... -- assets` call — see
//! `reflector.rs`) — the same category of gap PRD §4.4 already named
//! for commodity prices, just discovered here for the currency-
//! conversion half too; `price_asset` works with anything Reflector
//! does quote today (this session's tests use GBP), and activates for
//! NGN the day Reflector adds it, no code change needed.
//!
//! **PRD §4.2's option (b) — buyer tops up or is refunded at
//! settlement — is now wired in**, a product decision made explicitly
//! rather than assumed (the PRD names three options and leaves the
//! choice to pilot partners). `resolve_fx_shortfall` — permissionless,
//! reachable once `Delivered` with both advance tranches already
//! resolved, so the comparison uses final, settled figures rather than
//! a snapshot that could still change — reads a fresh oracle rate,
//! converts `oracle_config.denominated_amount` (the deal's true value
//! in `price_asset`, adjusted by `confirm_delivery`'s `settlement_bps`)
//! into the settlement token, and compares it against what's actually
//! escrowed. A shortfall (the settlement currency strengthened enough
//! since funding that the escrowed amount no longer covers the
//! obligation) has to be paid in by the buyer via `fund_fx_shortfall`
//! before `settle` will run; missing that deadline is
//! `expire_fx_shortfall_window`, which reuses `Status::Defaulted` —
//! the *same* immediate-permanent-bar consequence
//! `expire_remainder_window`'s buyer-default already carries, by
//! product decision, not by accident, and for free at the API layer
//! since the existing reputation machinery already reacts to any fresh
//! transition into `Defaulted`. One-time and computed exactly once per
//! commitment: re-resolving later against a different rate would let
//! either side game the number by choosing when to call it.
//!
//! **The "waiting on a fresh quote" signal (#1)**: a stale quote still
//! refuses `resolve_fx_shortfall` outright (`Error::OracleStale`) — and
//! because a Soroban invocation that returns `Err` rolls back every state
//! write it made, that refusal can't itself record *why* settlement is
//! stuck. `refresh_fx_quote_status` — permissionless, same reasoning as
//! `resolve_fx_shortfall` — is the write that refusal can't be: it runs
//! the same staleness check and records the answer in
//! `Commitment::fx_quote_stale`, which `get_commitment` surfaces so the
//! API/indexers/UI can show "waiting on a fresh quote" instead of a
//! caller having to infer that from a failed `resolve_fx_shortfall` call.
//! Deliberately a boolean flag alongside `fx_resolved`, not a new
//! `Status` variant: this is a condition on one sub-step of settlement,
//! not a position in the deal's lifecycle (the commitment is still
//! `Delivered`), and a new variant would force every status-matching
//! guard, `StatusChanged` consumer, and off-chain reputation mapping to
//! learn about an oracle-side delay none of them act on. The flag gates
//! nothing — `resolve_fx_shortfall` still enforces staleness itself — so
//! its worst-case failure is a misleading display, never a wrong payout.
//!
//! **Storage lifetime (#98)**: every persistent fact this contract keeps —
//! the `Commitment` struct, the allocation ledger, the oracle config, and
//! the dispute fields (which live *inside* `Commitment`, not under
//! separate keys) — is instance storage, i.e. one `ContractData` ledger
//! entry with one TTL, and nothing here uses `env.storage().persistent()`
//! or `temporary()`. Soroban does **not** extend a contract instance's
//! TTL merely because the contract was called, and a commitment can
//! legitimately sit in `Draft` or `Locked` with no deadline forcing
//! anyone to act; so every read path funnels through `load`, which
//! re-rents the entry when its remaining lifetime has decayed below half
//! the network maximum (see `bump_instance_ttl`'s doc comment). The full
//! key-by-key audit, the residual expiry risk, and the operational
//! mitigations live in `docs/storage-ttl.md`.
//!
//! **The state machine is declared once, not once per function (#227)**:
//! every status guard on a state-changing call is a single
//! `require_status(&c, Method::X)` lookup into the one const transition
//! table below (`Method` + `Method::allowed_from`). Read that table top to
//! bottom and you have the whole machine: which method each status admits,
//! and — via its `None` rows — which two calls are deliberately not
//! status-gated at all (`claim_advance_*`/`reclaim_advance_*`, whose
//! windows stay actionable while a dispute is open; see `Method`'s doc).
//! Nothing in this file may re-state a source-status list by hand: a new
//! guard is a new table row, so the "Disputed blocks everything" property
//! and the "pre-delivery range" are each written once instead of a dozen
//! times. The guard *order* (status first, then flags, then deadlines) is
//! untouched by that centralization, which is what keeps the existing
//! error-ordering expectations valid.
//!
//! **Test builds assert the same invariants on every write (#225)**:
//! `save` calls `Commitment::check_invariants` under
//! `cfg(any(test, feature = "testutils"))`, so the field doc comments that
//! state things like "0 = not yet opened" or "`Draft` whenever `status`
//! isn't `Disputed`" are checked by every test that writes state, not only
//! by whoever rereads them. It compiles out of the release WASM entirely.

#![no_std]

#[cfg(test)]
extern crate std;

mod reflector;

/// The shared test harness (#274): one copy of the fixtures, constants and
/// clock helpers behind every suite — the in-crate unit tests in
/// `src/test.rs` and the integration suites in `tests/`. Declared here rather
/// than duplicated so the two can't drift; see the module's own docs for how
/// each suite reaches it and why the integration suites need the `testutils`
/// feature (which `Cargo.toml`'s self dev-dependency turns on for them).
#[cfg(any(test, feature = "testutils"))]
pub mod testutils;

use reflector::{Asset as ReflectorAsset, ReflectorPulseClient};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, symbol_short, token,
    Address, BytesN, Env, Symbol, Vec,
};

// Build metadata embedded in the WASM's `contractmetav0` custom section (#246),
// so a deployed contract can say which version and source it came from without
// a lookup in CHANGELOG.md. Read it back with:
//
//   stellar contract info meta --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm
//
// Both values come from Cargo, so there is one place to change each:
// `version` from `contracts/escrow/Cargo.toml`'s `version` (kept in step with
// the deployment line per docs/versioning.md §5), `repository` from the
// workspace `[workspace.package] repository`. Deliberately no timestamp, git
// hash or other build-environment value: those would differ between two clean
// builds of the same commit and break the `build-wasm` reproducibility check.
// See docs/versioning.md §6.
soroban_sdk::contractmeta!(
    key = "version",
    val = concat!("escrow-v", env!("CARGO_PKG_VERSION"))
);
soroban_sdk::contractmeta!(key = "repository", val = env!("CARGO_PKG_REPOSITORY"));

/// Maximum number of entries `initialize` accepts in `grade_price_bps`.
/// Deliberately generous — the PRD's grading examples are 3 tiers (A/B/C),
/// so 16 leaves headroom for any realistic refinement while keeping the
/// schedule's per-call iteration and storage cost bounded. See the
/// `GradeScheduleTooLong` error.
pub const MAX_GRADE_ENTRIES: u32 = 16;

/// Maximum number of members `set_allocation` accepts. Anchored to PRD
/// §16.3, which sizes the disbursement design against a 200-farmer
/// cooperative ("200 payments of ~$50", "across 200 members") — the
/// largest real membership this product is specified for. A list meaningfully
/// longer than that is a caller mistake, not a bigger cooperative. See the
/// `AllocationTooLarge` error.
pub const MAX_ALLOCATION_MEMBERS: u32 = 200;

/// Largest `OracleConfig::denominated_amount` `initialize` accepts (#247):
/// the biggest value for which `bps_amount(denominated_amount, 10_000)` —
/// `denominated_amount * 10_000`, the largest multiply `resolve_fx_shortfall`
/// performs on it, since `settlement_bps` never exceeds 10_000 — still fits
/// in an `i128`. Anything above would make settlement trap on overflow
/// rather than be refused up front with `InvalidOracleConfig`.
///
/// This bounds only the `bps_amount` step. The later
/// `ngn_owed * rate.price` multiply depends on a live oracle value that
/// `initialize` cannot know, and is the F4 finding pinned in
/// `tests/oracle_fault_injection.rs`.
pub const MAX_DENOMINATED_AMOUNT: i128 = i128::MAX / 10_000;

/// Largest `total_amount` `initialize` accepts.
///
/// `bps_amount` computes `total * bps / 10_000`, and every bps value it is
/// ever called with is bounded by 10_000 (`advance1_bps + advance2_bps` is
/// checked at `initialize`; a single tranche is one of those two). With
/// `overflow-checks = true` in the release profile, a `total_amount` above
/// `i128::MAX / 10_000` makes that multiplication trap in `lock`,
/// `claim_advance_*`, `deposit_amount` and `settle` — long after the call
/// that supplied the bad value. Rejecting it at `initialize` keeps the
/// failure at the call that took the input, and makes every `bps_amount`
/// call site overflow-free by construction. See `Error::AmountTooLarge`.
pub const MAX_TOTAL_AMOUNT: i128 = i128::MAX / 10_000;

/// Maximum number of times `flag_dispute` may be called per commitment.
/// Only a lapsed dispute window (resolved by `expire_dispute_window`) counts
/// against the cap; a unanimous `resolve_dispute` does not.
/// Three covers any realistic round of parallel good-faith attempts and
/// closes the stalling attack documented in `docs/dispute-stalling.md`.
/// See `Error::DisputeCapReached`.
pub const MAX_DISPUTE_FLAGS: u32 = 3;

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Draft,
    Locked,
    Advance1Released,
    CheckpointPassed,
    Advance2Released,
    /// The cooperative has signaled intent to deliver — opens the
    /// remainder-payment window. See module docs.
    ReadyForDelivery,
    Delivered,
    Settled,
    Cancelled,
    /// Buyer failed to fund the remainder before its deadline. Uncontested
    /// by construction — the deadline either passed or it didn't.
    Defaulted,
    /// Cooperative never reached `Delivered` before the overall delivery
    /// deadline. Also uncontested by construction. Deliberately a
    /// separate variant from `Defaulted`, not a reuse of it — the two
    /// represent opposite parties' failure, and collapsing them into one
    /// status would make an already-settled commitment's history
    /// ambiguous about who actually failed to perform.
    Forfeited,
    Disputed,
}

/// Whose authorization a callable action needs, as advertised by
/// [`Self::available_actions`].
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthParty {
    /// No `require_auth()` at all — anyone may submit the call.
    Anyone,
    /// The commitment's current `buyer`.
    Buyer,
    /// The commitment's `cooperative`.
    Cooperative,
    /// The commitment's `warehouse_operator`.
    WarehouseOperator,
    /// Both the buyer and the cooperative (deduplicated by address).
    BuyerAndCooperative,
    /// All three named parties (buyer, cooperative, warehouse operator),
    /// deduplicated by address.
    AllParties,
    /// Any one of the three named parties, authenticating as the `flagger`
    /// it passes. Strangers are refused with `Error::NotAParty`.
    AnyNamedParty,
    /// Current buyer, cooperative, **and** the incoming buyer supplied as
    /// `new_buyer` — the three signatures `reassign_buyer` requires.
    BuyerCooperativeAndNewBuyer,
    /// Current buyer, cooperative, **and** the incoming operator supplied as
    /// `new_operator` — the three signatures `reassign_operator` requires.
    BuyerCooperativeAndNewOperator,
}

/// The timestamp, if any, that bounds a callable action. The variant says
/// which side of the boundary the action lives on, so a client can tell
/// "claim closes at T" from "expiry opens just after T" without knowing
/// each function's guard by heart.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionDeadline {
    /// No deadline gates this action in the current state.
    None,
    /// Legal while `env.ledger().timestamp() <= T`.
    Inclusive(u64),
    /// Legal once `env.ledger().timestamp() > T`.
    Exclusive(u64),
}

/// One state-changing call that the current commitment permits.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// The public function to invoke, as a Soroban symbol.
    pub function: Symbol,
    pub auth: AuthParty,
    pub deadline: ActionDeadline,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidState = 3,
    InvalidBps = 4,
    ZeroAmount = 5,
    InvalidWindow = 6,
    NotYetOpened = 7,
    AlreadyClaimed = 8,
    AlreadyExpired = 9,
    ClaimWindowPassed = 10,
    ClaimWindowNotPassed = 11,
    /// A tranche is neither claimed nor expired yet — `settle` refuses to
    /// run until someone resolves it via `claim_advance_*` or
    /// `reclaim_advance_*`. See `settle`'s doc comment for why this is a
    /// hard requirement, not just a nicety.
    TrancheUnresolved = 12,
    RemainderAlreadyFunded = 13,
    /// `fund_remainder` called after `remainder_deadline` has *strictly*
    /// passed — the deadline second itself is still in time.
    RemainderWindowPassed = 14,
    /// `expire_remainder_window` called at or before
    /// `remainder_deadline` — the deadline second itself still belongs to
    /// the buyer's funding window, so expiry needs strictly more than the
    /// deadline, not merely the deadline.
    RemainderWindowNotPassed = 15,
    /// `confirm_delivery` called while `ReadyForDelivery` but the buyer
    /// hasn't funded the remainder yet.
    RemainderNotFunded = 16,
    /// `reclaim_on_nondelivery` called at or before `delivery_deadline` —
    /// the deadline second itself still belongs to the cooperative.
    DeliveryDeadlineNotPassed = 17,
    /// `contracted_quantity` was zero at `initialize`.
    InvalidQuantity = 18,
    /// `grade_price_bps` was empty, or contained an entry over 10_000
    /// (a grade worth more than the pre-agreed full unit price) at
    /// `initialize`.
    InvalidGradeSchedule = 19,
    /// `confirm_delivery`'s `grade_index` didn't land inside
    /// `grade_price_bps`.
    InvalidGradeIndex = 20,
    /// `set_allocation` called a second time — the allocation ledger is
    /// one-time and immutable once recorded.
    AllocationAlreadySet = 21,
    /// `set_allocation`'s `members` was empty, or the sum of every
    /// entry's `share_bps` exceeded 10_000.
    InvalidAllocation = 22,
    /// `get_allocation` called before `set_allocation` ever ran.
    AllocationNotSet = 23,
    /// `initialize`'s `oracle_config` was `Some` with either:
    /// - `max_age_secs == 0`: a zero staleness bound would reject every
    ///   real quote, which is never what a caller actually wants (that's
    ///   what leaving `oracle_config` as `None` is for); or
    /// - `denominated_amount <= 0` or `> MAX_DENOMINATED_AMOUNT` (#247):
    ///   a non-positive value would make `resolve_fx_shortfall` owe the
    ///   cooperative nothing after a full delivery, and an oversized one
    ///   would overflow `bps_amount` and trap at settlement.
    InvalidOracleConfig = 24,
    /// `oracle_rate` or `resolve_fx_shortfall` called but `initialize`
    /// never set an `oracle_config` for this commitment — the only two
    /// functions that read the oracle at all (#124).
    OracleNotConfigured = 25,
    /// The configured Reflector oracle returned `None` for
    /// `oracle_config.price_asset` — it doesn't quote that asset at all
    /// (call the oracle's own `assets()` to check what it does quote).
    OraclePriceUnavailable = 26,
    /// The most recent quote is older than `oracle_config.max_age_secs`
    /// allows (PRD §16.3's staleness bound) — refused rather than
    /// returned, since a caller silently getting a too-old rate is worse
    /// than getting no rate at all.
    OracleStale = 27,
    /// `resolve_fx_shortfall` called a second time — one-time and
    /// immutable, same reasoning as `set_allocation`: re-resolving later
    /// against a different rate would let either side game the number
    /// by choosing when to call it.
    FxAlreadyResolved = 28,
    /// `fund_fx_shortfall`/`expire_fx_shortfall_window`/`settle` called
    /// before `resolve_fx_shortfall` has run for an oracle-configured
    /// commitment.
    FxNotResolved = 29,
    /// `settle` called while a resolved FX shortfall is still unfunded.
    FxShortfallUnfunded = 30,
    /// `fund_fx_shortfall`/`expire_fx_shortfall_window` called but
    /// `resolve_fx_shortfall` found no shortfall at all (the escrowed
    /// amount already covered the fresh-rate-converted obligation) —
    /// there's nothing to fund or expire.
    NoFxShortfall = 31,
    /// `fund_fx_shortfall` called a second time.
    FxShortfallAlreadyFunded = 32,
    /// `fund_fx_shortfall` called after `fx_shortfall_deadline` has
    /// *strictly* passed — too late to cure, though the deadline second
    /// itself is still in time. `expire_fx_shortfall_window` is the only
    /// path left from here.
    FxShortfallWindowPassed = 33,
    /// `expire_fx_shortfall_window` called at or before
    /// `fx_shortfall_deadline` — the deadline second itself still belongs
    /// to the buyer's funding window, matching `fund_fx_shortfall`'s own
    /// inclusive cut-off (`now > deadline`).
    FxShortfallWindowNotPassed = 34,
    /// `flag_dispute`'s `flagger` argument wasn't the buyer, cooperative,
    /// or warehouse operator — only a named party to the deal can flag
    /// one, not an arbitrary third address.
    NotAParty = 35,
    /// `expire_dispute_window` called at or before `dispute_deadline` —
    /// the deadline second itself is still inside the freeze. Only that
    /// timeout path can produce this: `resolve_dispute` has no deadline
    /// gate at all. (Either call made while `status` isn't `Disputed` is
    /// covered by the same `InvalidState` every other status-gated call
    /// already uses.)
    DisputeWindowNotPassed = 36,
    /// `initialize`'s `grade_price_bps` held more than
    /// `MAX_GRADE_ENTRIES` entries — longer than any real grading
    /// schedule needs, and unbounded work the contract shouldn't accept.
    GradeScheduleTooLong = 37,
    /// `set_allocation`'s `members` held more than
    /// `MAX_ALLOCATION_MEMBERS` entries — beyond the PRD's specified
    /// cooperative scale, and unbounded storage the contract shouldn't
    /// accept.
    AllocationTooLarge = 38,
    /// `initialize`'s `total_amount` exceeded `MAX_TOTAL_AMOUNT`
    /// (`i128::MAX / 10_000`) — a commitment whose `bps_amount` arithmetic
    /// would overflow, and therefore one that could never be locked or
    /// settled. Refused at the call that supplied the value (#249).
    AmountTooLarge = 39,
    /// The configured Reflector oracle returned a zero or negative price;
    /// non-positive exchange rates cannot produce a valid settlement amount.
    OraclePriceInvalid = 40,
    /// `expire_delivery_window` called before `delivery_deadline +
    /// remainder_window_secs` has strictly passed. The permissionless
    /// fallback is intentionally delayed past the buyer's own reclaim
    /// window to give the buyer time to act first.
    DeliveryGraceWindowNotPassed = 41,
    /// `flag_dispute` called after the per-commitment `MAX_DISPUTE_FLAGS`
    /// cap has been reached. Only `expire_dispute_window` lapsals count
    /// against the cap; `resolve_dispute` (unanimous consent) does not.
    DisputeCapReached = 42,
    /// `confirm_delivery` called after `delivery_deadline` has *strictly*
    /// passed (`now > delivery_deadline`). The deadline second itself still
    /// belongs to the operator's confirmation window; strictly after it, only
    /// `reclaim_on_nondelivery` is legal (#199).
    DeliveryDeadlinePassed = 43,
    /// The oracle conversion's numerator or decimal divisor cannot be
    /// represented as an `i128`; settlement must not trap on unusable oracle
    /// values.
    OracleConversionOutOfRange = 44,
    /// The configured Reflector oracle returned a quote timestamp later than
    /// the current ledger timestamp; future quotes cannot satisfy freshness.
    OracleTimestampInFuture = 45,
    /// The configured oracle contract could not answer at all (#245): the
    /// cross-contract `lastprice` or `decimals` call failed in a way the
    /// host lets the caller recover from. That covers an
    /// `oracle_config.oracle_contract` with no contract deployed behind it,
    /// a callee that panics or traps, and a callee whose reply doesn't
    /// decode as the expected type. Distinct from `OraclePriceUnavailable`,
    /// which means the oracle *did* answer and quotes no such asset.
    ///
    /// Not every failure can become this error: the host still aborts the
    /// whole invocation, with no contract error to return, when the
    /// callee uses up the transaction's shared CPU/memory budget. See the
    /// header of `tests/oracle_fault_injection.rs` for which failures are
    /// recoverable and which still abort.
    OracleUnavailable = 46,
    /// `lock` called after `delivery_deadline` has *strictly* passed
    /// (`now > delivery_deadline`) — the deadline second itself is still in
    /// time, matching every other deadline here. The delivery window is the
    /// cooperative's time to perform and it starts at `initialize` (see
    /// `Commitment::delivery_deadline`), so a lock that lands after it would
    /// hand the buyer an immediate `reclaim_on_nondelivery` — a `Forfeited`
    /// strike against a cooperative that never had a window to deliver in
    /// (#277).
    LockAfterDeliveryDeadline = 47,
    /// `extend_delivery_deadline` was asked to move the deadline backwards
    /// or leave it unchanged.
    DeliveryDeadlineNotExtended = 48,
}

#[contracttype]
pub enum DataKey {
    Commitment,
    /// Present only once `set_allocation` has run — absence, not an
    /// empty `Vec`, is how `get_allocation` distinguishes "never set"
    /// from a (rejected-at-write-time, so never actually reachable)
    /// empty list.
    Allocation,
    /// Present only when `initialize`'s `oracle_config` was `Some` —
    /// absence means this commitment needs no currency conversion.
    OracleConfig,
}

/// Which Reflector oracle instance and asset symbol `oracle_rate` and
/// `resolve_fx_shortfall` read from, how old a quote they'll accept, and
/// what the deal is actually worth in that currency. Set once, at
/// `initialize`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleConfig {
    /// The Reflector oracle contract to call — e.g. the testnet
    /// "Fiat exchange rates" instance, `CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W`
    /// (see `reflector.rs`).
    pub oracle_contract: Address,
    /// The asset symbol to quote against that oracle's `base()` — e.g.
    /// `NGN`. Always queried as `ReflectorAsset::Other`, since this
    /// contract's use case is fiat/FX symbols, not Stellar-native assets.
    pub price_asset: Symbol,
    /// Maximum age, in seconds, a quote may have and still be accepted —
    /// PRD §16.3's oracle staleness bound. Must be > 0.
    pub max_age_secs: u64,
    /// The deal's true value in `price_asset`, in the same "smallest
    /// unit, 7 decimal places" convention `total_amount` already uses
    /// for the settlement token (so e.g. 1 NGN == 10_000_000 units here,
    /// matching how 1 XLM == 10_000_000 stroops) — **not** the same
    /// number as `total_amount`, which is `denominated_amount`'s value
    /// *at whatever rate happened to hold when the deal was funded*.
    /// `resolve_fx_shortfall` reprices this at a fresh rate; the gap
    /// between that and what's actually escrowed is PRD §4.2's FX risk,
    /// make concrete. This field always uses the 7-decimal convention;
    /// `resolve_fx_shortfall` converts the result to the settlement token's
    /// own `decimals()` (which is not necessarily 7 — see #258) before
    /// comparing it with the escrowed balance. See that function's doc
    /// comment for the worked example.
    ///
    /// **Not** the oracle's `decimals()` — that is a property of the
    /// oracle, read live at settlement and assumed immutable (see
    /// `docs/reflector-decimals-assumption.md`, issue #63); this field is
    /// the deal's value in the same 7-decimal units as `total_amount`.
    ///
    /// **Must be in `1..=MAX_DENOMINATED_AMOUNT`** (#247), or `initialize`
    /// refuses with `InvalidOracleConfig`. Zero or negative would make
    /// `resolve_fx_shortfall`'s `fx_adjusted_total <= 0`, so `settle` would
    /// pay the cooperative nothing and refund everything to the buyer after
    /// a full delivery. Above `MAX_DENOMINATED_AMOUNT` (`i128::MAX / 10_000`),
    /// `bps_amount(denominated_amount, settlement_bps)` would overflow and
    /// trap at settlement. Both mistakes are caught at deploy time instead
    /// of only showing up when the deal settles.
    pub denominated_amount: i128,
}

/// A live rate read from the configured Reflector oracle, returned by
/// `oracle_rate`. Mirrors Reflector's own `PriceData` shape (see
/// `reflector.rs`) rather than reusing that type directly, since this is
/// this contract's own public interface, not a re-export of Reflector's.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleRate {
    /// `price / 10^decimals` is the actual rate, where `decimals` comes
    /// from the oracle contract's own `decimals()` (14, for Reflector's
    /// fiat exchange rate oracle as of this writing) — not duplicated
    /// into `OracleConfig`, since it's a property of the oracle, not of
    /// this commitment, and callers can read it directly off the oracle
    /// contract if they need it.
    pub price: i128,
    /// The ledger timestamp the oracle recorded this quote at — already
    /// checked against `max_age_secs` by the time this is returned, but
    /// surfaced anyway so a caller can display "as of" freshness.
    pub timestamp: u64,
}

/// One farmer member's recorded share of a commitment, captured by
/// `set_allocation`. `member_hash` is a per-member salted hash — never a
/// bare phone number or other PII — computed off-chain (API) as
/// `HMAC-SHA256(salt, phone_number)` with a fresh random salt per
/// member. The salt and the phone-number mapping live only in the API's
/// Postgres, which is what makes this genuinely erasable per NDPA s.34:
/// delete the off-chain row and this hash becomes permanently
/// unlinkable to a real person, since the salt is gone and brute-
/// forcing a random salt is infeasible regardless of how small the
/// phone-number keyspace is.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationMember {
    pub member_hash: BytesN<32>,
    /// Basis points of the commitment's total payout this member is
    /// entitled to, off-chain. Record-only in v1 — see `set_allocation`'s
    /// doc comment for why `settle` doesn't pro-rate against this.
    pub share_bps: u32,
}

/// Which advance tranche an operation applies to. Not part of the public
/// contract interface — `claim_advance_1`/`claim_advance_2` etc. are
/// separate exported functions that both call into the same internal
/// logic parameterized by this, so the claim/reclaim rules can't drift
/// between the two tranches.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tranche {
    One,
    Two,
}

struct TrancheRef<'a> {
    deadline: &'a mut u64,
    claimed: &'a mut bool,
    expired: &'a mut bool,
}

/// On-chain state for a single escrow commitment instance.
///
/// **Footprint Audit & Storage Necessity Finding**:
/// A comprehensive review of every field in this struct confirmed that all 33
/// fields are genuinely necessary in on-chain instance storage and cannot be
/// moved off-chain or derived from events:
/// 1. **Auth & Token Routing** (`buyer`, `cooperative`, `warehouse_operator`, `token`):
///    Enforces `require_auth()` and routes token transfers across every lifecycle call.
/// 2. **Financial Terms & Schedules** (`total_amount`, `advance1_bps`, `advance2_bps`,
///    `claim_window_secs`, `remainder_window_secs`, `delivery_deadline`, `contracted_quantity`,
///    `grade_price_bps`):
///    Smart contracts cannot query historical event logs; every guard, deadline check,
///    and settlement payout equation requires these baseline terms on-chain.
/// 3. **State & Lifecycle Flags** (`status`, `created_at`, `advance1_deadline`,
///    `advance1_claimed`, `advance1_expired`, `advance2_deadline`, `advance2_claimed`,
///    `advance2_expired`, `remainder_deadline`, `remainder_funded`):
///    Enforces single-claim/reclaim semantics, prevents double withdrawals, and gates
///    subsequent state transitions without trusting off-chain assertions.
/// 4. **Attestation & Settlement** (`delivered_quantity`, `grade_index`, `settlement_bps`):
///    Warehouse delivery attestation data consumed by `settle` to calculate proportional payout.
/// 5. **Oracle/FX & Dispute State** (`fx_resolved`, `fx_adjusted_total`, `fx_shortfall_amount`,
///    `fx_shortfall_funded`, `fx_shortfall_deadline`, `fx_quote_stale`, `dispute_pre_status`,
///    `dispute_deadline`): Enforces dispute freeze/recovery and bounds oracle currency
///    adjustments. `fx_quote_stale` (#1) is the one *informational* field — nothing gates
///    on it — but it still belongs on chain: it records the outcome of a cross-contract
///    oracle probe that only a write path can perform (a view getter that called the oracle
///    would make every `get_commitment` poll depend on an external contract's liveness and
///    cost), and it must persist so `get_commitment` can answer "waiting on a fresh quote"
///    without anyone re-running the read.
///
/// Furthermore, `get_commitment` returns this entire struct to off-chain indexers and `api/`.
/// Bitfield compression was evaluated and rejected: packing booleans would break canonical
/// Soroban ScVal ABI compatibility and increase CPU instruction costs for negligible storage gains.
/// Finding: retain struct fields as-is.
#[contracttype]
#[derive(Clone)]
// `PartialEq`/`Eq`/`Debug` are test-only (#267): the dispute round-trip
// property test compares `Commitment` byte-for-byte before `flag_dispute`
// and after `resolve_dispute`/`expire_dispute_window`, which is why the
// derive exists at all. Keeping it behind `cfg(test)` means the deployed
// WASM pays nothing for it — a matching-logic trait that will never be
// invoked at runtime shouldn't widen the on-chain binary (docs/gas-costs.md
// treats every reserved byte as a product cost).
#[cfg_attr(test, derive(PartialEq, Eq, Debug))]
pub struct Commitment {
    pub buyer: Address,
    pub cooperative: Address,
    pub warehouse_operator: Address,
    pub token: Address,
    pub total_amount: i128,
    /// Basis points (of `total_amount`) released at lock-in. 0-10_000.
    pub advance1_bps: u32,
    /// Basis points released at the mid-season checkpoint. 0-10_000.
    /// `advance1_bps + advance2_bps` must not exceed 10_000 — enforced at
    /// `initialize`. Together these two define the *deposit* — the
    /// portion `lock` actually escrows; see module docs.
    pub advance2_bps: u32,
    /// How long the cooperative has to claim an advance once it opens,
    /// in seconds. Same window for both tranches — PRD doesn't call for
    /// different windows per tranche, so one shared value is the simpler
    /// choice until a reason to split them shows up.
    ///
    /// Boundary convention (#111, pinned by `tests/deadline_boundary.rs`):
    /// the deadline is `release time + claim_window_secs`, and *claiming* is
    /// allowed up to **and including** that instant (`timestamp > deadline`
    /// is the rejection guard) while *reclaiming* is allowed only **strictly
    /// after** it (`timestamp <= deadline` is the rejection guard). The two
    /// guards are exact complements, so every ledger timestamp belongs to
    /// exactly one party. Consequence worth knowing: the cooperative's
    /// effective window spans `claim_window_secs + 1` distinct timestamps
    /// (the opening one and the deadline one), not `claim_window_secs`.
    pub claim_window_secs: u64,
    /// How long the buyer has to fund the remainder once
    /// `ready_for_delivery` opens the window. Independent of
    /// `claim_window_secs` — this is a payment deadline, not a claim one.
    pub remainder_window_secs: u64,
    pub status: Status,
    pub created_at: u64,

    /// Absolute deadline (`created_at + delivery_window_secs`), computed
    /// once at `initialize` — unlike the tranche/remainder deadlines,
    /// this one doesn't depend on some other call happening first, so
    /// there's no "0 = unset" state for it.
    ///
    /// **The delivery window starts at `initialize`, not at `lock`**: it is
    /// the cooperative's time to reach `Delivered`, anchored to the moment
    /// the deal was created (`created_at`), and it is therefore already
    /// ticking while the commitment sits in `Draft`. That is deliberate —
    /// the window is the buyer's protection against a cooperative that
    /// never delivers, and re-anchoring it at `lock` would let a buyer
    /// extend it by simply locking late. The complementary guard is on the
    /// other side of that trade-off (#277): `lock` refuses once
    /// `now > delivery_deadline` with
    /// [`Error::LockAfterDeliveryDeadline`], so a buyer can never lock into
    /// a window that has already closed and immediately forfeit the
    /// cooperative's position via `reclaim_on_nondelivery`.
    pub delivery_deadline: u64,

    /// 0 = not yet opened. Once `release_advance_1` runs, this is set to
    /// the ledger timestamp after which the cooperative can no longer
    /// claim and the buyer becomes eligible to reclaim.
    pub advance1_deadline: u64,
    pub advance1_claimed: bool,
    pub advance1_expired: bool,

    pub advance2_deadline: u64,
    pub advance2_claimed: bool,
    pub advance2_expired: bool,

    /// Set by `ready_for_delivery` when the remainder is non-zero and by
    /// `fund_remainder` once the buyer's second deposit lands. **Also set by
    /// `ready_for_delivery` itself when the remainder is zero** (#275): a
    /// deposit covering all of `total_amount` (advance bps summing to 10 000)
    /// leaves nothing to fund, so the window opens already satisfied and the
    /// buyer can't be defaulted for skipping a call that moves no tokens.
    pub remainder_deadline: u64,
    pub remainder_funded: bool,

    /// The quantity (caller-defined unit, e.g. kg) the buyer is
    /// contracting for. Set at `initialize`, never changes. The
    /// denominator for `confirm_delivery`'s proportional shortfall
    /// adjustment (PRD §7).
    pub contracted_quantity: u32,
    /// Pre-agreed grade -> price-multiplier table, in basis points of
    /// `total_amount`'s implied unit price (10_000 = full price), set at
    /// `initialize` and never changed — "pre-agreed," per the PRD, means
    /// agreed before delivery, not decided at settlement time. Ordered by
    /// the caller; `confirm_delivery`'s `grade_index` selects an entry.
    /// Each entry must be <= 10_000 (a grade can't be worth more than the
    /// full agreed unit price) and there must be at least one entry.
    pub grade_price_bps: Vec<u32>,

    /// Set by `confirm_delivery`. 0 until then (also a legitimate real
    /// value — total crop failure — but nothing reads this field before
    /// `Status::Delivered`, so no separate sentinel is needed).
    pub delivered_quantity: u32,
    /// Set by `confirm_delivery` — which `grade_price_bps` entry the
    /// warehouse operator attested.
    pub grade_index: u32,
    /// Set by `confirm_delivery`: the combined quantity x grade
    /// multiplier, in basis points of `total_amount`, that `settle` pays
    /// out against. `min(delivered_quantity, contracted_quantity) /
    /// contracted_quantity * grade_price_bps[grade_index]` — capped at
    /// 10_000 by construction (over-delivery isn't paid extra in v1; see
    /// `confirm_delivery`'s doc comment).
    pub settlement_bps: u32,

    /// Set by `resolve_fx_shortfall`, only meaningful when `oracle_config`
    /// is `Some`. `settle` uses this instead of re-deriving from
    /// `total_amount` whenever an oracle is configured — see
    /// `resolve_fx_shortfall`'s doc comment.
    pub fx_resolved: bool,
    /// `oracle_config.denominated_amount`, adjusted by `settlement_bps`
    /// and converted to the settlement token at the fresh rate
    /// `resolve_fx_shortfall` read. 0 until resolved (also what it'd be
    /// for a total write-off, which is why `fx_resolved` is the actual
    /// "has this run" signal, not this field being nonzero).
    pub fx_adjusted_total: i128,
    /// The price and timestamp of the Reflector quote recorded at
    /// `confirm_delivery` — if an oracle is configured, `resolve_fx_shortfall`
    /// uses this recorded snapshot instead of reading a live quote, fixing
    /// the rate at the moment the delivery was attested (PRD §4.2 option (b)
    /// first-caller fix). Set only when `confirm_delivery` runs with an
    /// oracle configured; zero-valued when no oracle was in effect.
    pub fx_recorded_price: i128,
    pub fx_recorded_timestamp: u64,
    /// How much more the buyer owes beyond what's already escrowed, per
    /// `resolve_fx_shortfall`'s comparison. 0 means no top-up needed —
    /// `settle` can run without waiting on `fund_fx_shortfall` at all.
    pub fx_shortfall_amount: i128,
    pub fx_shortfall_funded: bool,
    /// 0 if `fx_shortfall_amount` is 0 (no deadline needed). Reuses
    /// `remainder_window_secs` as the window length rather than adding
    /// yet another `initialize` parameter for a second deadline serving
    /// the same "give the buyer a fair chance to act" purpose.
    pub fx_shortfall_deadline: u64,

    /// Set by `refresh_fx_quote_status` (#1): the most recent oracle-freshness
    /// probe found a quote older than `oracle_config.max_age_secs`, i.e.
    /// `resolve_fx_shortfall` is currently blocked on Reflector publishing a
    /// fresh one. Cleared by the next probe that finds a fresh quote (and by
    /// a successful `resolve_fx_shortfall`, so it can never linger once the
    /// one-time resolution has run). **Informational only** — no guard reads
    /// it; see `refresh_fx_quote_status`'s doc comment for why this is a
    /// flag alongside `fx_resolved` rather than a distinct `Status`.
    pub fx_quote_stale: bool,

    /// Set by `flag_dispute`, the status to restore on `resolve_dispute`
    /// or `expire_dispute_window`. `Status::Draft` whenever `status`
    /// isn't `Disputed` — a safe "unset" sentinel, not a real possible
    /// value, since `flag_dispute` never accepts `Draft` as a status to
    /// dispute *from*. (Not `Option<Status>`: the `contracttype` macro's
    /// generated `ScVal` conversion doesn't satisfy the blanket `Option`
    /// impl for a unit-variant-only enum like `Status`, unlike a struct
    /// such as `OracleConfig` — confirmed by trying it first.)
    pub dispute_pre_status: Status,
    /// 0 whenever `status` isn't `Disputed`. Reuses `remainder_window_secs`
    /// as the window length, same reasoning as `fx_shortfall_deadline`
    /// above — see `flag_dispute`'s doc comment for why a dispute freeze
    /// needs a bound at all.
    pub dispute_deadline: u64,
    /// Number of lapsed dispute windows accumulated so far. `flag_dispute`
    /// refuses once this reaches `MAX_DISPUTE_FLAGS`. Only
    /// `expire_dispute_window` lapsals increment it; `resolve_dispute`
    /// (unanimous consent) does not — good-faith resolution should not
    /// shorten the window for future disputes.
    pub dispute_count: u32,
}

/// The invariants the state machine is allowed to rely on (#225), every one
/// of which previously lived only in a field doc comment above.
///
/// Test-only by construction: the whole block is `cfg`'d to `test` /
/// `testutils`, and `save` calls it before every write, so the hundreds of
/// existing tests each double as an invariant test on every state write —
/// including `initialize`'s, which writes through `save` too. It is absent
/// from the release WASM (neither cfg is set there), so the deployed
/// contract pays nothing for it.
///
/// Panicking rather than returning an error is deliberate: a violation means
/// this contract wrote an impossible state, which is a bug here, not a
/// caller mistake to report to a caller. A panic makes every test that
/// reaches the bad write fail, whether or not it knows the invariant exists.
#[cfg(any(test, feature = "testutils"))]
impl Commitment {
    /// No auth required — test-build-only check of in-memory state: it calls
    /// nothing, writes nothing, and has no caller-supplied input. Panics (see
    /// this impl's doc) rather than returning an error.
    pub fn check_invariants(&self) {
        // A tranche is resolved at most once, and in one direction.
        assert!(
            !(self.advance1_claimed && self.advance1_expired),
            "invariant: advance1_claimed && advance1_expired"
        );
        assert!(
            !(self.advance2_claimed && self.advance2_expired),
            "invariant: advance2_claimed && advance2_expired"
        );
        // "0 = not yet opened": neither flag can be set before the window
        // that sets it exists.
        if self.advance1_deadline == 0 {
            assert!(
                !self.advance1_claimed && !self.advance1_expired,
                "invariant: advance1 flags set with advance1_deadline == 0"
            );
        }
        if self.advance2_deadline == 0 {
            assert!(
                !self.advance2_claimed && !self.advance2_expired,
                "invariant: advance2 flags set with advance2_deadline == 0"
            );
        }
        // The commitment is frozen iff a dispute window is open, and the
        // pre-dispute status is only a real value while it is.
        assert!(
            (self.status == Status::Disputed) == (self.dispute_deadline != 0),
            "invariant: status == Disputed iff dispute_deadline != 0"
        );
        if self.status == Status::Disputed {
            // `flag_dispute` never accepts `Draft`, so `Draft` can only be
            // the unset sentinel — never a real restore target.
            assert!(
                self.dispute_pre_status != Status::Draft,
                "invariant: dispute_pre_status == Draft while Disputed"
            );
        } else {
            assert!(
                self.dispute_pre_status == Status::Draft,
                "invariant: dispute_pre_status != Draft outside Disputed"
            );
        }
        // The remainder can only be funded inside a window that was opened.
        if self.remainder_funded {
            assert!(
                self.remainder_deadline != 0,
                "invariant: remainder_funded with remainder_deadline == 0"
            );
        }
        // #275: a deposit that already covers `total_amount` leaves nothing
        // for `fund_remainder` to escrow, so the remainder window opens
        // already satisfied — there is no state where a zero remainder is
        // unfunded and the buyer can be defaulted over it.
        if self.remainder_deadline != 0 && EscrowContract::remainder_amount(self) == 0 {
            assert!(
                self.remainder_funded,
                "invariant: zero remainder left unfunded with a window open"
            );
        }
        // A shortfall exists iff it has a deadline to cure it by.
        assert!(
            (self.fx_shortfall_amount > 0) == (self.fx_shortfall_deadline != 0),
            "invariant: fx_shortfall_amount > 0 iff fx_shortfall_deadline != 0"
        );
        // Settlement cannot owe more than the agreed full value.
        assert!(
            self.settlement_bps <= 10_000,
            "invariant: settlement_bps > 10_000"
        );
        // The delivery window is opened at creation, never before it.
        assert!(
            self.delivery_deadline >= self.created_at,
            "invariant: delivery_deadline < created_at"
        );
    }
}

/// The version of the [`CommitmentView`] read shape.
///
/// Bumped whenever a field is added, removed, renamed, or changes meaning —
/// see `docs/versioning.md` §2. Storage-only changes to [`Commitment`] do
/// **not** bump it; that is the whole point of the view.
///
/// **Version 2 (#270)**: `terms.delivery_deadline` became
/// `terms.delivery_window_secs`, so the agreed-terms struct is literally
/// the same type `initialize` accepts (named fields in both directions)
/// instead of two look-alike shapes. A reader written against version 1
/// must recompute the absolute deadline as `created_at +
/// terms.delivery_window_secs`; that is the only field whose meaning
/// moved, and every other field kept its name and meaning.
pub const COMMITMENT_VIEW_SCHEMA_VERSION: u32 = 2;

/// The three named parties plus the settlement token.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentParties {
    pub buyer: Address,
    pub cooperative: Address,
    pub warehouse_operator: Address,
    pub token: Address,
}

/// The deal's agreed terms and windows, fixed at `initialize`.
///
/// **The one definition of "the terms" (#270).** `initialize` takes this
/// struct instead of a run of eight positional arguments, and
/// `get_commitment_view` returns the same struct as `terms` — so the
/// generated bindings, the read model, and `docs/0002-interface-spec.md`
/// all name the same fields. That is the whole point: `advance1_bps` and
/// `advance2_bps` are adjacent `u32`s, and `claim_window_secs`,
/// `remainder_window_secs` and `delivery_window_secs` are adjacent `u64`s,
/// so a positional swap type-checked and silently produced a different
/// deal. A named field cannot be swapped silently, and generated clients
/// carry the names through.
///
/// `delivery_window_secs` is the *window length*, not the absolute
/// deadline: `initialize` computes `delivery_deadline = created_at +
/// delivery_window_secs` once and stores that absolute value in
/// [`Commitment`]. The view reconstructs the window as
/// `delivery_deadline - created_at`, which is exactly the value passed
/// here, so the read shape reports the *agreed term* rather than a
/// clock-derived one. Every other field is stored verbatim.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentTerms {
    pub total_amount: i128,
    /// Basis points of `total_amount` released at lock-in. 0-10_000, and
    /// `advance1_bps + advance2_bps <= 10_000`.
    pub advance1_bps: u32,
    /// Basis points released at the mid-season checkpoint. Together with
    /// `advance1_bps` this defines the deposit `lock` escrows.
    pub advance2_bps: u32,
    /// How long the cooperative has to claim an advance once it opens.
    pub claim_window_secs: u64,
    /// How long the buyer has to fund the remainder once
    /// `ready_for_delivery` opens the window.
    pub remainder_window_secs: u64,
    /// How long after `created_at` the cooperative has to reach
    /// `Delivered`. Must be > 0.
    pub delivery_window_secs: u64,
    /// The quantity the buyer is contracting for, in caller-defined units.
    pub contracted_quantity: u32,
    /// Pre-agreed grade -> price multiplier table, in basis points
    /// (10_000 = full price). Non-empty, at most `MAX_GRADE_ENTRIES`
    /// entries, every entry <= 10_000.
    pub grade_price_bps: Vec<u32>,
}

/// One advance tranche's claim state. `deadline == 0` means not opened yet.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrancheView {
    pub deadline: u64,
    pub claimed: bool,
    pub expired: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentTranches {
    pub advance1: TrancheView,
    pub advance2: TrancheView,
}

/// Two-phase funding state: the deposit is escrowed by `lock`, the remainder
/// by `fund_remainder` once `ready_for_delivery` opens its window.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentFunding {
    pub remainder_deadline: u64,
    pub remainder_funded: bool,
}

/// The warehouse operator's delivery attestation, consumed by `settle`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentDelivery {
    pub delivered_quantity: u32,
    pub grade_index: u32,
    pub settlement_bps: u32,
}

/// PRD §4.2 option (b)'s FX-resolution state. All fields are zero/false for a
/// commitment initialized without an `oracle_config`; `configured` tells the
/// two apart without a second storage read.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentFx {
    pub configured: bool,
    pub resolved: bool,
    pub adjusted_total: i128,
    pub shortfall_amount: i128,
    pub shortfall_funded: bool,
    pub shortfall_deadline: u64,
    pub quote_stale: bool,
}

/// Dispute freeze state; `pre_status` is `Draft` when not disputed.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentDispute {
    pub pre_status: Status,
    pub deadline: u64,
}

/// A stable, versioned read model for a commitment (#256).
///
/// [`Commitment`] is the storage struct: every field exists because some guard
/// or payout needs it on chain, and its shape is free to change for storage
/// reasons. Off-chain clients read this view instead, so a storage-layout
/// change does not rewrite their ABI.
///
/// **Stability promise.** Fields are only added, or their meaning changed, in
/// a release that increments [`COMMITMENT_VIEW_SCHEMA_VERSION`]; readers
/// should refuse a `schema_version` newer than they understand and may assume
/// that a `schema_version` they do understand keeps the same field meanings.
/// The storage struct can change without a version bump — only this view's
/// shape is contractual.
///
/// The grouping is semantic, not storage order: `parties` (who), `terms`
/// (agreed before funding), `tranches` (advance claim state), `funding`
/// (two-phase payment), `delivery` (the operator's attestation), `fx` (oracle
/// conversion), and `dispute` (the bounded freeze). `status` and `created_at`
/// stay top-level because they describe the commitment as a whole.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentView {
    pub schema_version: u32,
    pub status: Status,
    pub created_at: u64,
    pub parties: CommitmentParties,
    pub terms: CommitmentTerms,
    pub tranches: CommitmentTranches,
    pub funding: CommitmentFunding,
    pub delivery: CommitmentDelivery,
    pub fx: CommitmentFx,
    pub dispute: CommitmentDispute,
}

impl CommitmentView {
    /// Projects the storage struct into the read model: every `Commitment`
    /// field appears exactly once, unchanged. The one extra field,
    /// `fx.configured`, is not storage state of its own — it reports whether
    /// the `DataKey::OracleConfig` entry exists. The view is a reshaping, not
    /// a second source of truth. Kept private: the only supported construction
    /// path is the contract's own `get_commitment_view`, so no client can
    /// build a view that disagrees with stored state.
    fn from_commitment(c: &Commitment, oracle_configured: bool) -> Self {
        Self {
            schema_version: COMMITMENT_VIEW_SCHEMA_VERSION,
            status: c.status,
            created_at: c.created_at,
            parties: CommitmentParties {
                buyer: c.buyer.clone(),
                cooperative: c.cooperative.clone(),
                warehouse_operator: c.warehouse_operator.clone(),
                token: c.token.clone(),
            },
            terms: CommitmentTerms {
                total_amount: c.total_amount,
                advance1_bps: c.advance1_bps,
                advance2_bps: c.advance2_bps,
                claim_window_secs: c.claim_window_secs,
                remainder_window_secs: c.remainder_window_secs,
                // The stored field is the absolute deadline `initialize`
                // computed; subtract `created_at` to report the agreed
                // window itself. `check_invariants` guarantees
                // `delivery_deadline >= created_at`, so this cannot
                // underflow.
                delivery_window_secs: c.delivery_deadline - c.created_at,
                contracted_quantity: c.contracted_quantity,
                grade_price_bps: c.grade_price_bps.clone(),
            },
            tranches: CommitmentTranches {
                advance1: TrancheView {
                    deadline: c.advance1_deadline,
                    claimed: c.advance1_claimed,
                    expired: c.advance1_expired,
                },
                advance2: TrancheView {
                    deadline: c.advance2_deadline,
                    claimed: c.advance2_claimed,
                    expired: c.advance2_expired,
                },
            },
            funding: CommitmentFunding {
                remainder_deadline: c.remainder_deadline,
                remainder_funded: c.remainder_funded,
            },
            delivery: CommitmentDelivery {
                delivered_quantity: c.delivered_quantity,
                grade_index: c.grade_index,
                settlement_bps: c.settlement_bps,
            },
            fx: CommitmentFx {
                configured: oracle_configured,
                resolved: c.fx_resolved,
                adjusted_total: c.fx_adjusted_total,
                shortfall_amount: c.fx_shortfall_amount,
                shortfall_funded: c.fx_shortfall_funded,
                shortfall_deadline: c.fx_shortfall_deadline,
                quote_stale: c.fx_quote_stale,
            },
            dispute: CommitmentDispute {
                pre_status: c.dispute_pre_status,
                deadline: c.dispute_deadline,
            },
        }
    }
}

/// Published on every real status transition — `lock`, both advance
/// releases, checkpoint, delivery confirmation, settlement, cancellation,
/// both default paths, and the dispute freeze/resume pair. Every emission
/// point is `set_status` (the only place `status` is assigned after
/// `initialize`), so a future transition gets an event by construction
/// rather than by remembering to add one.
///
/// Both statuses ride as topics, in the shape SEP-41's `transfer` uses
/// (indexed dimensions first, payload second): an off-chain consumer can
/// topic-filter for a specific transition — e.g. everything that just
/// became `Defaulted` — instead of fetching every event and decoding data.
/// `initialize` deliberately doesn't emit: a commitment being created in
/// `Draft` is an initial state, not a transition into one.
///
/// Added for #69, which asked whether an indexer would benefit: none
/// exists yet, but the API's cache is read-triggered precisely because
/// there is no cheaper way to learn about a state change (see
/// `api/HANDOFF.md`), and this is the primitive that changes that
/// trade-off — everything else that would consume these events is
/// deliberately deferred app-side work, not a reason to withhold the
/// event surface itself.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusChanged {
    #[topic]
    pub from: Status,
    #[topic]
    pub to: Status,
}

/// Emitted after every fund transfer in a terminal or payout path —
/// `settle` (once per non-zero leg), `cancel`, `expire_remainder_window`,
/// `reclaim_on_nondelivery`, and `expire_fx_shortfall_window`.
/// `reason` is a short symbol naming the call so an indexer can reconstruct
/// each party's final balance from escrow events alone (#204).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Payout {
    #[topic]
    pub reason: Symbol,
    pub to: Address,
    pub amount: i128,
}

/// Emitted by `claim_advance_1` / `claim_advance_2` after a tranche is
/// transferred to the cooperative. `tranche` is 1 or 2 (#202).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrancheClaimed {
    #[topic]
    pub tranche: u32,
    pub to: Address,
    pub amount: i128,
}

/// Emitted by `reclaim_advance_1` / `reclaim_advance_2` after a lapsed
/// tranche is returned to the buyer (#202).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrancheReclaimed {
    #[topic]
    pub tranche: u32,
    pub to: Address,
    pub amount: i128,
}

/// Emitted by `fund_remainder` after the buyer's second deposit lands (#202).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemainderFunded {
    pub from: Address,
    pub amount: i128,
}

/// Emitted by `fund_fx_shortfall` after the buyer's FX top-up lands (#202).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxShortfallFunded {
    pub from: Address,
    pub amount: i128,
}

/// Emitted by `reassign_buyer` after the buyer position is novated to `new_buyer` (#206).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuyerReassigned {
    #[topic]
    pub previous: Address,
    #[topic]
    pub new_buyer: Address,
}

/// Whether a dispute ended by unanimous consent or by the window expiring (#208).
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisputeOutcome {
    Resolved,
    Expired,
}

/// Emitted by `flag_dispute` when a named party freezes the commitment (#208).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeFlagged {
    #[topic]
    pub flagger: Address,
    pub pre_status: Status,
    pub deadline: u64,
}

/// Emitted by `resolve_dispute` and `expire_dispute_window` when a dispute ends (#208).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeEnded {
    #[topic]
    pub outcome: DisputeOutcome,
    pub restored: Status,
}

/// Emitted by `resolve_fx_shortfall` after the FX shortfall amount is computed (#210).
/// `deadline` is 0 when `shortfall` is 0 (no top-up window needed).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxResolved {
    pub rate: i128,
    pub rate_timestamp: u64,
    pub fx_adjusted_total: i128,
    pub shortfall: i128,
    pub deadline: u64,
}

/// Emitted by `refresh_fx_quote_status` when `fx_quote_stale` changes (#210).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxQuoteStaleChanged {
    pub stale: bool,
}

/// Emitted by `confirm_delivery` after the operator's attestation is recorded (#212).
/// The three fields together are the trust anchor for every downstream
/// calculation: `settlement_bps` is derived from `delivered_quantity` ×
/// `grade_price_bps[grade_index]` / `contracted_quantity`, computed once and
/// never re-derived. Per-member hashes stay out of this event — see
/// `docs/allocation-salt-analysis.md` for why member identity belongs
/// off-chain.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryConfirmed {
    #[topic]
    pub grade_index: u32,
    pub delivered_quantity: u32,
    pub settlement_bps: u32,
}

/// Emitted by `set_allocation` after the member entitlement ledger is
/// recorded (#212). Reports the aggregate shape (member count and total
/// share basis points) without exposing individual hashes, so an indexer
/// can verify completeness without decoding the full allocation list.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationRecorded {
    pub member_count: u32,
    pub total_share_bps: u32,
}

/// Emitted by `replace_oracle_config` when both parties consent to swap the
/// oracle address (#215). The old config is not stored after replacement;
/// subscribers that need it can read it from this event's context, via the
/// previous `initialize` transaction's arguments.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleConfigReplaced {
    pub new_oracle_contract: Address,
}

/// The pre-delivery range: every status `cancel` and `reassign_buyer`
/// accept. Past `Delivered` the buyer no longer gates any remaining action,
/// so moving a position or unwinding the deal there would be a no-op
/// dressed up as a real transfer — see those functions' doc comments.
const PRE_DELIVERY: &[Status] = &[
    Status::Draft,
    Status::Locked,
    Status::Advance1Released,
    Status::CheckpointPassed,
    Status::Advance2Released,
    Status::ReadyForDelivery,
];

/// The range with funds already at stake or a claim outstanding, but no
/// delivery attested yet: the two seller-failure paths
/// (`reclaim_on_nondelivery`, `expire_delivery_window`) are reachable
/// exactly here, and `Draft` is deliberately excluded (nothing was ever
/// escrowed at that point).
const LOCKED_THROUGH_READY: &[Status] = &[
    Status::Locked,
    Status::Advance1Released,
    Status::CheckpointPassed,
    Status::Advance2Released,
    Status::ReadyForDelivery,
];

/// The dispute window: everything the three named parties can freeze — from
/// the first status with money escrowed (`Locked`) through the operator's
/// delivery attestation (`Delivered`). Not `Draft` (nothing to freeze), not
/// any terminal status, and not `Disputed` itself (a dispute can't be
/// re-flagged while it is already open).
const LOCKED_THROUGH_DELIVERED: &[Status] = &[
    Status::Locked,
    Status::Advance1Released,
    Status::CheckpointPassed,
    Status::Advance2Released,
    Status::ReadyForDelivery,
    Status::Delivered,
];

/// One status-gated public method — the key into the declared transition
/// table, [`Self::allowed_from`].
///
/// This enum plus that one function **is** the state machine's source of
/// truth (#227). Every `can_*` guard in this file begins with
/// `Self::require_status(c, Method::X)`, so a method's accepted source
/// statuses are written exactly once; before this table existed, the same
/// `Locked..=ReadyForDelivery` range was hand-copied into four different
/// guard functions and `Disputed`-is-never-accepted was implied by a dozen
/// separate lists that all had to stay in sync. Adding a call means adding
/// a row here, and a status that admits nothing can be read off the table
/// directly.
///
/// Two rows (`ClaimAdvance1`/`ClaimAdvance2`, `ReclaimAdvance1`/
/// `ReclaimAdvance2`) declare **no** status gate, and that is deliberate,
/// not an omission: an advance window, once opened, stays claimable (or
/// reclaimable) while `advanceN_deadline` is open regardless of what the
/// deal's status has since moved on to — crop progress doesn't wait on
/// paperwork, and a dispute freeze deliberately does not stop a tranche
/// window that is already ticking (pinned by
/// `claim_advance_1_while_disputed_still_pays` and
/// `reclaim_advance_1_while_disputed_still_refunds`). Their real guards are
/// the deadline/flag predicates in `can_claim_tranche`/
/// `can_reclaim_tranche`, which is why the table says `None` rather than
/// listing every status.
#[derive(Clone, Copy)]
enum Method {
    SetAllocation,
    Lock,
    ExtendDeliveryDeadline,
    ReleaseAdvance1,
    ClaimAdvance1,
    ReclaimAdvance1,
    MarkCheckpoint,
    ReleaseAdvance2,
    ClaimAdvance2,
    ReclaimAdvance2,
    ReadyForDelivery,
    FundRemainder,
    ExpireRemainderWindow,
    ReclaimOnNondelivery,
    ExpireDeliveryWindow,
    ConfirmDelivery,
    Settle,
    ResolveFxShortfall,
    RefreshFxQuoteStatus,
    ReplaceOracleConfig,
    FundFxShortfall,
    ExpireFxShortfallWindow,
    Cancel,
    ReassignBuyer,
    FlagDispute,
    ResolveDispute,
    ExpireDisputeWindow,
}

impl Method {
    /// The declared transition table: for each method, the source statuses
    /// it accepts. `None` means "this method has no status gate at all"
    /// (see the enum's doc for the two tranche pairs, and why).
    ///
    /// An empty list is never used: a method reachable from nowhere is far
    /// more likely to be a table mistake than a real state, and is
    /// impossible to exercise.
    const fn allowed_from(self) -> Option<&'static [Status]> {
        match self {
            Method::SetAllocation | Method::Lock => Some(&[Status::Draft]),
            Method::ExtendDeliveryDeadline => Some(LOCKED_THROUGH_READY),
            Method::ReleaseAdvance1 => Some(&[Status::Locked]),
            Method::MarkCheckpoint => Some(&[Status::Advance1Released]),
            Method::ReleaseAdvance2 => Some(&[Status::CheckpointPassed]),
            Method::ReadyForDelivery => Some(&[Status::Advance2Released]),
            Method::ClaimAdvance1
            | Method::ReclaimAdvance1
            | Method::ClaimAdvance2
            | Method::ReclaimAdvance2 => None,
            Method::FundRemainder | Method::ExpireRemainderWindow | Method::ConfirmDelivery => {
                Some(&[Status::ReadyForDelivery])
            }
            Method::ReclaimOnNondelivery | Method::ExpireDeliveryWindow => {
                Some(LOCKED_THROUGH_READY)
            }
            Method::Settle
            | Method::ResolveFxShortfall
            | Method::RefreshFxQuoteStatus
            | Method::ReplaceOracleConfig
            | Method::FundFxShortfall
            | Method::ExpireFxShortfallWindow => Some(&[Status::Delivered]),
            Method::Cancel | Method::ReassignBuyer => Some(PRE_DELIVERY),
            Method::FlagDispute => Some(LOCKED_THROUGH_DELIVERED),
            Method::ResolveDispute | Method::ExpireDisputeWindow => Some(&[Status::Disputed]),
        }
    }
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Creates the commitment in `Draft`. No funds move yet.
    ///
    /// Requires the buyer's auth because they're the one committing to a
    /// future deposit — the cooperative and warehouse operator don't need
    /// to co-sign contract creation itself, only the state transitions
    /// that matter to them (`mark_checkpoint`, `confirm_delivery`).
    ///
    /// The three party addresses are *not* required to be distinct, on
    /// purpose. Role separation is enforced where it matters — at each
    /// state transition, by that role's own `require_auth()` — rather than
    /// by a distinctness check here. Collapsing buyer, cooperative and
    /// warehouse onto one principal is a legitimate deployment choice (a
    /// cooperative that also runs the warehouse, or a minimal single-signer
    /// validation deployment); `initialize` records the parties it is handed
    /// and does not second-guess them. The consequence is accepted, not
    /// hidden: with one principal in all three roles every multi-party
    /// consent gate reduces to that single signer. See the tests
    /// `initialize_allows_buyer_cooperative_and_warehouse_to_be_one_address`
    /// and `all_three_identical_parties_accept_only_that_principals_signature`.
    ///
    /// `grade_price_bps` must be non-empty, may hold at most
    /// `MAX_GRADE_ENTRIES` entries, and every entry must be ≤ 10_000.
    ///
    /// **`token` must have standard SEP-41 transfer semantics — ideally a
    /// Stellar Asset Contract (SAC).** The contract calls `balance()` and
    /// `transfer()` on `token` and relies on both to behave as SEP-41
    /// specifies: `transfer(amount)` moves exactly `amount` from sender
    /// to receiver with no fee skim, and `balance(id)` reports what `id`
    /// actually holds. Non-standard tokens that implement the SEP-41
    /// interface but misbehave — fee-on-transfer, lying `balance()` — do
    /// not corrupt the escrow's accounting (`settle`/`cancel` either leave
    /// money with the wrong party or trap mid-call, never over-pay), but
    /// they do harm the cooperative, who did not pick the address. See
    /// threat-model entry 30's sub-case and the pinned behavior in
    /// `contracts/escrow/tests/nonstandard_token.rs`. The real HarvestLock
    /// API deployments only ever pass a SAC (USDC testnet / USDC
    /// mainnet); anything calling `initialize` directly (CLI, tests, a
    /// future direct caller) must honor this requirement itself.
    ///
    /// **A commitment abandoned at Draft forever (#96)**: this call moves
    /// no funds, and `lock` is the first and only place in the whole
    /// contract where funds can enter — every token transfer into the
    /// contract happens inside `lock` (the deposit) or `fund_remainder` /
    /// `fund_fx_shortfall` (both unreachable before `lock`, since their
    /// status guards require states only `lock` can produce). So for a
    /// commitment that is deployed, initialized, and then simply never
    /// touched again while still `Draft`: **there are no funds at stake to
    /// reclaim, because none ever left the buyer.** The contract's balance
    /// of `token` is provably zero — the buyer never approved or sent
    /// anything, and a `balance()` call on the token contract for this
    /// address confirms it independently of this contract's own state. The
    /// formal unwind does exist even at this stage — `cancel` is reachable
    /// from `Draft` and refunds the contract's whole balance to the buyer,
    /// which here refunds zero and just records the `Cancelled`
    /// transition — but it needs both the buyer's and the cooperative's
    /// consent, and consent is not needed to *protect* anyone: with no
    /// funds escrowed there is nothing for a reclamation path to reclaim.
    /// `reclaim_on_nondelivery` deliberately refuses `Draft` for the same
    /// reason. The only thing an abandoned Draft leaves behind is its
    /// storage entry, whose lifetime (and eventual archival) is covered by
    /// docs/storage-ttl.md — a dead record, not lost money.
    ///
    /// **Signature (#270)**: the eight numeric/list terms travel as one
    /// named [`CommitmentTerms`] struct rather than eight positional
    /// parameters, so a swapped `advance1_bps`/`advance2_bps` — or
    /// `claim_window_secs`/`remainder_window_secs` — can no longer
    /// type-check, and generated clients name every field. The parties and
    /// the optional `oracle_config` stay standalone arguments: an
    /// `Address` cannot be confused for another `Address` by position in
    /// the way adjacent `u32`/`u64` terms can, and `OracleConfig` is a
    /// separate optional concern, not an agreed term. Per
    /// docs/versioning.md §2 this is a MINOR **BREAKING** change; the
    /// migration is one call-site reshape, with validation and stored
    /// state identical to before.
    pub fn initialize(
        env: Env,
        buyer: Address,
        cooperative: Address,
        warehouse_operator: Address,
        token: Address,
        terms: CommitmentTerms,
        // Some if this deal needs the oracle_rate conversion path — see
        // the module doc. None for a plain deal.
        oracle_config: Option<OracleConfig>,
    ) -> Result<(), Error> {
        // Destructured once, so every guard and the stored struct below
        // read exactly the names the positional parameters used to have.
        // The validation order is unchanged.
        let CommitmentTerms {
            total_amount,
            advance1_bps,
            advance2_bps,
            claim_window_secs,
            remainder_window_secs,
            delivery_window_secs,
            contracted_quantity,
            grade_price_bps,
        } = terms;
        if env.storage().instance().has(&DataKey::Commitment) {
            return Err(Error::AlreadyInitialized);
        }
        if total_amount <= 0 {
            return Err(Error::ZeroAmount);
        }
        // Upper bound first, before any bps-dependent arithmetic: see
        // `MAX_TOTAL_AMOUNT` and `Error::AmountTooLarge` (#249).
        if total_amount > MAX_TOTAL_AMOUNT {
            return Err(Error::AmountTooLarge);
        }
        if advance1_bps.saturating_add(advance2_bps) > 10_000 {
            return Err(Error::InvalidBps);
        }
        if claim_window_secs == 0 || remainder_window_secs == 0 || delivery_window_secs == 0 {
            return Err(Error::InvalidWindow);
        }
        if contracted_quantity == 0 {
            return Err(Error::InvalidQuantity);
        }
        if grade_price_bps.is_empty() || grade_price_bps.iter().any(|bps| bps > 10_000) {
            return Err(Error::InvalidGradeSchedule);
        }
        if grade_price_bps.len() > MAX_GRADE_ENTRIES {
            return Err(Error::GradeScheduleTooLong);
        }
        if let Some(oc) = &oracle_config {
            if oc.max_age_secs == 0 {
                return Err(Error::InvalidOracleConfig);
            }
            // #247: `denominated_amount` is what decides the cooperative's
            // payout on an oracle deal (`resolve_fx_shortfall` reprices it),
            // so it gets the same "must be positive" bar `total_amount` gets
            // above. Zero or negative would make `fx_adjusted_total <= 0`,
            // clamp `owed_from_remainder` to 0, and let `settle` refund the
            // whole escrow to the buyer after a full delivery. The upper
            // bound rejects any value for which `bps_amount(_, 10_000)` —
            // the largest multiplier `resolve_fx_shortfall` can apply, since
            // `settlement_bps <= 10_000` — would overflow `i128` and trap at
            // settlement instead of being refused here.
            if oc.denominated_amount <= 0 || oc.denominated_amount > MAX_DENOMINATED_AMOUNT {
                return Err(Error::InvalidOracleConfig);
            }
        }
        buyer.require_auth();

        let created_at = env.ledger().timestamp();
        let commitment = Commitment {
            buyer,
            cooperative,
            warehouse_operator,
            token,
            total_amount,
            advance1_bps,
            advance2_bps,
            claim_window_secs,
            remainder_window_secs,
            status: Status::Draft,
            created_at,
            delivery_deadline: created_at + delivery_window_secs,
            advance1_deadline: 0,
            advance1_claimed: false,
            advance1_expired: false,
            advance2_deadline: 0,
            advance2_claimed: false,
            advance2_expired: false,
            remainder_deadline: 0,
            remainder_funded: false,
            contracted_quantity,
            grade_price_bps,
            delivered_quantity: 0,
            grade_index: 0,
            settlement_bps: 0,
            fx_resolved: false,
            fx_adjusted_total: 0,
            fx_shortfall_amount: 0,
            fx_shortfall_funded: false,
            fx_shortfall_deadline: 0,
            fx_quote_stale: false,
            fx_recorded_price: 0,
            fx_recorded_timestamp: 0,
            dispute_pre_status: Status::Draft,
            dispute_deadline: 0,
            dispute_count: 0,
        };
        // Through `save` like every other write, so the initial state is
        // covered by the same test-build invariant checker (#225) — the
        // `Draft` state it writes is the one every later invariant is
        // relative to.
        Self::save(&env, &commitment);
        if let Some(oc) = oracle_config {
            env.storage().instance().set(&DataKey::OracleConfig, &oc);
        }
        // `initialize` is the only public entry point that doesn't read
        // through `load` (there's nothing to load yet), so it extends the
        // instance's TTL directly — the entry otherwise carries only
        // whatever lifetime `deploy` gave it (#98, see `bump_instance_ttl`).
        Self::bump_instance_ttl(&env);
        Ok(())
    }

    /// The configured oracle, if `initialize` set one.
    ///
    /// No auth required — read-only view, callable by anyone.
    pub fn get_oracle_config(env: Env) -> Result<Option<OracleConfig>, Error> {
        let _ = Self::load(&env)?; // NotInitialized if the commitment itself doesn't exist
        Ok(env.storage().instance().get(&DataKey::OracleConfig))
    }

    /// Replaces a broken oracle configuration with a new one, with two-party
    /// consent (issue #215). Addresses F1 (unreachable oracle) by letting the
    /// buyer and cooperative substitute a working `oracle_config` before
    /// `fx_resolved` is set.
    ///
    /// Only legal in `Status::Delivered` while `fx_resolved` is false.
    /// The new config is validated the same way `initialize` does
    /// (`max_age_secs != 0`). Requires buyer and cooperative to co-sign.
    pub fn replace_oracle_config(env: Env, new_config: OracleConfig) -> Result<(), Error> {
        let c = Self::load(&env)?;
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        if c.fx_resolved {
            return Err(Error::FxAlreadyResolved);
        }
        if !env.storage().instance().has(&DataKey::OracleConfig) {
            return Err(Error::OracleNotConfigured);
        }
        if new_config.max_age_secs == 0 {
            return Err(Error::InvalidOracleConfig);
        }
        Self::require_once_each(&[&c.buyer, &c.cooperative]);
        let new_oracle_contract = new_config.oracle_contract.clone();
        env.storage()
            .instance()
            .set(&DataKey::OracleConfig, &new_config);
        OracleConfigReplaced {
            new_oracle_contract,
        }
        .publish(&env);
        Ok(())
    }

    /// Reads the current rate for this commitment's configured oracle
    /// asset, enforcing the staleness bound. A pure read against
    /// Reflector's own stored history, not a state transition — callable
    /// in any status, including after `Settled`.
    ///
    /// No auth required — permissionless read.
    pub fn oracle_rate(env: Env) -> Result<OracleRate, Error> {
        let _ = Self::load(&env)?;
        let config: OracleConfig = env
            .storage()
            .instance()
            .get(&DataKey::OracleConfig)
            .ok_or(Error::OracleNotConfigured)?;
        Self::read_oracle_rate(&env, &config)
    }

    /// Shared by the public `oracle_rate` wrapper above and
    /// `resolve_fx_shortfall` below, so the actual Reflector-calling and
    /// staleness-checking logic exists exactly once. Takes `&OracleConfig`
    /// rather than re-reading storage, since `resolve_fx_shortfall`
    /// already has its own copy in hand and needs the rest of it
    /// (`denominated_amount`) right after this call.
    ///
    /// Uses the generated `try_lastprice`, not the panicking `lastprice`
    /// (#245). A missing or crashing oracle, or a reply that doesn't decode
    /// as `Option<PriceData>`, comes back as `Error::OracleUnavailable`
    /// instead of aborting the caller's whole invocation.
    fn read_oracle_rate(env: &Env, config: &OracleConfig) -> Result<OracleRate, Error> {
        let client = ReflectorPulseClient::new(env, &config.oracle_contract);
        let quote =
            match client.try_lastprice(&ReflectorAsset::Other(config.price_asset.clone())) {
                Ok(Ok(quote)) => quote,
                // Outer `Err`: the call itself failed (no contract there, the
                // callee panicked or returned an error). Inner `Err`: it
                // answered, but not with a value of the expected type.
                Ok(Err(_)) | Err(_) => return Err(Error::OracleUnavailable),
            }
            .ok_or(Error::OraclePriceUnavailable)?;
        if quote.price <= 0 {
            return Err(Error::OraclePriceInvalid);
        }

        let now = env.ledger().timestamp();
        if quote.timestamp > now {
            return Err(Error::OracleTimestampInFuture);
        }
        let age = now - quote.timestamp;
        if age > config.max_age_secs {
            return Err(Error::OracleStale);
        }

        Ok(OracleRate {
            price: quote.price,
            timestamp: quote.timestamp,
        })
    }

    /// Cooperative-authored record of member farmers' entitlement shares
    /// — PRD's "member allocation ledger capture at lock-in." **Record-
    /// only in v1**: `settle` still pays the cooperative wallet a lump
    /// sum; this doesn't pro-rate an on-chain payout across members. That
    /// matches PRD §4.9's own stated v1 default (Transparency Ladder
    /// Rung 1 — "Payment settles to the cooperative wallet. Each
    /// member's entitlement is on chain and readable; members receive an
    /// SMS stating their share") — not an open question this contract
    /// deferred, but the documented default. Pro-rated on-chain payout
    /// (Rung 2+) would need real design work (N token transfers instead
    /// of one, rounding-remainder handling) and isn't built.
    ///
    /// One-time and immutable once set — no amend function exists.
    /// `members` must be non-empty, may hold at most
    /// `MAX_ALLOCATION_MEMBERS` entries, and the shares must sum to at
    /// most 10_000 bps.
    /// Deliberately **not** required before `lock`: a solo-farmer
    /// commitment with no cooperative pooling shouldn't be forced through
    /// a ledger step that doesn't apply to it, and gating `lock` on this
    /// would be a real behavior change affecting every existing
    /// commitment shape, not a decision this session is making
    /// unilaterally. Cooperative-gated, not buyer-consented — this is the
    /// cooperative's own membership data, not a term the buyer negotiates.
    ///
    /// **Independence from delivery grading**: the member list and its count
    /// are strictly internal to cooperative entitlement tracking and have no
    /// coupling with `confirm_delivery`'s `grade_index` (which indexes the
    /// quality pricing schedule `grade_price_bps`).
    pub fn set_allocation(env: Env, members: Vec<AllocationMember>) -> Result<(), Error> {
        let c = Self::load(&env)?;
        Self::can_set_allocation(&env, &c)?;
        if members.is_empty() {
            return Err(Error::InvalidAllocation);
        }
        if members.len() > MAX_ALLOCATION_MEMBERS {
            return Err(Error::AllocationTooLarge);
        }
        let mut total_bps: u32 = 0;
        for m in members.iter() {
            total_bps = total_bps.saturating_add(m.share_bps);
        }
        if total_bps > 10_000 {
            return Err(Error::InvalidAllocation);
        }
        c.cooperative.require_auth();

        let member_count = members.len();
        env.storage()
            .persistent()
            .set(&DataKey::Allocation, &members);
        Self::bump_allocation_ttl(&env);
        AllocationRecorded {
            member_count,
            total_share_bps: total_bps,
        }
        .publish(&env);
        Ok(())
    }

    /// The recorded allocation ledger, if `set_allocation` has run.
    ///
    /// No auth required — read-only view, callable by anyone.
    pub fn get_allocation(env: Env) -> Result<Vec<AllocationMember>, Error> {
        let _ = Self::load(&env)?; // NotInitialized if the commitment itself doesn't exist
        env.storage()
            .persistent()
            .get(&DataKey::Allocation)
            .ok_or(Error::AllocationNotSet)
    }

    /// Extends the delivery window by mutual consent while the commitment is
    /// still pre-delivery and the original deadline has not passed. The
    /// deadline is monotonic: a consent call can only add time, never revoke
    /// time that was already promised to the cooperative.
    pub fn extend_delivery_deadline(env: Env, new_deadline: u64) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_extend_delivery_deadline(&c, env.ledger().timestamp(), new_deadline)?;
        Self::require_once_each(&[&c.buyer, &c.cooperative]);
        c.delivery_deadline = new_deadline;
        Self::save(&env, &c);
        Ok(())
    }

    /// Draft -> Locked. Pulls the buyer's **deposit** into the contract —
    /// `advance1_bps + advance2_bps` of `total_amount`, not the full
    /// amount. See module docs for why this changed from earlier versions
    /// of this contract, which escrowed everything here.
    ///
    /// Requires buyer auth.
    ///
    /// **Refused once `delivery_deadline` has strictly passed (#277)**: the
    /// delivery window counts from `initialize`, not from this call (see
    /// `Commitment::delivery_deadline` for why), so locking after it would
    /// buy the buyer a same-second `reclaim_on_nondelivery` and a free
    /// `Forfeited` against a cooperative that never had time to deliver.
    /// The deadline second itself is still in time — `now > delivery_deadline`
    /// is the only rejection (`RemainderWindowPassed`/`ClaimWindowPassed`
    /// use the same inclusive cut-off), so
    /// `lock_exactly_at_the_delivery_deadline_still_succeeds` pins the
    /// boundary from both sides. Rejecting here is the guard half of the
    /// fix; the alternative considered and not taken was re-anchoring
    /// `delivery_deadline` to `max(delivery_deadline, lock time + window)`,
    /// which would have let a buyer extend the cooperative's deadline
    /// unilaterally.
    ///
    /// No prior SEP-41 `approve()` needed — verified live against real
    /// testnet, not assumed. Soroban's single-transaction nested-
    /// authorization model means the buyer's signature on the top-level
    /// `lock` invocation already covers the nested `token::Client::transfer`
    /// call's own `require_auth()` below; this differs from the classic
    /// ERC-20-style approve/transferFrom pattern a reader coming from EVM
    /// might expect. (An earlier version of this comment claimed approve
    /// was required — that was never checked against real behavior; this
    /// replaces it with what was actually verified.)
    pub fn lock(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_lock(&c, env.ledger().timestamp())?;
        c.buyer.require_auth();

        let deposit = Self::deposit_amount(&c);

        // State is updated and saved *before* the external transfer
        // (checks-effects-interactions), so the effect is committed even
        // if the transfer itself later fails: Soroban then rolls the whole
        // invocation back, including this save, so an `Err` still leaves
        // nothing behind.
        //
        // Two layers defend a mid-transfer re-entry, and `tests/reentrancy.rs`
        // pins which one actually applies. On soroban-env-host 29.0.1 the
        // host rejects any call back into a contract already on the call
        // stack ("Contract re-entry is not allowed") *before* this
        // contract's frame runs, so a hostile token's callback is stopped at
        // the host — the saved `Locked` status is never even read. The
        // ordering is retained as defence in depth: if a future host or a
        // different token standard ever allowed that callback through, the
        // re-entrant call would see `Locked` already and be rejected by the
        // guard above instead of pulling the deposit a second time. The
        // standard SAC/native token used here has no callback hook at all;
        // this remains hardening against a future token type, not a response
        // to a demonstrated exploit against the current one.
        Self::set_status(&env, &mut c, Status::Locked);
        Self::save(&env, &c);

        if deposit > 0 {
            token::Client::new(&env, &c.token).transfer(
                &c.buyer,
                env.current_contract_address(),
                &deposit,
            );
        }
        Ok(())
    }

    /// Locked -> Advance1Released. Opens tranche 1's claim window — does
    /// **not** move funds. The cooperative claims via `claim_advance_1`,
    /// or the buyer reclaims via `reclaim_advance_1` after the window
    /// passes unclaimed.
    ///
    /// Deliberately not auth-gated: it starts a clock, it doesn't move
    /// money or grant anyone anything they weren't already entitled to
    /// under the agreed terms.
    ///
    /// Security-reviewed on the record (#2): that reasoning holds under
    /// adversarial timing — a third party can pick the instant the window
    /// opens but gains nothing by it (no funds move, the deadline is
    /// written exactly once so the clock can never be restarted or
    /// extended, the lapse beneficiary stays the buyer, and the
    /// cooperative always gets the full `claim_window_secs` from whenever
    /// the window opened). The full reader audit and the pinned scenarios
    /// live in `docs/permissionless-release-review.md` and
    /// `contracts/escrow/tests/permissionless_release_review.rs`.
    /// Auth-gating would add a timing veto the non-custodial design
    /// deliberately gives nobody (PRD §4.5) while adding a liveness
    /// dependency, not safety.
    pub fn release_advance_1(env: Env) -> Result<(), Error> {
        Self::open_tranche(&env, Tranche::One)
    }

    /// Cooperative claims tranche 1, if within the window. Requires
    /// cooperative auth.
    ///
    /// Boundary (#111): "within" is inclusive of the deadline instant —
    /// `claim_tranche` rejects only when `timestamp > advance1_deadline`, so
    /// `advance1_deadline` itself is still claimable, and it is the last
    /// instant that is. Pinned by `tests/deadline_boundary.rs`.
    pub fn claim_advance_1(env: Env) -> Result<(), Error> {
        Self::claim_tranche(&env, Tranche::One)
    }

    /// Buyer reclaims tranche 1, if the window has passed unclaimed.
    /// Requires buyer auth.
    ///
    /// Boundary (#111): the exact complement of `claim_advance_1` —
    /// `reclaim_tranche` rejects while `timestamp <= advance1_deadline`, so
    /// the deadline instant belongs to the cooperative's claim and the buyer's
    /// reclaim first becomes possible at `advance1_deadline + 1`. No timestamp
    /// ever admits both, and none admits neither.
    pub fn reclaim_advance_1(env: Env) -> Result<(), Error> {
        Self::reclaim_tranche(&env, Tranche::One)
    }

    /// Advance1Released -> CheckpointPassed. Requires the warehouse
    /// operator's attestation — this is a judgment call, not a mechanical
    /// state advance, so unlike opening an advance tranche it *is*
    /// auth-gated. Independent of whether tranche 1 has actually been
    /// claimed yet — crop progress doesn't wait on paperwork.
    pub fn mark_checkpoint(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_mark_checkpoint(&c)?;
        c.warehouse_operator.require_auth();
        Self::set_status(&env, &mut c, Status::CheckpointPassed);
        Self::save(&env, &c);
        Ok(())
    }

    /// CheckpointPassed -> Advance2Released. Same mechanics as
    /// `release_advance_1`, for tranche 2. Permissionless like
    /// `release_advance_1` — deliberately not auth-gated, it only starts
    /// a clock. The #2 security review covers both calls together (they
    /// share `open_tranche`); see `release_advance_1`'s doc comment and
    /// `docs/permissionless-release-review.md`. Note that tranche 2's
    /// clock additionally cannot be pulled early at all: reaching
    /// `CheckpointPassed` requires the warehouse operator's auth-gated
    /// `mark_checkpoint` first.
    pub fn release_advance_2(env: Env) -> Result<(), Error> {
        Self::open_tranche(&env, Tranche::Two)
    }

    /// Cooperative claims tranche 2, if within the window. Requires
    /// cooperative auth — same mechanics as `claim_advance_1`, including its
    /// inclusive `advance2_deadline` boundary (#111).
    pub fn claim_advance_2(env: Env) -> Result<(), Error> {
        Self::claim_tranche(&env, Tranche::Two)
    }

    /// Buyer reclaims tranche 2, if the window has passed unclaimed.
    /// Requires buyer auth — same mechanics as `reclaim_advance_1`, including
    /// the exclusive `advance2_deadline` boundary (#111).
    pub fn reclaim_advance_2(env: Env) -> Result<(), Error> {
        Self::reclaim_tranche(&env, Tranche::Two)
    }

    /// Advance2Released -> ReadyForDelivery. The cooperative signaling
    /// "setting out for delivery" — opens the remainder-payment window
    /// (`remainder_window_secs` from now). Auth-gated, unlike
    /// `release_advance_*`: this one has real stakes for the buyer (it
    /// starts the clock that can end in `Defaulted`), so it shouldn't be
    /// triggerable by just anyone.
    ///
    /// **A zero remainder is funded here, not by a token transfer (#275).**
    /// `lock` escrows `advance1_bps + advance2_bps` of `total_amount` and
    /// `fund_remainder` escrows the rest; when an advance split sums to
    /// exactly 10 000 bps the deposit *is* `total_amount`, so there is no
    /// remainder to send. Requiring `fund_remainder` anyway would let a
    /// fully-prepaid buyer be marked `Defaulted` — and lose the whole
    /// deposit to the cooperative — by not making a call that moves zero
    /// tokens before `remainder_deadline`. So when the remainder is zero,
    /// this call sets `remainder_funded` itself, which keeps
    /// `expire_remainder_window`'s buyer-default (`RemainderAlreadyFunded`)
    /// and `confirm_delivery`'s gate (`RemainderNotFunded`) meaningful: the
    /// first is unreachable, the second already satisfied. No
    /// `RemainderFunded` event is emitted for it, because no funds were
    /// funded — the state is observable in `get_commitment`.
    pub fn ready_for_delivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_ready_for_delivery(&c)?;
        c.cooperative.require_auth();
        c.remainder_deadline = env.ledger().timestamp() + c.remainder_window_secs;
        // #275: nothing left to escrow, so nothing to default on.
        if Self::remainder_amount(&c) == 0 {
            c.remainder_funded = true;
        }
        Self::set_status(&env, &mut c, Status::ReadyForDelivery);
        Self::save(&env, &c);
        Ok(())
    }

    /// Buyer escrows `total_amount - deposit` — the second half of
    /// two-phase funding. Must happen within the window
    /// `ready_for_delivery` opened, or it's a default (see
    /// `expire_remainder_window`). Requires buyer auth — this is new
    /// money the buyer specifically owes.
    ///
    /// The window's end is **inclusive of `remainder_deadline` itself**:
    /// `now > remainder_deadline` is the only rejection, so a payment
    /// landing in the deadline second still counts.
    /// `expire_remainder_window` needs strictly more than the deadline,
    /// which is what keeps the boundary from being ambiguous — the two
    /// calls are never both legal at the same timestamp.
    ///
    /// **A zero remainder is already funded (#275)**: when
    /// `advance1_bps + advance2_bps == 10_000`, `lock` has escrowed the
    /// whole `total_amount`, `ready_for_delivery` marked the remainder
    /// funded on the buyer's behalf, and this call refuses with
    /// `RemainderAlreadyFunded` instead of transferring zero tokens. The
    /// deal can go straight to `confirm_delivery` — no no-op call is
    /// required, and none can be held against the buyer.
    pub fn fund_remainder(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_fund_remainder(&c, env.ledger().timestamp())?;
        c.buyer.require_auth();

        let remainder = Self::remainder_amount(&c);

        // Effects before interaction — see `lock`'s comment for why.
        c.remainder_funded = true;
        Self::save(&env, &c);

        if remainder > 0 {
            token::Client::new(&env, &c.token).transfer(
                &c.buyer,
                env.current_contract_address(),
                &remainder,
            );
        }
        RemainderFunded {
            from: c.buyer.clone(),
            amount: remainder,
        }
        .publish(&env);
        Ok(())
    }

    /// The buyer-default path: `remainder_deadline` has *strictly* passed
    /// (`now > remainder_deadline`) with the remainder never funded — in
    /// the deadline second itself this refuses with
    /// `RemainderWindowNotPassed`, so the buyer's own funding window is
    /// never cut short by a second. Sweeps whatever's currently escrowed
    /// (the deposit, or whatever of it the cooperative hasn't already
    /// claimed) to the cooperative and sets `Defaulted` — uncontested by
    /// construction, so unlike `cancel`/`reassign_buyer` this needs no
    /// consent from anyone.
    ///
    /// Permissionless, deliberately: the outcome (sweep to cooperative)
    /// doesn't depend on who calls it, only on whether the deadline has
    /// passed, the same reasoning `reclaim_tranche` would use if it
    /// weren't already scoped to benefit the buyer specifically. Anyone —
    /// including an off-chain watcher — can trigger it once the fact of
    /// the matter (deadline passed, unfunded) is true.
    ///
    /// **Unreachable for a fully-prepaid commitment (#275)**: when the
    /// advance bps sum to 10 000 there is no remainder, `ready_for_delivery`
    /// marks it funded, and this call refuses with
    /// `RemainderAlreadyFunded` — the buyer can never be defaulted for not
    /// sending zero tokens.
    pub fn expire_remainder_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_expire_remainder_window(&c, env.ledger().timestamp())?;

        // Effects before interaction — see `lock`'s comment for why.
        Self::set_status(&env, &mut c, Status::Defaulted);
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
            Payout {
                reason: symbol_short!("exp_rem"),
                to: c.cooperative.clone(),
                amount: balance,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// The seller-non-delivery path: `delivery_deadline` has *strictly*
    /// passed (`now > delivery_deadline`; the deadline second itself still
    /// refuses with `DeliveryDeadlineNotPassed`) without `confirm_delivery`
    /// ever having run. Returns whatever's currently escrowed to the buyer
    /// and sets `Forfeited`.
    ///
    /// Buyer-gated, not permissionless — unlike `expire_remainder_window`,
    /// the beneficiary here is a specific party (the buyer) reclaiming
    /// what's rightfully theirs, the same reasoning `reclaim_tranche` uses.
    ///
    /// Not reachable from `Draft`: nothing's ever been escrowed at that
    /// point, so there's nothing to reclaim and no real event has
    /// occurred worth recording as a forfeiture. Since `lock` refuses once
    /// `delivery_deadline` has passed (#277), there is also no way to reach
    /// `Locked` with the delivery window already closed, so this sweep can
    /// only ever fire against a window the cooperative actually had.
    pub fn reclaim_on_nondelivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_reclaim_on_nondelivery(&c, env.ledger().timestamp())?;
        c.buyer.require_auth();

        // Effects before interaction — see `lock`'s comment for why.
        Self::set_status(&env, &mut c, Status::Forfeited);
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
            Payout {
                reason: symbol_short!("forfeit"),
                to: c.buyer.clone(),
                amount: balance,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// Permissionless fallback that forfeits the escrowed funds to the buyer
    /// when the cooperative never called `reclaim_on_nondelivery` and the
    /// buyer's key is lost or inaccessible. Callable by anyone after
    /// `delivery_deadline + remainder_window_secs` (the grace period).
    /// Without this hatch, a lost buyer key would lock funds forever.
    pub fn expire_delivery_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_expire_delivery_window(&c, env.ledger().timestamp())?;

        // Effects before interaction — see `lock`'s comment for why.
        Self::set_status(&env, &mut c, Status::Forfeited);
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
            Payout {
                reason: symbol_short!("exp_del"),
                to: c.buyer.clone(),
                amount: balance,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// ReadyForDelivery -> Delivered. Warehouse-operator-attested, same
    /// reasoning as `mark_checkpoint`. Requires the remainder to already
    /// be funded — delivery shouldn't be confirmable while the buyer
    /// still owes money on the deal.
    ///
    /// Applies the PRD §7 shortfall/grade adjustment schedule: `delivered_quantity`
    /// is compared against `contracted_quantity` (proportional, capped at
    /// 100% — over-delivery isn't paid extra in v1, matching "excess
    /// otherwise released to cooperative" off-chain), `grade_index` selects
    /// an entry from the pre-agreed `grade_price_bps` table, and the two
    /// multiply together into `settlement_bps`, which `settle` pays out
    /// against. The warehouse operator's attestation is authoritative here
    /// — a grade or quantity dispute is the operator's own appeals
    /// process, not a HarvestLock dispute (PRD's edge-case table).
    ///
    /// **Storage access pattern**:
    /// Reads the `Commitment` struct exactly once from instance storage upon
    /// entry via `Self::load`. All validation checks, shortfall scaling, and
    /// `settlement_bps` multiplier calculations execute strictly in memory.
    /// Writes the updated `Commitment` struct exactly once to instance storage
    /// via `Self::save` upon state transition to `Status::Delivered`. No
    /// redundant reads or writes occur across the quantity/grade calculation
    /// or final state-write.
    ///
    /// # Panics
    ///
    /// The checked conversions panic if either derived basis-point value
    /// exceeds `u32`; validated quantity and grade bounds keep both values
    /// within 10,000.
    pub fn confirm_delivery(
        env: Env,
        delivered_quantity: u32,
        grade_index: u32,
    ) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_confirm_delivery(&c)?;
        // Deadline guard: the operator's attestation window ends at
        // `delivery_deadline`; strictly after it only `reclaim_on_nondelivery`
        // is legal. The deadline second itself still belongs to the operator.
        if env.ledger().timestamp() > c.delivery_deadline {
            return Err(Error::DeliveryDeadlinePassed);
        }
        let Some(grade_bps) = c.grade_price_bps.get(grade_index) else {
            return Err(Error::InvalidGradeIndex);
        };
        c.warehouse_operator.require_auth();

        let quantity_bps = u32::try_from(
            i128::from(delivered_quantity.min(c.contracted_quantity)) * 10_000
                / i128::from(c.contracted_quantity),
        )
        .expect("quantity basis points are bounded by 10,000");
        c.delivered_quantity = delivered_quantity;
        c.grade_index = grade_index;
        c.settlement_bps = u32::try_from(i128::from(quantity_bps) * i128::from(grade_bps) / 10_000)
            .expect("settlement basis points are bounded by 10,000");
        // Record the oracle rate at confirmation time, fixing the FX snapshot
        // for the life of the commitment (PRD §4.2 option (b) first-caller fix).
        // If the oracle is stale or unavailable at delivery time, skip recording
        // the rate — `resolve_fx_shortfall` will fall back to reading a fresh
        // quote. This keeps `confirm_delivery` from blocking delivery when the
        // oracle is temporarily unavailable.
        if env.storage().instance().has(&DataKey::OracleConfig) {
            let oracle_config: OracleConfig = env
                .storage()
                .instance()
                .get(&DataKey::OracleConfig)
                .unwrap();
            match Self::read_oracle_rate(&env, &oracle_config) {
                Ok(rate) => {
                    c.fx_recorded_price = rate.price;
                    c.fx_recorded_timestamp = rate.timestamp;
                }
                Err(_) => {
                    // If the oracle rate can't be read at confirmation time
                    // (stale, unavailable, or contract error), skip recording the
                    // rate. `resolve_fx_shortfall` will fall back to reading a
                    // fresh quote. This keeps `confirm_delivery` from blocking
                    // delivery when the oracle is temporarily unavailable.
                }
            }
        }
        Self::set_status(&env, &mut c, Status::Delivered);
        Self::save(&env, &c);
        DeliveryConfirmed {
            grade_index,
            delivered_quantity,
            settlement_bps: c.settlement_bps,
        }
        .publish(&env);
        Ok(())
    }

    /// Delivered -> Settled. Splits the contract's remaining token balance
    /// (the funded remainder — advances already left the contract via
    /// `claim_tranche`/`reclaim_tranche` by this point) between the
    /// cooperative and a shortfall refund to the buyer, per
    /// `confirm_delivery`'s `settlement_bps`.
    ///
    /// "Advance not clawed back" (PRD §7, partial delivery): whatever the
    /// cooperative already claimed via `claim_advance_1`/`claim_advance_2`
    /// stays theirs regardless of the final `settlement_bps` — this only
    /// adjusts the *remainder* payment. `adjusted_total = total_amount *
    /// settlement_bps / 10_000` is the full contract value the delivery
    /// actually earned; subtracting what the cooperative already claimed
    /// gives what's still owed to them from the remainder (floored at
    /// zero, since claimed advances are never clawed back even if a severe
    /// shortfall means they technically over-earned already). Whatever of
    /// the remainder balance isn't owed goes back to the buyer as a
    /// shortfall refund.
    ///
    /// **Requires both advance tranches already resolved** (each either
    /// claimed or expired) before it will run. This was *not* the first
    /// design tried: an earlier version had `settle` silently sweep any
    /// still-unresolved tranche into the cooperative's payment. That's
    /// wrong — if a tranche's claim window had already lapsed but the
    /// buyer simply hadn't gotten around to calling `reclaim_advance_*`
    /// yet, that sweep would hand the buyer's already-vested reclaim right
    /// to the cooperative instead, with no adversarial timing required to
    /// trigger it, just an inactive buyer. Requiring explicit resolution
    /// first means every stroop's destination is always decided by an
    /// actual `claim`/`reclaim` call, never inferred by `settle`.
    ///
    /// **Oracle-configured commitments** (PRD §4.2, option (b)): once
    /// `resolve_fx_shortfall` has run, `adjusted_total` below uses its
    /// fresh-rate-converted `fx_adjusted_total` instead of re-deriving
    /// from `total_amount` — see that function's doc comment for the
    /// conversion itself. `settle` refuses to run at all until
    /// `resolve_fx_shortfall` has resolved (`FxNotResolved`) and any
    /// shortfall it found has actually been paid in
    /// (`FxShortfallUnfunded`) — same "explicit resolution required,
    /// never inferred" principle the tranche-resolution requirement
    /// above already established for this function.
    ///
    /// Permissionless — no auth required: the payout math depends only on
    /// already-recorded state, not on who triggers it.
    pub fn settle(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_settle(&env, &c)?;
        let has_oracle = env.storage().instance().has(&DataKey::OracleConfig);

        let adjusted_total = if has_oracle {
            c.fx_adjusted_total
        } else {
            Self::bps_amount(c.total_amount, c.settlement_bps)
        };
        let claimed_by_coop = Self::claimed_by_coop(&c);
        let owed_from_remainder = (adjusted_total - claimed_by_coop).max(0);

        // Effects before interaction — see `lock`'s comment for why.
        Self::set_status(&env, &mut c, Status::Settled);
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let remainder_balance = token_client.balance(&env.current_contract_address());
        let coop_payment = owed_from_remainder.min(remainder_balance);
        let buyer_refund = remainder_balance - coop_payment;

        if coop_payment > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &c.cooperative,
                &coop_payment,
            );
            Payout {
                reason: symbol_short!("coop_pay"),
                to: c.cooperative.clone(),
                amount: coop_payment,
            }
            .publish(&env);
        }
        if buyer_refund > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &buyer_refund);
            Payout {
                reason: symbol_short!("refund"),
                to: c.buyer.clone(),
                amount: buyer_refund,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// PRD §4.2 option (b), the actual conversion step: reads a fresh
    /// oracle rate, reprices `oracle_config.denominated_amount` (adjusted
    /// by `settlement_bps`, same shortfall/grade math `settle` itself
    /// uses) into the settlement token, and compares that against what's
    /// actually escrowed. Whatever gap remains — the buyer's FX risk,
    /// made concrete — becomes `fx_shortfall_amount` for `fund_fx_shortfall`
    /// to collect.
    ///
    /// Worked example, using this session's live-tested numbers: a deal
    /// worth 1,000,000 NGN (`denominated_amount = 10_000_000_000_000` at
    /// the 7-decimal convention), fully delivered at full grade
    /// (`settlement_bps = 10_000`), Reflector quoting `price =
    /// 66_700_000_000` at `decimals = 14` (≈1 NGN = 0.000667 USD, i.e.
    /// ≈1,500 NGN/USD): `ngn_owed = 10_000_000_000_000` (unchanged, full
    /// bps), `fx_adjusted_total = 10_000_000_000_000 * 66_700_000_000 /
    /// 10^14 = 6_670_000_000` — 667 USDC, matching ₦1,000,000 at
    /// ≈1,500 NGN/USD by hand. If `total_amount` had been funded
    /// assuming a slightly better rate, escrow might hold only
    /// 6_500_000_000 (650 USDC) — a genuine 17 USDC shortfall the buyer
    /// now owes, exactly the FX-risk gap PRD §4.2 names.
    ///
    /// **The oracle's `decimals` divisor is trusted to be constant** (#63): it
    /// is read live from the oracle rather than pinned in `OracleConfig`,
    /// because it is a property of the oracle, not the commitment. Reflector's
    /// real contract makes `decimals` immutable (write-once at `config`, no
    /// setter; SEP-40 likewise says it "should never change"), so the trust
    /// is well-placed for the deployed oracle. If it were ever violated the
    /// conversion would silently rescale by a power of ten when representable;
    /// unrepresentable arithmetic returns `OracleConversionOutOfRange` rather
    /// than trapping. See `docs/reflector-decimals-assumption.md` and the
    /// `a_plausible_decimals_change_silently_rescales_resolve_fx_shortfall`
    /// test that pins it.
    ///
    /// **The settlement token's decimals are read, not assumed** (#258).
    /// `denominated_amount` is in the 7-decimal convention `total_amount`
    /// uses for native XLM; a SEP-41 token may use 6, 18, or any other
    /// `decimals()`. The conversion therefore rescales the 7-decimal result
    /// to the token's live `decimals()` before comparing it against escrowed
    /// balances. Unlike the oracle's scale this is a property of the token
    /// contract, read per call — a token proxy that changed its decimals
    /// would be a different, equally trust-the-provider assumption.
    ///
    /// Requires both tranches already resolved (same guard `settle`
    /// itself has) before reading `claimed_by_coop` — computing this
    /// against a still-open tranche would use figures that could still
    /// change by the time `settle` actually runs. Permissionless, same
    /// reasoning as `release_advance_*`: this only computes and records
    /// a number, it doesn't move funds or grant anyone anything they
    /// weren't already entitled to. One-time (`FxAlreadyResolved`) —
    /// see the module doc for why re-resolving isn't allowed.
    ///
    /// On success, clears `fx_quote_stale` (#1): a commitment whose
    /// resolution just ran is by definition no longer waiting on a fresh
    /// quote. (The stale-refusal itself still can't set anything — an
    /// invocation returning `Err` rolls back every write — which is
    /// exactly what `refresh_fx_quote_status` exists for.)
    pub fn resolve_fx_shortfall(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_resolve_fx_shortfall(&env, &c)?;
        let oracle_config: OracleConfig = env
            .storage()
            .instance()
            .get(&DataKey::OracleConfig)
            .ok_or(Error::OracleNotConfigured)?;

        let rate = Self::read_oracle_rate(&env, &oracle_config)?;
        // `try_decimals`, not `decimals` (#245): `lastprice` just succeeded
        // above, but the oracle can still fail on this second call, and
        // that should also be a contract error rather than a host abort.
        let oracle_decimals =
            match ReflectorPulseClient::new(&env, &oracle_config.oracle_contract).try_decimals() {
                Ok(Ok(decimals)) => decimals,
                Ok(Err(_)) | Err(_) => return Err(Error::OracleUnavailable),
            };
        let token_decimals = token::Client::new(&env, &c.token).decimals();

        let ngn_owed = Self::bps_amount(oracle_config.denominated_amount, c.settlement_bps);
        // `ngn_owed` is in `denominated_amount`'s 7-decimal units. Apply the
        // oracle rate first: the result is still in 7-decimal settlement-token
        // units. Then rescale those 7-decimal units to the settlement token's
        // own `decimals()` (#258). Native XLM and USDC SACs are both 7-decimal,
        // which hid this: a 6- or 18-decimal SEP-41 token would otherwise be
        // off by a power of ten in every FX settlement. Rescaling after the
        // oracle division (rather than multiplying the numerator by
        // `10^token_decimals` and dividing at the end) keeps an 18-decimal
        // token's conversion from overflowing i128 on realistic amounts; the
        // cost is flooring once at 7-decimal precision before scaling.
        let fx_in_denominated_decimals =
            Self::checked_fx_conversion(ngn_owed, rate.price, oracle_decimals)?;
        let fx_adjusted_total = if token_decimals >= 7 {
            fx_in_denominated_decimals * 10i128.pow(token_decimals - 7)
        } else {
            fx_in_denominated_decimals / 10i128.pow(7 - token_decimals)
        };

        let claimed_by_coop = Self::claimed_by_coop(&c);
        // Reclaimed tranches were returned to the buyer and are no longer in
        // escrow, so the coop has no claim on them; subtract them so we do not
        // charge the buyer a shortfall for funds they already recovered.
        let reclaimed_by_buyer = Self::reclaimed_by_buyer(&c);
        let owed_from_remainder = (fx_adjusted_total - claimed_by_coop - reclaimed_by_buyer).max(0);
        let available_balance =
            token::Client::new(&env, &c.token).balance(&env.current_contract_address());
        let shortfall = (owed_from_remainder - available_balance).max(0);

        c.fx_adjusted_total = fx_adjusted_total;
        c.fx_resolved = true;
        c.fx_quote_stale = false;
        c.fx_shortfall_amount = shortfall;
        if shortfall > 0 {
            c.fx_shortfall_funded = false;
            c.fx_shortfall_deadline = env.ledger().timestamp() + c.remainder_window_secs;
        } else {
            // Nothing owed -- trivially "funded" so settle()'s guard
            // (`fx_shortfall_amount > 0 && !fx_shortfall_funded`) reads
            // naturally without a separate "or amount is zero" branch.
            c.fx_shortfall_funded = true;
        }
        Self::save(&env, &c);
        FxResolved {
            rate: rate.price,
            rate_timestamp: rate.timestamp,
            fx_adjusted_total,
            shortfall,
            deadline: c.fx_shortfall_deadline,
        }
        .publish(&env);
        Ok(())
    }

    /// Records whether settlement's FX resolution is currently blocked
    /// on a stale oracle quote (#1) — the "waiting on a fresh quote"
    /// signal, made readable instead of inferable only from a failed
    /// `resolve_fx_shortfall` call.
    ///
    /// Why this exists as its own call: Soroban rolls back every state
    /// write an invocation makes when it returns `Err`, so
    /// `resolve_fx_shortfall`'s `OracleStale` refusal cannot persist any
    /// marker of its own — by the time a caller sees that error, nothing
    /// was recorded. This probe is the write that refusal can't be: it
    /// runs the same staleness check and records the answer in
    /// `Commitment::fx_quote_stale`, which `get_commitment` surfaces to
    /// the API/indexers/UI.
    ///
    /// Sets the flag when the quote is stale; clears it when a fresh
    /// quote is read — so the flag self-corrects the moment Reflector
    /// publishes a fresh one, with no reverse transition to learn. A
    /// missing quote (`OraclePriceUnavailable`) propagates as an error
    /// instead of setting anything: "the oracle doesn't quote this asset
    /// at all" is a permanent configuration gap, not "come back later",
    /// and conflating the two would tell a watcher to wait for something
    /// that may never arrive.
    ///
    /// An unreachable or crashing oracle (#245) now comes back as
    /// `Error::OracleUnavailable`, not a host abort, and like
    /// `OraclePriceUnavailable` it also writes nothing. Two reasons to
    /// return the error rather than set `fx_quote_stale` or add a new flag:
    /// (a) `oracle_config` has no setter, so an address with no contract
    /// behind it is just as permanent a misconfiguration as an unquoted
    /// asset, and telling a watcher to wait would be wrong; (b) a new
    /// `Commitment` field would change `get_commitment`'s return shape (an
    /// ABI change under docs/versioning.md §2) for information the caller
    /// already gets from the error code. What changes for callers is that
    /// the probe now *returns* that error code instead of aborting, so the
    /// API can record why settlement is stuck instead of just "unreadable".
    ///
    /// The flag is **informational only**: no guard reads it, and
    /// `resolve_fx_shortfall` still enforces staleness itself — its
    /// worst-case failure is a misleading display, never a wrong payout.
    /// Deliberately a boolean flag alongside `fx_resolved`, not a new
    /// `Status` variant: "waiting on a fresh quote" is a condition on one
    /// sub-step of settlement, not a position in the deal's lifecycle —
    /// the commitment is still `Delivered`, and a new variant would force
    /// every status-matching guard, `StatusChanged` consumer, and
    /// off-chain reputation mapping to learn about an oracle-side delay
    /// none of them act on. (A new variant is also technically barred in
    /// a subtler way than the churn: the dispute mechanism restores an
    /// arbitrary pre-dispute `Status`, and every additional variant is
    /// one more restore target the freeze/resume pair would have to
    /// reason about.) The lighter option issue #1 named, with none of
    /// the state-machine complexity it asked about.
    ///
    /// Permissionless — no auth required, same reasoning as
    /// `resolve_fx_shortfall`: it records an oracle fact, moves nothing,
    /// grants nothing, and takes no outcome parameter (the answer is
    /// whatever the configured oracle says). Refuses with `InvalidState`
    /// outside `Delivered` (the only status in which the flag can
    /// matter), `OracleNotConfigured` when this commitment needs no
    /// conversion, and `FxAlreadyResolved` once the one-time resolution
    /// has run — there is nothing left to wait for.
    pub fn refresh_fx_quote_status(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_refresh_fx_quote_status(&env, &c)?;
        let oracle_config: OracleConfig = env
            .storage()
            .instance()
            .get(&DataKey::OracleConfig)
            .ok_or(Error::OracleNotConfigured)?;

        let prev_stale = c.fx_quote_stale;
        match Self::read_oracle_rate(&env, &oracle_config) {
            Ok(_) => c.fx_quote_stale = false,
            Err(Error::OracleStale) => c.fx_quote_stale = true,
            Err(e) => return Err(e),
        }
        let new_stale = c.fx_quote_stale;
        Self::save(&env, &c);
        if new_stale != prev_stale {
            FxQuoteStaleChanged { stale: new_stale }.publish(&env);
        }
        Ok(())
    }

    /// Buyer pays in the shortfall `resolve_fx_shortfall` computed.
    /// Buyer-gated, matching `fund_remainder`'s own reasoning: this is
    /// new money the buyer specifically owes, not a mechanical step
    /// anyone could trigger.
    ///
    /// Deadline-**inclusive**, exactly like `fund_remainder`:
    /// `now > fx_shortfall_deadline` is the only rejection, so the
    /// deadline second itself is still the buyer's to pay in.
    pub fn fund_fx_shortfall(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_fund_fx_shortfall(&c, env.ledger().timestamp())?;
        c.buyer.require_auth();

        let amount = c.fx_shortfall_amount;
        // Effects before interaction — see `lock`'s comment for why.
        c.fx_shortfall_funded = true;
        Self::save(&env, &c);

        token::Client::new(&env, &c.token).transfer(
            &c.buyer,
            env.current_contract_address(),
            &amount,
        );
        FxShortfallFunded {
            from: c.buyer.clone(),
            amount,
        }
        .publish(&env);
        Ok(())
    }

    /// The FX-shortfall buyer-default path: `fx_shortfall_deadline` has
    /// *strictly* passed (`now > fx_shortfall_deadline`; the deadline
    /// second itself still refuses with `FxShortfallWindowNotPassed`) with
    /// the top-up never funded. Sweeps whatever's currently
    /// escrowed to the cooperative and sets `Status::Defaulted` — the
    /// *same* status `expire_remainder_window`'s buyer-default already
    /// uses, by explicit product decision (see module doc), which means
    /// the API's existing reputation consequence (immediate permanent
    /// bar) applies here with no changes needed there at all.
    ///
    /// Permissionless, same reasoning as `expire_remainder_window`: the
    /// outcome doesn't depend on who calls it, only on whether the
    /// deadline has passed.
    pub fn expire_fx_shortfall_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_expire_fx_shortfall_window(&c, env.ledger().timestamp())?;

        // Effects before interaction — see `lock`'s comment for why.
        Self::set_status(&env, &mut c, Status::Defaulted);
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
            Payout {
                reason: symbol_short!("exp_fx"),
                to: c.cooperative.clone(),
                amount: balance,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// Mutual cancellation — PRD §7: "Defined unwind: advance settled per
    /// agreed schedule, remaining escrow returned, no penalty, logged."
    /// Allowed from any pre-delivery state (`Draft` through
    /// `ReadyForDelivery`). Not from `Delivered` onward — at that point
    /// `settle` is the correct path, an unwind doesn't apply anymore.
    ///
    /// Requires **both** the buyer's and the cooperative's auth in the same
    /// call, since this is mutual, not unilateral — unlike `reclaim_*`,
    /// which is the buyer's unilateral right once a claim window lapses on
    /// its own.
    ///
    /// "Advance settled per agreed schedule, no penalty": whatever's
    /// already been claimed stays with the cooperative — this doesn't claw
    /// anything back. "Remaining escrow returned": whatever's still in the
    /// contract (zero, if nothing was ever locked) goes back to the buyer,
    /// via the same balance-based transfer `settle` uses, so it's correct
    /// regardless of claim/reclaim history or how much of the deposit vs.
    /// remainder had been funded. "Logged": the state transition and the
    /// transfer both land on the ledger from this call — the transition
    /// additionally emits `status_changed` like every other transition
    /// (`set_status`), so an indexer sees the cancellation without having
    /// to inspect the balance change.
    pub fn cancel(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_cancel(&c)?;
        Self::require_once_each(&[&c.buyer, &c.cooperative]);

        // Effects before interaction — see `lock`'s comment for why.
        Self::set_status(&env, &mut c, Status::Cancelled);
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
            Payout {
                reason: symbol_short!("cancel"),
                to: c.buyer.clone(),
                amount: balance,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// Buyer-position assignability (PRD §4.8): transfers `buyer` to
    /// `new_buyer`. Deliberately **not** a market — there's no order book,
    /// no listing, no on-chain price discovery here, just a novation of
    /// who holds the position. No funds move; this only ever rewrites who
    /// `buyer` refers to for every future `reclaim_*`/`cancel`/
    /// `fund_remainder` auth check.
    ///
    /// Requires **three** signatures in the same call, not two: the
    /// current buyer's (they're giving up the position), the
    /// cooperative's (PRD's explicit "with cooperative consent"), and the
    /// new buyer's. That third one isn't named in the PRD line this
    /// implements, but leaving it out would let the current buyer and
    /// cooperative saddle a third party with a position — including its
    /// obligations — without that party ever agreeing to take it on. Two
    /// consents where the PRD asked for two would be the smaller change;
    /// three is the safer one, and safety wins here.
    ///
    /// Reachable from the same states as `cancel` (`Draft` through
    /// `ReadyForDelivery`) and for the same reason: past `Delivered`,
    /// `buyer` no longer gates any remaining action (`settle` doesn't
    /// check it), so reassigning it after that point would be a no-op
    /// dressed up as a real transfer.
    /// Emits [`BuyerReassigned`] after the write so observers learn both the
    /// outgoing and incoming buyer in a single indexed event (#206).
    pub fn reassign_buyer(env: Env, new_buyer: Address) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_reassign_buyer(&c)?;
        Self::require_once_each(&[&c.buyer, &c.cooperative, &new_buyer]);

        let previous = c.buyer.clone();
        c.buyer = new_buyer.clone();
        Self::save(&env, &c);
        BuyerReassigned {
            previous,
            new_buyer,
        }
        .publish(&env);
        Ok(())
    }

    /// Dispute flagging (PRD's must-have "dispute flagging with defined
    /// escalation"). Any **one** of the three named parties — `flagger`
    /// must equal `buyer`, `cooperative`, or `warehouse_operator` — can
    /// freeze the commitment by moving it to `Status::Disputed`, which
    /// blocks every other state-changing call by construction: every one
    /// of them matches a specific required status (or set of statuses)
    /// and `Disputed` is never among them.
    ///
    /// This contract still doesn't arbitrate *what* the dispute is about
    /// — see the module doc's explanation of why that's a deliberate
    /// non-goal, not an oversight. What this buys is the "flagging" half
    /// only: a unilateral pause, so a contested situation doesn't let
    /// some other deadline-triggered function (e.g. `settle`,
    /// `expire_remainder_window`) run to a conclusion while it's being
    /// sorted out off-chain. Reachable from any state with funds already
    /// at stake or a claim outstanding (`Locked` through `Delivered`) —
    /// not `Draft` (nothing escrowed yet to freeze) and not any terminal
    /// status or `Disputed` itself.
    ///
    /// **Known limitation, not fixed here**: freezing `status` does not
    /// pause any other *absolute* deadline already ticking on this
    /// commitment (an advance's claim deadline, `remainder_deadline`,
    /// `delivery_deadline`, `fx_shortfall_deadline`). If one of those
    /// passes while a dispute is open, the corresponding
    /// `claim_*`/`expire_*`/`reclaim_*` becomes immediately callable the
    /// moment the dispute resolves or its own window lapses, even though
    /// no one could act on it during the freeze. Shifting every other
    /// deadline by the dispute's duration would close this cleanly but
    /// is real added complexity for a mechanism nobody has used yet —
    /// left open on purpose, same bias against building ahead of real
    /// usage this project applies elsewhere; revisit if a real dispute
    /// ever actually collides with another deadline this way.
    ///
    /// Requires the flagger's own auth (`flagger.require_auth()`), and
    /// `flagger` must be the buyer, the cooperative, or the warehouse
    /// operator.
    pub fn flag_dispute(env: Env, flagger: Address) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_flag_dispute(&c, &flagger)?;
        flagger.require_auth();

        c.dispute_pre_status = c.status;
        c.dispute_deadline = env.ledger().timestamp() + c.remainder_window_secs;
        Self::set_status(&env, &mut c, Status::Disputed);
        Self::save(&env, &c);
        DisputeFlagged {
            flagger,
            pre_status: c.dispute_pre_status,
            deadline: c.dispute_deadline,
        }
        .publish(&env);
        Ok(())
    }

    /// Resolves a dispute by unanimous consent — **all three** named
    /// parties' auth, not a majority or any single one, since resuming
    /// is the mirror of `flag_dispute`'s unilateral freeze and shouldn't
    /// itself be unilateral. Restores `status` to exactly whatever it
    /// was the moment `flag_dispute` ran — this contract doesn't decide,
    /// or let the parties redirect it to, a *different* outcome (e.g.
    /// straight to `Cancelled`) from inside this call; that would be
    /// arbitration by another name. If the parties' off-chain resolution
    /// is "unwind the deal," they call the existing `cancel()` next,
    /// same as they would have without ever disputing.
    ///
    /// **Deadline-free on purpose**: unlike `expire_dispute_window`, this
    /// carries no `dispute_deadline` check at all, so it stays callable in
    /// the deadline second itself and after it. Unanimous consent is the
    /// only gate — the timeout exists to break a deadlock, never to take
    /// the decision away from three parties who have already agreed.
    pub fn resolve_dispute(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_resolve_dispute(&c)?;
        Self::require_once_each(&[&c.buyer, &c.cooperative, &c.warehouse_operator]);

        let restored = c.dispute_pre_status;
        c.dispute_pre_status = Status::Draft;
        c.dispute_deadline = 0;
        Self::set_status(&env, &mut c, restored);
        Self::save(&env, &c);
        DisputeEnded {
            outcome: DisputeOutcome::Resolved,
            restored,
        }
        .publish(&env);
        Ok(())
    }

    /// The dispute-side escape hatch, same shape as
    /// `expire_remainder_window`/`expire_fx_shortfall_window`:
    /// permissionless, deadline-gated. If the three parties can't reach
    /// the unanimous consent `resolve_dispute` requires before
    /// `dispute_deadline`, this does **not** guess who was at fault —
    /// consistent with `flag_dispute`'s doc comment, this contract still
    /// isn't arbitrating anything. It just restores the pre-dispute
    /// status, exactly like `resolve_dispute` would, so the freeze can't
    /// become permanent by one party simply refusing to ever consent.
    /// Whatever normal deadline-triggered mechanism would otherwise have
    /// applied (`expire_remainder_window`, `reclaim_on_nondelivery`,
    /// etc.) is free to run again from there.
    ///
    /// Deadline-**exclusive**: `now > dispute_deadline` is required, so in
    /// the deadline second itself this still refuses with
    /// `DisputeWindowNotPassed` while `resolve_dispute` remains available —
    /// the boundary second is never a dead zone where neither resume path
    /// works.
    pub fn expire_dispute_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        Self::can_expire_dispute_window(&c, env.ledger().timestamp())?;

        let restored = c.dispute_pre_status;
        c.dispute_pre_status = Status::Draft;
        c.dispute_deadline = 0;
        c.dispute_count += 1;
        Self::set_status(&env, &mut c, restored);
        Self::save(&env, &c);
        DisputeEnded {
            outcome: DisputeOutcome::Expired,
            restored,
        }
        .publish(&env);
        Ok(())
    }

    /// No auth required — read-only view, callable by anyone.
    pub fn get_status(env: Env) -> Result<Status, Error> {
        Ok(Self::load(&env)?.status)
    }

    /// No auth required — read-only view, returns the full commitment state.
    ///
    /// Kept for backward compatibility: existing clients read this storage
    /// struct directly, and it remains the canonical on-chain record. New
    /// clients should prefer [`Self::get_commitment_view`], whose shape is
    /// versioned independently of the storage layout.
    pub fn get_commitment(env: Env) -> Result<Commitment, Error> {
        Self::load(&env)
    }

    /// No auth required — read-only view, callable by anyone.
    ///
    /// Returns every *state-changing* call the current commitment permits,
    /// each with the auth it needs and the timestamp that bounds it (if
    /// any). This is the contract's answer to the hand-written "who can
    /// call what, when" table in `docs/griefing-and-inaction.md` §1: a
    /// client can render exactly the legal buttons instead of rebuilding
    /// the state machine.
    ///
    /// Every entry is produced by the same private predicate the real
    /// guard calls (`can_lock`, `can_claim_tranche`, `can_settle`, ...), so
    /// the view cannot drift from the checks that actually authorize the
    /// call. A predicate is `Ok` only when every state-derived guard would
    /// pass; argument-specific validation (a valid `grade_index`, a
    /// non-empty allocation list, a `new_buyer` address, a `flagger` that
    /// is one of the named parties) remains at the call site because it
    /// isn't knowable from the commitment.
    ///
    /// Two deliberate exclusions: `initialize` (there is no commitment to
    /// derive actions from before it runs — the view returns an empty list
    /// in that case) and the read-only getters/`oracle_rate` (they don't
    /// change state and are always callable). Oracle-backed actions are
    /// advertised purely from local state — a configured oracle and an
    /// unresolved FX step — since quote freshness is an external read, not
    /// a guard predicate. The `fx_quote_stale` flag on `Commitment` is the
    /// companion signal for "waiting on a fresh quote"; an advertised
    /// `resolve_fx_shortfall` can still return `Error::OracleStale` until
    /// Reflector publishes one.
    pub fn available_actions(env: Env) -> Vec<Action> {
        let Ok(mut c) = Self::load(&env) else {
            // No commitment yet: there is no state-derived action to name.
            return Vec::new(&env);
        };
        let now = env.ledger().timestamp();
        let mut actions = Vec::new(&env);

        Self::push_action(
            &env,
            &mut actions,
            Self::can_set_allocation(&env, &c),
            "set_allocation",
            AuthParty::Cooperative,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_lock(&c, now),
            "lock",
            AuthParty::Buyer,
            // #277: `lock` is only legal while the delivery window that
            // `initialize` started is still open, so advertise that bound
            // rather than "no deadline" — an advertised action must always
            // be one the contract would actually accept.
            ActionDeadline::Inclusive(c.delivery_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_open_tranche(&c, Tranche::One),
            "release_advance_1",
            AuthParty::Anyone,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_claim_tranche(&mut c, Tranche::One, now),
            "claim_advance_1",
            AuthParty::Cooperative,
            ActionDeadline::Inclusive(c.advance1_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_reclaim_tranche(&mut c, Tranche::One, now),
            "reclaim_advance_1",
            AuthParty::Buyer,
            ActionDeadline::Exclusive(c.advance1_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_mark_checkpoint(&c),
            "mark_checkpoint",
            AuthParty::WarehouseOperator,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_open_tranche(&c, Tranche::Two),
            "release_advance_2",
            AuthParty::Anyone,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_claim_tranche(&mut c, Tranche::Two, now),
            "claim_advance_2",
            AuthParty::Cooperative,
            ActionDeadline::Inclusive(c.advance2_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_reclaim_tranche(&mut c, Tranche::Two, now),
            "reclaim_advance_2",
            AuthParty::Buyer,
            ActionDeadline::Exclusive(c.advance2_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_ready_for_delivery(&c),
            "ready_for_delivery",
            AuthParty::Cooperative,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_fund_remainder(&c, now),
            "fund_remainder",
            AuthParty::Buyer,
            ActionDeadline::Inclusive(c.remainder_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_expire_remainder_window(&c, now),
            "expire_remainder_window",
            AuthParty::Anyone,
            ActionDeadline::Exclusive(c.remainder_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_reclaim_on_nondelivery(&c, now),
            "reclaim_on_nondelivery",
            AuthParty::Buyer,
            ActionDeadline::Exclusive(c.delivery_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_expire_delivery_window(&c, now),
            "expire_delivery_window",
            AuthParty::Anyone,
            ActionDeadline::Exclusive(c.delivery_deadline.saturating_add(c.remainder_window_secs)),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_confirm_delivery(&c),
            "confirm_delivery",
            AuthParty::WarehouseOperator,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_settle(&env, &c),
            "settle",
            AuthParty::Anyone,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_resolve_fx_shortfall(&env, &c),
            "resolve_fx_shortfall",
            AuthParty::Anyone,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_replace_oracle_config(&env, &c),
            "replace_oracle_config",
            AuthParty::BuyerAndCooperative,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_refresh_fx_quote_status(&env, &c),
            "refresh_fx_quote_status",
            AuthParty::Anyone,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_fund_fx_shortfall(&c, now),
            "fund_fx_shortfall",
            AuthParty::Buyer,
            ActionDeadline::Inclusive(c.fx_shortfall_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_expire_fx_shortfall_window(&c, now),
            "expire_fx_shortfall_window",
            AuthParty::Anyone,
            ActionDeadline::Exclusive(c.fx_shortfall_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_extend_delivery_deadline(&c, now, c.delivery_deadline.saturating_add(1)),
            "extend_delivery_deadline",
            AuthParty::BuyerAndCooperative,
            ActionDeadline::Inclusive(c.delivery_deadline),
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_cancel(&c),
            "cancel",
            AuthParty::BuyerAndCooperative,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_reassign_buyer(&c),
            "reassign_buyer",
            AuthParty::BuyerCooperativeAndNewBuyer,
            ActionDeadline::None,
        );
        // The buyer is always one of the three named parties, so passing it
        // as the representative flagger makes this the same predicate the
        // real call uses (which also checks the caller-supplied address).
        Self::push_action(
            &env,
            &mut actions,
            Self::can_flag_dispute(&c, &c.buyer),
            "flag_dispute",
            AuthParty::AnyNamedParty,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_resolve_dispute(&c),
            "resolve_dispute",
            AuthParty::AllParties,
            ActionDeadline::None,
        );
        Self::push_action(
            &env,
            &mut actions,
            Self::can_expire_dispute_window(&c, now),
            "expire_dispute_window",
            AuthParty::Anyone,
            ActionDeadline::Exclusive(c.dispute_deadline),
        );
        actions
    }

    /// No auth required — the versioned read model (#256).
    ///
    /// Same state as `get_commitment`, reshaped into [`CommitmentView`] with
    /// an explicit `schema_version` and semantic sub-structs, so a field added
    /// to `Commitment` for storage reasons does not change the client-facing
    /// ABI. `fx.configured` is read from the same instance-storage entry that
    /// gates settlement's FX path, so a caller can distinguish "no oracle"
    /// from "oracle resolved with zeros" without a second call.
    pub fn get_commitment_view(env: Env) -> Result<CommitmentView, Error> {
        let c = Self::load(&env)?;
        let oracle_configured = env.storage().instance().has(&DataKey::OracleConfig);
        Ok(CommitmentView::from_commitment(&c, oracle_configured))
    }

    // -- internal --

    /// Extends the instance/code entry and the separate persistent
    /// allocation entry (#98/#232). Both are topped up only when their
    /// remaining lifetime falls below half the network maximum.
    ///
    /// Without this call the entry would keep only the TTL it got at
    /// deploy time (`min_persistent_entry_ttl` ledgers): Soroban does
    /// *not* auto-extend a contract instance when it is invoked, so a
    /// commitment with no activity — legitimately possible in `Draft`
    /// or `Locked`, where no deadline forces anyone to act — would see
    /// its own state archived out from under its escrow. Strategy:
    /// re-rent only once the remaining lifetime has decayed below half
    /// the network maximum, topping back up to that maximum. Actively
    /// used commitments therefore pay extension rent at most once per
    /// half-max lifetime (most calls are a free no-op above the
    /// threshold), and any dormant one is rescued back to full lifetime
    /// by the first read — including the API's routine
    /// `get_status`/`get_commitment` polling. A commitment left
    /// untouched for *longer* than the network max lifetime still
    /// expires; that residual is inherent to any Soroban storage model
    /// short of periodic keeper transactions, and docs/storage-ttl.md
    /// documents the mitigation (`stellar contract extend`/`restore`
    /// and the watcher pattern) rather than pretending it away.
    fn bump_instance_ttl(env: &Env) {
        let max_ttl = env.storage().max_ttl();
        env.storage().instance().extend_ttl(max_ttl / 2, max_ttl);
        Self::bump_allocation_ttl(env);
    }

    fn bump_allocation_ttl(env: &Env) {
        let max_ttl = env.storage().max_ttl();
        if env.storage().persistent().has(&DataKey::Allocation) {
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Allocation, max_ttl / 2, max_ttl);
        }
    }

    fn load(env: &Env) -> Result<Commitment, Error> {
        // Every public entry point except `initialize` reads through
        // here, which is what makes `bump_instance_ttl` above the one
        // place the TTL actually needs maintaining.
        Self::bump_instance_ttl(env);
        env.storage()
            .instance()
            .get(&DataKey::Commitment)
            .ok_or(Error::NotInitialized)
    }

    /// The one state write for `Commitment`, and therefore the one place
    /// the test-build invariant checker hooks in (#225).
    ///
    /// `check_invariants` is compiled in only for `cfg(test)` (the unit
    /// suite) or the `testutils` feature (so the integration suites in
    /// `tests/` can assert the same thing with `--features testutils`). It
    /// is absent from the release WASM — the release build enables neither
    /// — so this costs the deployed contract exactly nothing; see the
    /// "WASM Upload vs Instantiation" / build sections of
    /// `docs/gas-costs.md` for the size discipline this respects.
    fn save(env: &Env, c: &Commitment) {
        #[cfg(any(test, feature = "testutils"))]
        c.check_invariants();
        env.storage().instance().set(&DataKey::Commitment, c);
    }

    // -- shared guard predicates --
    //
    // Every state-derived guard on a state-changing call lives in exactly
    // one of the `can_*` functions below. The public function calls the same
    // predicate (and maps its `Err` straight through), and `available_actions`
    // calls it to decide whether to advertise the action. That shared call
    // is what stops the view from drifting from the real checks — including
    // which error wins when several guards fail at once.
    //
    // Caller-supplied argument validation that cannot be derived from the
    // commitment (a valid `grade_index`, a non-empty allocation list, a
    // `new_buyer`, a `flagger` that belongs to the deal) stays at the call
    // site; the predicates cover everything the view can actually know.
    //
    // The *status* half of every predicate is not written here at all: each
    // one opens with `require_status`, which reads the single declared
    // transition table (`Method::allowed_from`, #227). Predicate order still
    // matters and is unchanged — status first, then flags, then deadlines —
    // which is what keeps the error-ordering expectations in `src/test.rs`
    // and `tests/snapshot_replay.rs` valid.

    /// The one status guard (#227): looks `method` up in the declared
    /// transition table and refuses with `Error::InvalidState` when the
    /// commitment's current status isn't one of its allowed sources.
    ///
    /// `Error::InvalidState` — not a per-method error — is deliberate: it is
    /// what every hand-written guard returned before this table existed, so
    /// centralizing them changes no observable behaviour, error variants
    /// included. A method whose row is `None` (the two tranche-claim pairs)
    /// has no status gate and this always passes.
    fn require_status(c: &Commitment, method: Method) -> Result<(), Error> {
        match method.allowed_from() {
            None => Ok(()),
            Some(allowed) if allowed.contains(&c.status) => Ok(()),
            Some(_) => Err(Error::InvalidState),
        }
    }

    fn push_action(
        env: &Env,
        actions: &mut Vec<Action>,
        allowed: Result<(), Error>,
        name: &str,
        auth: AuthParty,
        deadline: ActionDeadline,
    ) {
        if allowed.is_ok() {
            actions.push_back(Action {
                function: Symbol::new(env, name),
                auth,
                deadline,
            });
        }
    }

    fn can_set_allocation(env: &Env, c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::SetAllocation)?;
        if env.storage().persistent().has(&DataKey::Allocation) {
            return Err(Error::AllocationAlreadySet);
        }
        Ok(())
    }

    fn can_lock(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::Lock)?;
        // #277: the delivery window starts at `initialize`, so a lock past
        // its end would leave the cooperative no time to deliver and hand
        // the buyer an immediate `reclaim_on_nondelivery`. The deadline
        // second itself still belongs to the window, like every other
        // deadline in this file.
        if now > c.delivery_deadline {
            return Err(Error::LockAfterDeliveryDeadline);
        }
        Ok(())
    }

    fn can_mark_checkpoint(c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::MarkCheckpoint)
    }

    fn can_ready_for_delivery(c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::ReadyForDelivery)
    }

    fn can_fund_remainder(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::FundRemainder)?;
        if c.remainder_funded {
            return Err(Error::RemainderAlreadyFunded);
        }
        if now > c.remainder_deadline {
            return Err(Error::RemainderWindowPassed);
        }
        Ok(())
    }

    fn can_expire_remainder_window(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::ExpireRemainderWindow)?;
        if c.remainder_funded {
            return Err(Error::RemainderAlreadyFunded);
        }
        if now <= c.remainder_deadline {
            return Err(Error::RemainderWindowNotPassed);
        }
        Ok(())
    }

    fn can_reclaim_on_nondelivery(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::ReclaimOnNondelivery)?;
        if now <= c.delivery_deadline {
            return Err(Error::DeliveryDeadlineNotPassed);
        }
        Ok(())
    }

    fn can_extend_delivery_deadline(
        c: &Commitment,
        now: u64,
        new_deadline: u64,
    ) -> Result<(), Error> {
        Self::require_status(c, Method::ExtendDeliveryDeadline)?;
        if now > c.delivery_deadline {
            return Err(Error::DeliveryDeadlinePassed);
        }
        if new_deadline <= c.delivery_deadline {
            return Err(Error::DeliveryDeadlineNotExtended);
        }
        Ok(())
    }

    fn can_expire_delivery_window(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::ExpireDeliveryWindow)?;
        let grace_deadline = c.delivery_deadline.saturating_add(c.remainder_window_secs);
        if now <= grace_deadline {
            return Err(Error::DeliveryGraceWindowNotPassed);
        }
        Ok(())
    }

    fn can_confirm_delivery(c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::ConfirmDelivery)?;
        if !c.remainder_funded {
            return Err(Error::RemainderNotFunded);
        }
        Ok(())
    }

    fn tranches_resolved(c: &Commitment) -> Result<(), Error> {
        if !c.advance1_claimed && !c.advance1_expired {
            return Err(Error::TrancheUnresolved);
        }
        if !c.advance2_claimed && !c.advance2_expired {
            return Err(Error::TrancheUnresolved);
        }
        Ok(())
    }

    fn can_settle(env: &Env, c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::Settle)?;
        Self::tranches_resolved(c)?;
        if env.storage().instance().has(&DataKey::OracleConfig) {
            if !c.fx_resolved {
                return Err(Error::FxNotResolved);
            }
            if c.fx_shortfall_amount > 0 && !c.fx_shortfall_funded {
                return Err(Error::FxShortfallUnfunded);
            }
        }
        Ok(())
    }

    fn can_replace_oracle_config(env: &Env, c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::ReplaceOracleConfig)?;
        if c.fx_resolved {
            return Err(Error::FxAlreadyResolved);
        }
        if !env.storage().instance().has(&DataKey::OracleConfig) {
            return Err(Error::OracleNotConfigured);
        }
        Ok(())
    }

    fn can_resolve_fx_shortfall(env: &Env, c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::ResolveFxShortfall)?;
        Self::tranches_resolved(c)?;
        if c.fx_resolved {
            return Err(Error::FxAlreadyResolved);
        }
        if !env.storage().instance().has(&DataKey::OracleConfig) {
            return Err(Error::OracleNotConfigured);
        }
        Ok(())
    }

    fn can_refresh_fx_quote_status(env: &Env, c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::RefreshFxQuoteStatus)?;
        if c.fx_resolved {
            return Err(Error::FxAlreadyResolved);
        }
        if !env.storage().instance().has(&DataKey::OracleConfig) {
            return Err(Error::OracleNotConfigured);
        }
        Ok(())
    }

    fn can_fund_fx_shortfall(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::FundFxShortfall)?;
        if !c.fx_resolved {
            return Err(Error::FxNotResolved);
        }
        if c.fx_shortfall_amount == 0 {
            return Err(Error::NoFxShortfall);
        }
        if c.fx_shortfall_funded {
            return Err(Error::FxShortfallAlreadyFunded);
        }
        if now > c.fx_shortfall_deadline {
            return Err(Error::FxShortfallWindowPassed);
        }
        Ok(())
    }

    fn can_expire_fx_shortfall_window(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::ExpireFxShortfallWindow)?;
        if !c.fx_resolved {
            return Err(Error::FxNotResolved);
        }
        if c.fx_shortfall_amount == 0 {
            return Err(Error::NoFxShortfall);
        }
        if c.fx_shortfall_funded {
            return Err(Error::FxShortfallAlreadyFunded);
        }
        if now <= c.fx_shortfall_deadline {
            return Err(Error::FxShortfallWindowNotPassed);
        }
        Ok(())
    }

    fn can_cancel(c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::Cancel)
    }

    fn can_reassign_buyer(c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::ReassignBuyer)
    }

    fn can_flag_dispute(c: &Commitment, flagger: &Address) -> Result<(), Error> {
        Self::require_status(c, Method::FlagDispute)?;
        // Kept as the same Address PartialEq expression the #122 audit pins
        // in tests/address_equality.rs, now in one place for the view and the
        // real guard.
        let flagger = flagger.clone();
        if flagger != c.buyer && flagger != c.cooperative && flagger != c.warehouse_operator {
            return Err(Error::NotAParty);
        }
        if c.dispute_count >= MAX_DISPUTE_FLAGS {
            return Err(Error::DisputeCapReached);
        }
        Ok(())
    }

    fn can_resolve_dispute(c: &Commitment) -> Result<(), Error> {
        Self::require_status(c, Method::ResolveDispute)
    }

    fn can_expire_dispute_window(c: &Commitment, now: u64) -> Result<(), Error> {
        Self::require_status(c, Method::ExpireDisputeWindow)?;
        if now <= c.dispute_deadline {
            return Err(Error::DisputeWindowNotPassed);
        }
        Ok(())
    }

    /// The one place `status` is assigned after `initialize` — assigns and
    /// publishes the `status_changed` event together, so the "every real
    /// transition emits an event" property in `StatusChanged`'s doc holds by
    /// construction, not by convention.
    fn set_status(env: &Env, c: &mut Commitment, next: Status) {
        let from = c.status;
        c.status = next;
        StatusChanged { from, to: next }.publish(env);
    }

    fn bps_amount(total: i128, bps: u32) -> i128 {
        total * i128::from(bps) / 10_000
    }

    fn checked_fx_conversion(ngn_owed: i128, price: i128, decimals: u32) -> Result<i128, Error> {
        let numerator = ngn_owed
            .checked_mul(price)
            .ok_or(Error::OracleConversionOutOfRange)?;
        let divisor = 10i128
            .checked_pow(decimals)
            .ok_or(Error::OracleConversionOutOfRange)?;
        Ok(numerator / divisor)
    }

    /// Calls `require_auth()` once per distinct address, in first-seen order.
    ///
    /// Soroban's auth manager rejects a *second* `require_auth()` for the same
    /// address within a single call frame: the frame is already marked
    /// authorized and there is no further matching auth entry left to consume,
    /// so the host raises `Auth/InvalidAction` and the whole invocation aborts.
    /// When one address legitimately holds two of a commitment's named roles —
    /// a cooperative that also operates the warehouse, the realistic
    /// two-of-three case — blindly asking twice turns a valid single-signature
    /// authorization into a hard failure. Every multi-party check in this
    /// contract routes through here rather than calling `require_auth()`
    /// directly. The behavior is pinned by the `#139`/`#140` auth tests, which
    /// exercise it against genuinely distinct `MockAuth` entries instead of
    /// `mock_all_auths()` (whose recording mode silently tolerates the
    /// duplicate).
    fn require_once_each(addresses: &[&Address]) {
        for (i, addr) in addresses.iter().enumerate() {
            if addresses[..i].iter().all(|seen| seen != addr) {
                addr.require_auth();
            }
        }
    }

    /// The portion of `total_amount` that `lock` actually escrows —
    /// `advance1_bps + advance2_bps`. See module docs for why funding is
    /// split into this plus `fund_remainder`'s share instead of all at
    /// `lock`, the way earlier versions of this contract worked.
    ///
    /// Rounding: this floors the combined basis points once, while each
    /// tranche payout floors separately, so up to one token unit per
    /// commitment can stay in the contract after both advances are claimed.
    /// It is never lost — every terminal path sweeps `balance()`: `cancel`
    /// and `reclaim_on_nondelivery` refund it to the buyer, `settle` refunds
    /// it to the buyer unless the settlement obligation still exceeds the
    /// balance (then it goes to the cooperative with the rest), and
    /// `expire_remainder_window`/`expire_fx_shortfall_window` sweep it to the
    /// cooperative. Pinned by `tests/rounding_dust.rs` (#259).
    fn deposit_amount(c: &Commitment) -> i128 {
        Self::bps_amount(
            c.total_amount,
            c.advance1_bps.saturating_add(c.advance2_bps),
        )
    }

    /// What `fund_remainder` still has to escrow: `total_amount` minus the
    /// deposit `lock` already took. Zero exactly when the two advances add up
    /// to 10_000 bps on an amount large enough for the floor in
    /// `deposit_amount` to be exact — the fully-prepaid case #275 is about,
    /// where `ready_for_delivery` marks the remainder funded rather than
    /// asking the buyer for a zero-value transfer.
    fn remainder_amount(c: &Commitment) -> i128 {
        c.total_amount - Self::deposit_amount(c)
    }

    /// Sum of advance tranches already claimed by the cooperative (#285).
    fn claimed_by_coop(c: &Commitment) -> i128 {
        (if c.advance1_claimed {
            Self::bps_amount(c.total_amount, c.advance1_bps)
        } else {
            0
        }) + (if c.advance2_claimed {
            Self::bps_amount(c.total_amount, c.advance2_bps)
        } else {
            0
        })
    }

    /// Sum of advance tranches already reclaimed by the buyer after expiry (#285).
    fn reclaimed_by_buyer(c: &Commitment) -> i128 {
        (if c.advance1_expired {
            Self::bps_amount(c.total_amount, c.advance1_bps)
        } else {
            0
        }) + (if c.advance2_expired {
            Self::bps_amount(c.total_amount, c.advance2_bps)
        } else {
            0
        })
    }

    /// Field accessors for a given tranche, so `claim_tranche`/
    /// `reclaim_tranche`/`open_tranche` have one implementation shared by
    /// both `_1` and `_2` entry points instead of two copies that could
    /// silently drift apart.
    fn tranche_bps(c: &Commitment, t: Tranche) -> &u32 {
        match t {
            Tranche::One => &c.advance1_bps,
            Tranche::Two => &c.advance2_bps,
        }
    }

    fn tranche_mut(c: &mut Commitment, t: Tranche) -> TrancheRef<'_> {
        match t {
            Tranche::One => TrancheRef {
                deadline: &mut c.advance1_deadline,
                claimed: &mut c.advance1_claimed,
                expired: &mut c.advance1_expired,
            },
            Tranche::Two => TrancheRef {
                deadline: &mut c.advance2_deadline,
                claimed: &mut c.advance2_claimed,
                expired: &mut c.advance2_expired,
            },
        }
    }

    /// The transition-table row for opening a tranche's window. Which
    /// status admits each tranche is declared in `Method::allowed_from`, not
    /// here — this only names which row applies.
    fn open_tranche_method(t: Tranche) -> Method {
        match t {
            Tranche::One => Method::ReleaseAdvance1,
            Tranche::Two => Method::ReleaseAdvance2,
        }
    }

    /// The transition-table row for claiming a tranche. That row is
    /// deliberately ungated (`None`) — see `Method`'s doc — so this is only
    /// the name of the row, not an extra status rule.
    fn claim_tranche_method(t: Tranche) -> Method {
        match t {
            Tranche::One => Method::ClaimAdvance1,
            Tranche::Two => Method::ClaimAdvance2,
        }
    }

    /// The transition-table row for reclaiming a tranche; ungated for the
    /// same reason as `claim_tranche_method`.
    fn reclaim_tranche_method(t: Tranche) -> Method {
        match t {
            Tranche::One => Method::ReclaimAdvance1,
            Tranche::Two => Method::ReclaimAdvance2,
        }
    }

    fn tranche_next_status(t: Tranche) -> Status {
        match t {
            Tranche::One => Status::Advance1Released,
            Tranche::Two => Status::Advance2Released,
        }
    }

    fn can_open_tranche(c: &Commitment, t: Tranche) -> Result<(), Error> {
        Self::require_status(c, Self::open_tranche_method(t))
    }

    fn can_claim_tranche(c: &mut Commitment, t: Tranche, now: u64) -> Result<(), Error> {
        // Asks the table for this method's status rule (which is `None`:
        // an opened window stays claimable whatever the status has since
        // become) so every public method's rule is consulted in one place.
        Self::require_status(c, Self::claim_tranche_method(t))?;
        let tranche = Self::tranche_mut(c, t);
        if *tranche.deadline == 0 {
            return Err(Error::NotYetOpened);
        }
        if *tranche.claimed {
            return Err(Error::AlreadyClaimed);
        }
        if *tranche.expired {
            return Err(Error::AlreadyExpired);
        }
        if now > *tranche.deadline {
            return Err(Error::ClaimWindowPassed);
        }
        Ok(())
    }

    fn can_reclaim_tranche(c: &mut Commitment, t: Tranche, now: u64) -> Result<(), Error> {
        // Ungated row, same reasoning as `can_claim_tranche` above.
        Self::require_status(c, Self::reclaim_tranche_method(t))?;
        let tranche = Self::tranche_mut(c, t);
        if *tranche.deadline == 0 {
            return Err(Error::NotYetOpened);
        }
        if *tranche.claimed {
            return Err(Error::AlreadyClaimed);
        }
        if *tranche.expired {
            return Err(Error::AlreadyExpired);
        }
        if now <= *tranche.deadline {
            return Err(Error::ClaimWindowNotPassed);
        }
        Ok(())
    }

    fn open_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        Self::can_open_tranche(&c, t)?;
        let deadline = env.ledger().timestamp() + c.claim_window_secs;
        *Self::tranche_mut(&mut c, t).deadline = deadline;
        Self::set_status(env, &mut c, Self::tranche_next_status(t));
        Self::save(env, &c);
        Ok(())
    }

    fn claim_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        c.cooperative.require_auth();
        Self::can_claim_tranche(&mut c, t, env.ledger().timestamp())?;

        let tranche_num = match t {
            Tranche::One => 1u32,
            Tranche::Two => 2u32,
        };
        let amount = Self::bps_amount(c.total_amount, *Self::tranche_bps(&c, t));
        // Effects before interaction — see `lock`'s comment for why.
        *Self::tranche_mut(&mut c, t).claimed = true;
        Self::save(env, &c);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.cooperative,
                &amount,
            );
        }
        TrancheClaimed {
            tranche: tranche_num,
            to: c.cooperative.clone(),
            amount,
        }
        .publish(env);
        Ok(())
    }

    fn reclaim_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        // After terminal statuses the balance has already been swept;
        // a claim/reclaim would reach token.transfer and trap on insufficient
        // balance. Reject with a clean contract error rather than a token-level
        // trap. Disputed is deliberately excluded — see dispute-stalling.md §2.
        match c.status {
            Status::Cancelled | Status::Forfeited | Status::Defaulted => {
                return Err(Error::InvalidState);
            }
            _ => {}
        }
        c.buyer.require_auth();
        Self::can_reclaim_tranche(&mut c, t, env.ledger().timestamp())?;

        let tranche_num = match t {
            Tranche::One => 1u32,
            Tranche::Two => 2u32,
        };
        let amount = Self::bps_amount(c.total_amount, *Self::tranche_bps(&c, t));
        // Effects before interaction — see `lock`'s comment for why.
        *Self::tranche_mut(&mut c, t).expired = true;
        Self::save(env, &c);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.buyer,
                &amount,
            );
        }
        TrancheReclaimed {
            tranche: tranche_num,
            to: c.buyer.clone(),
            amount,
        }
        .publish(env);
        Ok(())
    }
}

#[cfg(test)]
mod test;
