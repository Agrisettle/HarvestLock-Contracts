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

#![no_std]

mod reflector;

use reflector::{Asset as ReflectorAsset, ReflectorPulseClient};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, BytesN, Env, Symbol, Vec,
};

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
    RemainderWindowPassed = 14,
    RemainderWindowNotPassed = 15,
    /// `confirm_delivery` called while `ReadyForDelivery` but the buyer
    /// hasn't funded the remainder yet.
    RemainderNotFunded = 16,
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
    /// `initialize`'s `oracle_config` was `Some` with `max_age_secs == 0`
    /// — a zero staleness bound would reject every real quote, which is
    /// never what a caller actually wants (that's what leaving
    /// `oracle_config` as `None` is for).
    InvalidOracleConfig = 24,
    /// `oracle_rate` called but `initialize` never set an `oracle_config`
    /// for this commitment.
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
    /// `fund_fx_shortfall` called after `fx_shortfall_deadline` passed —
    /// too late to cure, `expire_fx_shortfall_window` is the only path
    /// left from here.
    FxShortfallWindowPassed = 33,
    /// `expire_fx_shortfall_window` called before the deadline.
    FxShortfallWindowNotPassed = 34,
    /// `flag_dispute`'s `flagger` argument wasn't the buyer, cooperative,
    /// or warehouse operator — only a named party to the deal can flag
    /// one, not an arbitrary third address.
    NotAParty = 35,
    /// `resolve_dispute`/`expire_dispute_window` called before
    /// `dispute_deadline` passed (the latter only) — or, for either
    /// call, while `status` isn't `Disputed` at all (covered by the same
    /// `InvalidState` every other status-gated call already uses).
    DisputeWindowNotPassed = 36,
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
    /// make concrete. Using the same 7-decimal convention on both sides
    /// of the multiplication is what lets `resolve_fx_shortfall`'s
    /// conversion formula skip a separate unit-normalization step — see
    /// its doc comment for the worked example.
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

#[contracttype]
#[derive(Clone)]
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

    /// 0 = not yet opened. Set by `ready_for_delivery`.
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
    pub fn initialize(
        env: Env,
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
        // Some if this deal needs the oracle_rate conversion path — see
        // the module doc. None for a plain deal.
        oracle_config: Option<OracleConfig>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Commitment) {
            return Err(Error::AlreadyInitialized);
        }
        if total_amount <= 0 {
            return Err(Error::ZeroAmount);
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
        if let Some(oc) = &oracle_config {
            if oc.max_age_secs == 0 {
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
            dispute_pre_status: Status::Draft,
            dispute_deadline: 0,
        };
        env.storage()
            .instance()
            .set(&DataKey::Commitment, &commitment);
        if let Some(oc) = oracle_config {
            env.storage().instance().set(&DataKey::OracleConfig, &oc);
        }
        Ok(())
    }

    /// The configured oracle, if `initialize` set one.
    pub fn get_oracle_config(env: Env) -> Result<Option<OracleConfig>, Error> {
        let _ = Self::load(&env)?; // NotInitialized if the commitment itself doesn't exist
        Ok(env.storage().instance().get(&DataKey::OracleConfig))
    }

    /// Reads the current rate for this commitment's configured oracle
    /// asset, enforcing the staleness bound. A pure read against
    /// Reflector's own stored history, not a state transition — callable
    /// in any status, including after `Settled`.
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
    fn read_oracle_rate(env: &Env, config: &OracleConfig) -> Result<OracleRate, Error> {
        let client = ReflectorPulseClient::new(env, &config.oracle_contract);
        let quote = client
            .lastprice(&ReflectorAsset::Other(config.price_asset.clone()))
            .ok_or(Error::OraclePriceUnavailable)?;

        let now = env.ledger().timestamp();
        let age = now.saturating_sub(quote.timestamp);
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
    /// Deliberately **not** required before `lock`: a solo-farmer
    /// commitment with no cooperative pooling shouldn't be forced through
    /// a ledger step that doesn't apply to it, and gating `lock` on this
    /// would be a real behavior change affecting every existing
    /// commitment shape, not a decision this session is making
    /// unilaterally. Cooperative-gated, not buyer-consented — this is the
    /// cooperative's own membership data, not a term the buyer negotiates.
    pub fn set_allocation(env: Env, members: Vec<AllocationMember>) -> Result<(), Error> {
        let c = Self::load(&env)?;
        if c.status != Status::Draft {
            return Err(Error::InvalidState);
        }
        if env.storage().instance().has(&DataKey::Allocation) {
            return Err(Error::AllocationAlreadySet);
        }
        if members.is_empty() {
            return Err(Error::InvalidAllocation);
        }
        let mut total_bps: u32 = 0;
        for m in members.iter() {
            total_bps = total_bps.saturating_add(m.share_bps);
        }
        if total_bps > 10_000 {
            return Err(Error::InvalidAllocation);
        }
        c.cooperative.require_auth();

        env.storage().instance().set(&DataKey::Allocation, &members);
        Ok(())
    }

    /// The recorded allocation ledger, if `set_allocation` has run.
    pub fn get_allocation(env: Env) -> Result<Vec<AllocationMember>, Error> {
        let _ = Self::load(&env)?; // NotInitialized if the commitment itself doesn't exist
        env.storage()
            .instance()
            .get(&DataKey::Allocation)
            .ok_or(Error::AllocationNotSet)
    }

    /// Draft -> Locked. Pulls the buyer's **deposit** into the contract —
    /// `advance1_bps + advance2_bps` of `total_amount`, not the full
    /// amount. See module docs for why this changed from earlier versions
    /// of this contract, which escrowed everything here.
    ///
    /// The buyer must have already approved this contract to transfer at
    /// least the deposit amount of `token` on their behalf (standard
    /// SEP-41 token `approve`), or this call fails at the token contract,
    /// not here.
    pub fn lock(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Draft {
            return Err(Error::InvalidState);
        }
        c.buyer.require_auth();

        let deposit = Self::deposit_amount(&c);

        // State is updated and saved *before* the external transfer
        // (checks-effects-interactions): if a reentrant call somehow
        // landed mid-transfer, it would see `Locked` already and be
        // rejected by the guard above, instead of being able to pull the
        // deposit a second time before this invocation's save lands.
        // The standard SAC/native token used here has no such hook — this
        // is defensive hardening against a future token type, not a
        // response to a demonstrated exploit against the current one.
        c.status = Status::Locked;
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
    pub fn release_advance_1(env: Env) -> Result<(), Error> {
        Self::open_tranche(&env, Tranche::One, Status::Locked, Status::Advance1Released)
    }

    /// Cooperative claims tranche 1, if within the window.
    pub fn claim_advance_1(env: Env) -> Result<(), Error> {
        Self::claim_tranche(&env, Tranche::One)
    }

    /// Buyer reclaims tranche 1, if the window has passed unclaimed.
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
        if c.status != Status::Advance1Released {
            return Err(Error::InvalidState);
        }
        c.warehouse_operator.require_auth();
        c.status = Status::CheckpointPassed;
        Self::save(&env, &c);
        Ok(())
    }

    /// CheckpointPassed -> Advance2Released. Same mechanics as
    /// `release_advance_1`, for tranche 2.
    pub fn release_advance_2(env: Env) -> Result<(), Error> {
        Self::open_tranche(
            &env,
            Tranche::Two,
            Status::CheckpointPassed,
            Status::Advance2Released,
        )
    }

    pub fn claim_advance_2(env: Env) -> Result<(), Error> {
        Self::claim_tranche(&env, Tranche::Two)
    }

    pub fn reclaim_advance_2(env: Env) -> Result<(), Error> {
        Self::reclaim_tranche(&env, Tranche::Two)
    }

    /// Advance2Released -> ReadyForDelivery. The cooperative signaling
    /// "setting out for delivery" — opens the remainder-payment window
    /// (`remainder_window_secs` from now). Auth-gated, unlike
    /// `release_advance_*`: this one has real stakes for the buyer (it
    /// starts the clock that can end in `Defaulted`), so it shouldn't be
    /// triggerable by just anyone.
    pub fn ready_for_delivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Advance2Released {
            return Err(Error::InvalidState);
        }
        c.cooperative.require_auth();
        c.remainder_deadline = env.ledger().timestamp() + c.remainder_window_secs;
        c.status = Status::ReadyForDelivery;
        Self::save(&env, &c);
        Ok(())
    }

    /// Buyer escrows `total_amount - deposit` — the second half of
    /// two-phase funding. Must happen within the window
    /// `ready_for_delivery` opened, or it's a default (see
    /// `expire_remainder_window`).
    pub fn fund_remainder(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::ReadyForDelivery {
            return Err(Error::InvalidState);
        }
        if c.remainder_funded {
            return Err(Error::RemainderAlreadyFunded);
        }
        if env.ledger().timestamp() > c.remainder_deadline {
            return Err(Error::RemainderWindowPassed);
        }
        c.buyer.require_auth();

        let remainder = c.total_amount - Self::deposit_amount(&c);

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
        Ok(())
    }

    /// The buyer-default path: `remainder_deadline` passed with the
    /// remainder never funded. Sweeps whatever's currently escrowed (the
    /// deposit, or whatever of it the cooperative hasn't already claimed)
    /// to the cooperative and sets `Defaulted` — uncontested by
    /// construction, so unlike `cancel`/`reassign_buyer` this needs no
    /// consent from anyone.
    ///
    /// Permissionless, deliberately: the outcome (sweep to cooperative)
    /// doesn't depend on who calls it, only on whether the deadline has
    /// passed, the same reasoning `reclaim_tranche` would use if it
    /// weren't already scoped to benefit the buyer specifically. Anyone —
    /// including an off-chain watcher — can trigger it once the fact of
    /// the matter (deadline passed, unfunded) is true.
    pub fn expire_remainder_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::ReadyForDelivery {
            return Err(Error::InvalidState);
        }
        if c.remainder_funded {
            return Err(Error::RemainderAlreadyFunded);
        }
        if env.ledger().timestamp() <= c.remainder_deadline {
            return Err(Error::RemainderWindowNotPassed);
        }

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Defaulted;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
        }
        Ok(())
    }

    /// The seller-non-delivery path: `delivery_deadline` passed without
    /// `confirm_delivery` ever having run. Returns whatever's currently
    /// escrowed to the buyer and sets `Forfeited`.
    ///
    /// Buyer-gated, not permissionless — unlike `expire_remainder_window`,
    /// the beneficiary here is a specific party (the buyer) reclaiming
    /// what's rightfully theirs, the same reasoning `reclaim_tranche` uses.
    ///
    /// Not reachable from `Draft`: nothing's ever been escrowed at that
    /// point, so there's nothing to reclaim and no real event has
    /// occurred worth recording as a forfeiture.
    pub fn reclaim_on_nondelivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery => {}
            _ => return Err(Error::InvalidState),
        }
        if env.ledger().timestamp() <= c.delivery_deadline {
            return Err(Error::DeliveryDeadlineNotPassed);
        }
        c.buyer.require_auth();

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Forfeited;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
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
    pub fn confirm_delivery(env: Env, delivered_quantity: u32, grade_index: u32) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::ReadyForDelivery {
            return Err(Error::InvalidState);
        }
        if !c.remainder_funded {
            return Err(Error::RemainderNotFunded);
        }
        let Some(grade_bps) = c.grade_price_bps.get(grade_index) else {
            return Err(Error::InvalidGradeIndex);
        };
        c.warehouse_operator.require_auth();

        let quantity_bps = (delivered_quantity.min(c.contracted_quantity) as i128 * 10_000
            / c.contracted_quantity as i128) as u32;
        c.delivered_quantity = delivered_quantity;
        c.grade_index = grade_index;
        c.settlement_bps = (quantity_bps as i128 * grade_bps as i128 / 10_000) as u32;
        c.status = Status::Delivered;
        Self::save(&env, &c);
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
    pub fn settle(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        if !c.advance1_claimed && !c.advance1_expired {
            return Err(Error::TrancheUnresolved);
        }
        if !c.advance2_claimed && !c.advance2_expired {
            return Err(Error::TrancheUnresolved);
        }
        let has_oracle = env.storage().instance().has(&DataKey::OracleConfig);
        if has_oracle {
            if !c.fx_resolved {
                return Err(Error::FxNotResolved);
            }
            if c.fx_shortfall_amount > 0 && !c.fx_shortfall_funded {
                return Err(Error::FxShortfallUnfunded);
            }
        }

        let adjusted_total = if has_oracle {
            c.fx_adjusted_total
        } else {
            Self::bps_amount(c.total_amount, c.settlement_bps)
        };
        let claimed_by_coop = (if c.advance1_claimed {
            Self::bps_amount(c.total_amount, c.advance1_bps)
        } else {
            0
        }) + (if c.advance2_claimed {
            Self::bps_amount(c.total_amount, c.advance2_bps)
        } else {
            0
        });
        let owed_from_remainder = (adjusted_total - claimed_by_coop).max(0);

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Settled;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let remainder_balance = token_client.balance(&env.current_contract_address());
        let coop_payment = owed_from_remainder.min(remainder_balance);
        let buyer_refund = remainder_balance - coop_payment;

        if coop_payment > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &coop_payment);
        }
        if buyer_refund > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &buyer_refund);
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
    /// Requires both tranches already resolved (same guard `settle`
    /// itself has) before reading `claimed_by_coop` — computing this
    /// against a still-open tranche would use figures that could still
    /// change by the time `settle` actually runs. Permissionless, same
    /// reasoning as `release_advance_*`: this only computes and records
    /// a number, it doesn't move funds or grant anyone anything they
    /// weren't already entitled to. One-time (`FxAlreadyResolved`) —
    /// see the module doc for why re-resolving isn't allowed.
    pub fn resolve_fx_shortfall(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        if !c.advance1_claimed && !c.advance1_expired {
            return Err(Error::TrancheUnresolved);
        }
        if !c.advance2_claimed && !c.advance2_expired {
            return Err(Error::TrancheUnresolved);
        }
        if c.fx_resolved {
            return Err(Error::FxAlreadyResolved);
        }
        let oracle_config: OracleConfig = env
            .storage()
            .instance()
            .get(&DataKey::OracleConfig)
            .ok_or(Error::OracleNotConfigured)?;

        let rate = Self::read_oracle_rate(&env, &oracle_config)?;
        let decimals = ReflectorPulseClient::new(&env, &oracle_config.oracle_contract).decimals();

        let ngn_owed = Self::bps_amount(oracle_config.denominated_amount, c.settlement_bps);
        let fx_adjusted_total = ngn_owed * rate.price / 10i128.pow(decimals);

        let claimed_by_coop = (if c.advance1_claimed {
            Self::bps_amount(c.total_amount, c.advance1_bps)
        } else {
            0
        }) + (if c.advance2_claimed {
            Self::bps_amount(c.total_amount, c.advance2_bps)
        } else {
            0
        });
        let owed_from_remainder = (fx_adjusted_total - claimed_by_coop).max(0);
        let available_balance = token::Client::new(&env, &c.token).balance(&env.current_contract_address());
        let shortfall = (owed_from_remainder - available_balance).max(0);

        c.fx_adjusted_total = fx_adjusted_total;
        c.fx_resolved = true;
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
        Ok(())
    }

    /// Buyer pays in the shortfall `resolve_fx_shortfall` computed.
    /// Buyer-gated, matching `fund_remainder`'s own reasoning: this is
    /// new money the buyer specifically owes, not a mechanical step
    /// anyone could trigger.
    pub fn fund_fx_shortfall(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        if !c.fx_resolved {
            return Err(Error::FxNotResolved);
        }
        if c.fx_shortfall_amount == 0 {
            return Err(Error::NoFxShortfall);
        }
        if c.fx_shortfall_funded {
            return Err(Error::FxShortfallAlreadyFunded);
        }
        if env.ledger().timestamp() > c.fx_shortfall_deadline {
            return Err(Error::FxShortfallWindowPassed);
        }
        c.buyer.require_auth();

        let amount = c.fx_shortfall_amount;
        // Effects before interaction — see `lock`'s comment for why.
        c.fx_shortfall_funded = true;
        Self::save(&env, &c);

        token::Client::new(&env, &c.token).transfer(&c.buyer, env.current_contract_address(), &amount);
        Ok(())
    }

    /// The FX-shortfall buyer-default path: `fx_shortfall_deadline`
    /// passed with the top-up never funded. Sweeps whatever's currently
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
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        if !c.fx_resolved {
            return Err(Error::FxNotResolved);
        }
        if c.fx_shortfall_amount == 0 {
            return Err(Error::NoFxShortfall);
        }
        if c.fx_shortfall_funded {
            return Err(Error::FxShortfallAlreadyFunded);
        }
        if env.ledger().timestamp() <= c.fx_shortfall_deadline {
            return Err(Error::FxShortfallWindowNotPassed);
        }

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Defaulted;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
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
    /// transfer both land on the ledger from this call — no separate event
    /// needed, same as every other transition here.
    pub fn cancel(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Draft
            | Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery => {}
            _ => return Err(Error::InvalidState),
        }
        c.buyer.require_auth();
        c.cooperative.require_auth();

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Cancelled;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
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
    pub fn reassign_buyer(env: Env, new_buyer: Address) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Draft
            | Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery => {}
            _ => return Err(Error::InvalidState),
        }
        c.buyer.require_auth();
        c.cooperative.require_auth();
        new_buyer.require_auth();

        c.buyer = new_buyer;
        Self::save(&env, &c);
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
    pub fn flag_dispute(env: Env, flagger: Address) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery
            | Status::Delivered => {}
            _ => return Err(Error::InvalidState),
        }
        if flagger != c.buyer && flagger != c.cooperative && flagger != c.warehouse_operator {
            return Err(Error::NotAParty);
        }
        flagger.require_auth();

        c.dispute_pre_status = c.status;
        c.dispute_deadline = env.ledger().timestamp() + c.remainder_window_secs;
        c.status = Status::Disputed;
        Self::save(&env, &c);
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
    pub fn resolve_dispute(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Disputed {
            return Err(Error::InvalidState);
        }
        c.buyer.require_auth();
        c.cooperative.require_auth();
        c.warehouse_operator.require_auth();

        c.status = c.dispute_pre_status;
        c.dispute_pre_status = Status::Draft;
        c.dispute_deadline = 0;
        Self::save(&env, &c);
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
    pub fn expire_dispute_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Disputed {
            return Err(Error::InvalidState);
        }
        if env.ledger().timestamp() <= c.dispute_deadline {
            return Err(Error::DisputeWindowNotPassed);
        }

        c.status = c.dispute_pre_status;
        c.dispute_pre_status = Status::Draft;
        c.dispute_deadline = 0;
        Self::save(&env, &c);
        Ok(())
    }

    pub fn get_status(env: Env) -> Result<Status, Error> {
        Ok(Self::load(&env)?.status)
    }

    pub fn get_commitment(env: Env) -> Result<Commitment, Error> {
        Self::load(&env)
    }

    // -- internal --

    fn load(env: &Env) -> Result<Commitment, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Commitment)
            .ok_or(Error::NotInitialized)
    }

    fn save(env: &Env, c: &Commitment) {
        env.storage().instance().set(&DataKey::Commitment, c);
    }

    fn bps_amount(total: i128, bps: u32) -> i128 {
        total * (bps as i128) / 10_000
    }

    /// The portion of `total_amount` that `lock` actually escrows —
    /// `advance1_bps + advance2_bps`. See module docs for why funding is
    /// split into this plus `fund_remainder`'s share instead of all at
    /// `lock`, the way earlier versions of this contract worked.
    fn deposit_amount(c: &Commitment) -> i128 {
        Self::bps_amount(
            c.total_amount,
            c.advance1_bps.saturating_add(c.advance2_bps),
        )
    }

    /// Field accessors for a given tranche, so `claim_tranche`/
    /// `reclaim_tranche`/`open_tranche` have one implementation shared by
    /// both `_1` and `_2` entry points instead of two copies that could
    /// silently drift apart.
    fn tranche_bps(c: &Commitment, t: Tranche) -> u32 {
        match t {
            Tranche::One => c.advance1_bps,
            Tranche::Two => c.advance2_bps,
        }
    }

    fn tranche_deadline(c: &Commitment, t: Tranche) -> u64 {
        match t {
            Tranche::One => c.advance1_deadline,
            Tranche::Two => c.advance2_deadline,
        }
    }

    fn tranche_claimed(c: &Commitment, t: Tranche) -> bool {
        match t {
            Tranche::One => c.advance1_claimed,
            Tranche::Two => c.advance2_claimed,
        }
    }

    fn tranche_expired(c: &Commitment, t: Tranche) -> bool {
        match t {
            Tranche::One => c.advance1_expired,
            Tranche::Two => c.advance2_expired,
        }
    }

    fn set_tranche_deadline(c: &mut Commitment, t: Tranche, deadline: u64) {
        match t {
            Tranche::One => c.advance1_deadline = deadline,
            Tranche::Two => c.advance2_deadline = deadline,
        }
    }

    fn set_tranche_claimed(c: &mut Commitment, t: Tranche) {
        match t {
            Tranche::One => c.advance1_claimed = true,
            Tranche::Two => c.advance2_claimed = true,
        }
    }

    fn set_tranche_expired(c: &mut Commitment, t: Tranche) {
        match t {
            Tranche::One => c.advance1_expired = true,
            Tranche::Two => c.advance2_expired = true,
        }
    }

    fn open_tranche(env: &Env, t: Tranche, required: Status, next: Status) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        if c.status != required {
            return Err(Error::InvalidState);
        }
        let deadline = env.ledger().timestamp() + c.claim_window_secs;
        Self::set_tranche_deadline(&mut c, t, deadline);
        c.status = next;
        Self::save(env, &c);
        Ok(())
    }

    fn claim_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        c.cooperative.require_auth();

        if Self::tranche_deadline(&c, t) == 0 {
            return Err(Error::NotYetOpened);
        }
        if Self::tranche_claimed(&c, t) {
            return Err(Error::AlreadyClaimed);
        }
        if Self::tranche_expired(&c, t) {
            return Err(Error::AlreadyExpired);
        }
        if env.ledger().timestamp() > Self::tranche_deadline(&c, t) {
            return Err(Error::ClaimWindowPassed);
        }

        let amount = Self::bps_amount(c.total_amount, Self::tranche_bps(&c, t));
        // Effects before interaction — see `lock`'s comment for why.
        Self::set_tranche_claimed(&mut c, t);
        Self::save(env, &c);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.cooperative,
                &amount,
            );
        }
        Ok(())
    }

    fn reclaim_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        c.buyer.require_auth();

        if Self::tranche_deadline(&c, t) == 0 {
            return Err(Error::NotYetOpened);
        }
        if Self::tranche_claimed(&c, t) {
            return Err(Error::AlreadyClaimed);
        }
        if Self::tranche_expired(&c, t) {
            return Err(Error::AlreadyExpired);
        }
        if env.ledger().timestamp() <= Self::tranche_deadline(&c, t) {
            return Err(Error::ClaimWindowNotPassed);
        }

        let amount = Self::bps_amount(c.total_amount, Self::tranche_bps(&c, t));
        // Effects before interaction — see `lock`'s comment for why.
        Self::set_tranche_expired(&mut c, t);
        Self::save(env, &c);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.buyer,
                &amount,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
