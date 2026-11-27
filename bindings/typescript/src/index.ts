import { Buffer } from "buffer";
import { Address } from "@stellar/stellar-sdk";
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from "@stellar/stellar-sdk/contract";
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  Option,
  Timepoint,
  Duration,
} from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";

if (typeof window !== "undefined") {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}




export const Errors = {
  1: {message:"AlreadyInitialized"},
  2: {message:"NotInitialized"},
  3: {message:"InvalidState"},
  4: {message:"InvalidBps"},
  5: {message:"ZeroAmount"},
  6: {message:"InvalidWindow"},
  7: {message:"NotYetOpened"},
  8: {message:"AlreadyClaimed"},
  9: {message:"AlreadyExpired"},
  10: {message:"ClaimWindowPassed"},
  11: {message:"ClaimWindowNotPassed"},
  /**
   * A tranche is neither claimed nor expired yet — `settle` refuses to
   * run until someone resolves it via `claim_advance_*` or
   * `reclaim_advance_*`. See `settle`'s doc comment for why this is a
   * hard requirement, not just a nicety.
   */
  12: {message:"TrancheUnresolved"},
  13: {message:"RemainderAlreadyFunded"},
  /**
   * `fund_remainder` called after `remainder_deadline` has *strictly*
   * passed — the deadline second itself is still in time.
   */
  14: {message:"RemainderWindowPassed"},
  /**
   * `expire_remainder_window` called at or before
   * `remainder_deadline` — the deadline second itself still belongs to
   * the buyer's funding window, so expiry needs strictly more than the
   * deadline, not merely the deadline.
   */
  15: {message:"RemainderWindowNotPassed"},
  /**
   * `confirm_delivery` called while `ReadyForDelivery` but the buyer
   * hasn't funded the remainder yet.
   */
  16: {message:"RemainderNotFunded"},
  /**
   * `reclaim_on_nondelivery` called at or before `delivery_deadline` —
   * the deadline second itself still belongs to the cooperative.
   */
  17: {message:"DeliveryDeadlineNotPassed"},
  /**
   * `contracted_quantity` was zero at `initialize`.
   */
  18: {message:"InvalidQuantity"},
  /**
   * `grade_price_bps` was empty, or contained an entry over 10_000
   * (a grade worth more than the pre-agreed full unit price) at
   * `initialize`.
   */
  19: {message:"InvalidGradeSchedule"},
  /**
   * `confirm_delivery`'s `grade_index` didn't land inside
   * `grade_price_bps`.
   */
  20: {message:"InvalidGradeIndex"},
  /**
   * `set_allocation` called a second time — the allocation ledger is
   * one-time and immutable once recorded.
   */
  21: {message:"AllocationAlreadySet"},
  /**
   * `set_allocation`'s `members` was empty, or the sum of every
   * entry's `share_bps` exceeded 10_000.
   */
  22: {message:"InvalidAllocation"},
  /**
   * `get_allocation` called before `set_allocation` ever ran.
   */
  23: {message:"AllocationNotSet"},
  /**
   * `initialize`'s `oracle_config` was `Some` with either:
   * - `max_age_secs == 0`: a zero staleness bound would reject every
   * real quote, which is never what a caller actually wants (that's
   * what leaving `oracle_config` as `None` is for); or
   * - `denominated_amount <= 0` or `> MAX_DENOMINATED_AMOUNT` (#247):
   * a non-positive value would make `resolve_fx_shortfall` owe the
   * cooperative nothing after a full delivery, and an oversized one
   * would overflow `bps_amount` and trap at settlement.
   */
  24: {message:"InvalidOracleConfig"},
  /**
   * `oracle_rate` or `resolve_fx_shortfall` called but `initialize`
   * never set an `oracle_config` for this commitment — the only two
   * functions that read the oracle at all (#124).
   */
  25: {message:"OracleNotConfigured"},
  /**
   * The configured Reflector oracle returned `None` for
   * `oracle_config.price_asset` — it doesn't quote that asset at all
   * (call the oracle's own `assets()` to check what it does quote).
   */
  26: {message:"OraclePriceUnavailable"},
  /**
   * The most recent quote is older than `oracle_config.max_age_secs`
   * allows (PRD §16.3's staleness bound) — refused rather than
   * returned, since a caller silently getting a too-old rate is worse
   * than getting no rate at all.
   */
  27: {message:"OracleStale"},
  /**
   * `resolve_fx_shortfall` called a second time — one-time and
   * immutable, same reasoning as `set_allocation`: re-resolving later
   * against a different rate would let either side game the number
   * by choosing when to call it.
   */
  28: {message:"FxAlreadyResolved"},
  /**
   * `fund_fx_shortfall`/`expire_fx_shortfall_window`/`settle` called
   * before `resolve_fx_shortfall` has run for an oracle-configured
   * commitment.
   */
  29: {message:"FxNotResolved"},
  /**
   * `settle` called while a resolved FX shortfall is still unfunded.
   */
  30: {message:"FxShortfallUnfunded"},
  /**
   * `fund_fx_shortfall`/`expire_fx_shortfall_window` called but
   * `resolve_fx_shortfall` found no shortfall at all (the escrowed
   * amount already covered the fresh-rate-converted obligation) —
   * there's nothing to fund or expire.
   */
  31: {message:"NoFxShortfall"},
  /**
   * `fund_fx_shortfall` called a second time.
   */
  32: {message:"FxShortfallAlreadyFunded"},
  /**
   * `fund_fx_shortfall` called after `fx_shortfall_deadline` has
   * *strictly* passed — too late to cure, though the deadline second
   * itself is still in time. `expire_fx_shortfall_window` is the only
   * path left from here.
   */
  33: {message:"FxShortfallWindowPassed"},
  /**
   * `expire_fx_shortfall_window` called at or before
   * `fx_shortfall_deadline` — the deadline second itself still belongs
   * to the buyer's funding window, matching `fund_fx_shortfall`'s own
   * inclusive cut-off (`now > deadline`).
   */
  34: {message:"FxShortfallWindowNotPassed"},
  /**
   * `flag_dispute`'s `flagger` argument wasn't the buyer, cooperative,
   * or warehouse operator — only a named party to the deal can flag
   * one, not an arbitrary third address.
   */
  35: {message:"NotAParty"},
  /**
   * `expire_dispute_window` called at or before `dispute_deadline` —
   * the deadline second itself is still inside the freeze. Only that
   * timeout path can produce this: `resolve_dispute` has no deadline
   * gate at all. (Either call made while `status` isn't `Disputed` is
   * covered by the same `InvalidState` every other status-gated call
   * already uses.)
   */
  36: {message:"DisputeWindowNotPassed"},
  /**
   * `initialize`'s `grade_price_bps` held more than
   * `MAX_GRADE_ENTRIES` entries — longer than any real grading
   * schedule needs, and unbounded work the contract shouldn't accept.
   */
  37: {message:"GradeScheduleTooLong"},
  /**
   * `set_allocation`'s `members` held more than
   * `MAX_ALLOCATION_MEMBERS` entries — beyond the PRD's specified
   * cooperative scale, and unbounded storage the contract shouldn't
   * accept.
   */
  38: {message:"AllocationTooLarge"},
  /**
   * `initialize`'s `total_amount` exceeded `MAX_TOTAL_AMOUNT`
   * (`i128::MAX / 10_000`) — a commitment whose `bps_amount` arithmetic
   * would overflow, and therefore one that could never be locked or
   * settled. Refused at the call that supplied the value (#249).
   */
  39: {message:"AmountTooLarge"},
  /**
   * The configured Reflector oracle returned a zero or negative price;
   * non-positive exchange rates cannot produce a valid settlement amount.
   */
  40: {message:"OraclePriceInvalid"},
  /**
   * `expire_delivery_window` called before `delivery_deadline +
   * remainder_window_secs` has strictly passed. The permissionless
   * fallback is intentionally delayed past the buyer's own reclaim
   * window to give the buyer time to act first.
   */
  41: {message:"DeliveryGraceWindowNotPassed"},
  /**
   * `flag_dispute` called after the per-commitment `MAX_DISPUTE_FLAGS`
   * cap has been reached. Only `expire_dispute_window` lapsals count
   * against the cap; `resolve_dispute` (unanimous consent) does not.
   */
  42: {message:"DisputeCapReached"},
  /**
   * `confirm_delivery` called after `delivery_deadline` has *strictly*
   * passed (`now > delivery_deadline`). The deadline second itself still
   * belongs to the operator's confirmation window; strictly after it, only
   * `reclaim_on_nondelivery` is legal (#199).
   */
  43: {message:"DeliveryDeadlinePassed"},
  /**
   * The oracle conversion's numerator or decimal divisor cannot be
   * represented as an `i128`; settlement must not trap on unusable oracle
   * values.
   */
  44: {message:"OracleConversionOutOfRange"},
  /**
   * The configured Reflector oracle returned a quote timestamp later than
   * the current ledger timestamp; future quotes cannot satisfy freshness.
   */
  45: {message:"OracleTimestampInFuture"},
  /**
   * The configured oracle contract could not answer at all (#245): the
   * cross-contract `lastprice` or `decimals` call failed in a way the
   * host lets the caller recover from. That covers an
   * `oracle_config.oracle_contract` with no contract deployed behind it,
   * a callee that panics or traps, and a callee whose reply doesn't
   * decode as the expected type. Distinct from `OraclePriceUnavailable`,
   * which means the oracle *did* answer and quotes no such asset.
   * 
   * Not every failure can become this error: the host still aborts the
   * whole invocation, with no contract error to return, when the
   * callee uses up the transaction's shared CPU/memory budget. See the
   * header of `tests/oracle_fault_injection.rs` for which failures are
   * recoverable and which still abort.
   */
  46: {message:"OracleUnavailable"},
  /**
   * `lock` called after `delivery_deadline` has *strictly* passed
   * (`now > delivery_deadline`) — the deadline second itself is still in
   * time, matching every other deadline here. The delivery window is the
   * cooperative's time to perform and it starts at `initialize` (see
   * `Commitment::delivery_deadline`), so a lock that lands after it would
   * hand the buyer an immediate `reclaim_on_nondelivery` — a `Forfeited`
   * strike against a cooperative that never had a window to deliver in
   * (#277).
   */
  47: {message:"LockAfterDeliveryDeadline"},
  /**
   * `extend_delivery_deadline` was asked to move the deadline backwards
   * or leave it unchanged.
   */
  48: {message:"DeliveryDeadlineNotExtended"}
}


/**
 * One state-changing call that the current commitment permits.
 */
export interface Action {
  auth: AuthParty;
  deadline: ActionDeadline;
  /**
 * The public function to invoke, as a Soroban symbol.
 */
function: string;
}

export type Status = {tag: "Draft", values: void} | {tag: "Locked", values: void} | {tag: "Advance1Released", values: void} | {tag: "CheckpointPassed", values: void} | {tag: "Advance2Released", values: void} | {tag: "ReadyForDelivery", values: void} | {tag: "Delivered", values: void} | {tag: "Settled", values: void} | {tag: "Cancelled", values: void} | {tag: "Defaulted", values: void} | {tag: "Forfeited", values: void} | {tag: "Disputed", values: void};


/**
 * Whose authorization a callable action needs, as advertised by
 * [`Self::available_actions`].
 */
export type AuthParty = {tag: "Anyone", values: void} | {tag: "Buyer", values: void} | {tag: "Cooperative", values: void} | {tag: "WarehouseOperator", values: void} | {tag: "BuyerAndCooperative", values: void} | {tag: "AllParties", values: void} | {tag: "AnyNamedParty", values: void} | {tag: "BuyerCooperativeAndNewBuyer", values: void} | {tag: "BuyerCooperativeAndNewOperator", values: void};


/**
 * On-chain state for a single escrow commitment instance.
 * 
 * **Footprint Audit & Storage Necessity Finding**:
 * A comprehensive review of every field in this struct confirmed that all 33
 * fields are genuinely necessary in on-chain instance storage and cannot be
 * moved off-chain or derived from events:
 * 1. **Auth & Token Routing** (`buyer`, `cooperative`, `warehouse_operator`, `token`):
 * Enforces `require_auth()` and routes token transfers across every lifecycle call.
 * 2. **Financial Terms & Schedules** (`total_amount`, `advance1_bps`, `advance2_bps`,
 * `claim_window_secs`, `remainder_window_secs`, `delivery_deadline`, `contracted_quantity`,
 * `grade_price_bps`):
 * Smart contracts cannot query historical event logs; every guard, deadline check,
 * and settlement payout equation requires these baseline terms on-chain.
 * 3. **State & Lifecycle Flags** (`status`, `created_at`, `advance1_deadline`,
 * `advance1_claimed`, `advance1_expired`, `advance2_deadline`, `advance2_claimed`,
 * `advance2_expired`, `remainder_deadline`, `remainder_funde
 */
export interface Commitment {
  /**
 * Basis points (of `total_amount`) released at lock-in. 0-10_000.
 */
advance1_bps: u32;
  advance1_claimed: boolean;
  /**
 * 0 = not yet opened. Once `release_advance_1` runs, this is set to
 * the ledger timestamp after which the cooperative can no longer
 * claim and the buyer becomes eligible to reclaim.
 */
advance1_deadline: u64;
  advance1_expired: boolean;
  /**
 * Basis points released at the mid-season checkpoint. 0-10_000.
 * `advance1_bps + advance2_bps` must not exceed 10_000 — enforced at
 * `initialize`. Together these two define the *deposit* — the
 * portion `lock` actually escrows; see module docs.
 */
advance2_bps: u32;
  advance2_claimed: boolean;
  advance2_deadline: u64;
  advance2_expired: boolean;
  buyer: string;
  /**
 * How long the cooperative has to claim an advance once it opens,
 * in seconds. Same window for both tranches — PRD doesn't call for
 * different windows per tranche, so one shared value is the simpler
 * choice until a reason to split them shows up.
 * 
 * Boundary convention (#111, pinned by `tests/deadline_boundary.rs`):
 * the deadline is `release time + claim_window_secs`, and *claiming* is
 * allowed up to **and including** that instant (`timestamp > deadline`
 * is the rejection guard) while *reclaiming* is allowed only **strictly
 * after** it (`timestamp <= deadline` is the rejection guard). The two
 * guards are exact complements, so every ledger timestamp belongs to
 * exactly one party. Consequence worth knowing: the cooperative's
 * effective window spans `claim_window_secs + 1` distinct timestamps
 * (the opening one and the deadline one), not `claim_window_secs`.
 */
claim_window_secs: u64;
  /**
 * The quantity (caller-defined unit, e.g. kg) the buyer is
 * contracting for. Set at `initialize`, never changes. The
 * denominator for `confirm_delivery`'s proportional shortfall
 * adjustment (PRD §7).
 */
contracted_quantity: u32;
  cooperative: string;
  created_at: u64;
  /**
 * Set by `confirm_delivery`. 0 until then (also a legitimate real
 * value — total crop failure — but nothing reads this field before
 * `Status::Delivered`, so no separate sentinel is needed).
 */
delivered_quantity: u32;
  /**
 * Absolute deadline (`created_at + delivery_window_secs`), computed
 * once at `initialize` — unlike the tranche/remainder deadlines,
 * this one doesn't depend on some other call happening first, so
 * there's no "0 = unset" state for it.
 * 
 * **The delivery window starts at `initialize`, not at `lock`**: it is
 * the cooperative's time to reach `Delivered`, anchored to the moment
 * the deal was created (`created_at`), and it is therefore already
 * ticking while the commitment sits in `Draft`. That is deliberate —
 * the window is the buyer's protection against a cooperative that
 * never delivers, and re-anchoring it at `lock` would let a buyer
 * extend it by simply locking late. The complementary guard is on the
 * other side of that trade-off (#277): `lock` refuses once
 * `now > delivery_deadline` with
 * [`Error::LockAfterDeliveryDeadline`], so a buyer can never lock into
 * a window that has already closed and immediately forfeit the
 * cooperative's position via `reclaim_on_nondelivery`.
 */
delivery_deadline: u64;
  /**
 * Number of lapsed dispute windows accumulated so far. `flag_dispute`
 * refuses once this reaches `MAX_DISPUTE_FLAGS`. Only
 * `expire_dispute_window` lapsals increment it; `resolve_dispute`
 * (unanimous consent) does not — good-faith resolution should not
 * shorten the window for future disputes.
 */
dispute_count: u32;
  /**
 * 0 whenever `status` isn't `Disputed`. Reuses `remainder_window_secs`
 * as the window length, same reasoning as `fx_shortfall_deadline`
 * above — see `flag_dispute`'s doc comment for why a dispute freeze
 * needs a bound at all.
 */
dispute_deadline: u64;
  /**
 * Set by `flag_dispute`, the status to restore on `resolve_dispute`
 * or `expire_dispute_window`. `Status::Draft` whenever `status`
 * isn't `Disputed` — a safe "unset" sentinel, not a real possible
 * value, since `flag_dispute` never accepts `Draft` as a status to
 * dispute *from*. (Not `Option<Status>`: the `contracttype` macro's
 * generated `ScVal` conversion doesn't satisfy the blanket `Option`
 * impl for a unit-variant-only enum like `Status`, unlike a struct
 * such as `OracleConfig` — confirmed by trying it first.)
 */
dispute_pre_status: Status;
  /**
 * `oracle_config.denominated_amount`, adjusted by `settlement_bps`
 * and converted to the settlement token at the fresh rate
 * `resolve_fx_shortfall` read. 0 until resolved (also what it'd be
 * for a total write-off, which is why `fx_resolved` is the actual
 * "has this run" signal, not this field being nonzero).
 */
fx_adjusted_total: i128;
  /**
 * Set by `refresh_fx_quote_status` (#1): the most recent oracle-freshness
 * probe found a quote older than `oracle_config.max_age_secs`, i.e.
 * `resolve_fx_shortfall` is currently blocked on Reflector publishing a
 * fresh one. Cleared by the next probe that finds a fresh quote (and by
 * a successful `resolve_fx_shortfall`, so it can never linger once the
 * one-time resolution has run). **Informational only** — no guard reads
 * it; see `refresh_fx_quote_status`'s doc comment for why this is a
 * flag alongside `fx_resolved` rather than a distinct `Status`.
 */
fx_quote_stale: boolean;
  /**
 * Set by `resolve_fx_shortfall`, only meaningful when `oracle_config`
 * is `Some`. `settle` uses this instead of re-deriving from
 * `total_amount` whenever an oracle is configured — see
 * `resolve_fx_shortfall`'s doc comment.
 */
fx_resolved: boolean;
  /**
 * How much more the buyer owes beyond what's already escrowed, per
 * `resolve_fx_shortfall`'s comparison. 0 means no top-up needed —
 * `settle` can run without waiting on `fund_fx_shortfall` at all.
 */
fx_shortfall_amount: i128;
  /**
 * 0 if `fx_shortfall_amount` is 0 (no deadline needed). Reuses
 * `remainder_window_secs` as the window length rather than adding
 * yet another `initialize` parameter for a second deadline serving
 * the same "give the buyer a fair chance to act" purpose.
 */
fx_shortfall_deadline: u64;
  fx_shortfall_funded: boolean;
  /**
 * Set by `confirm_delivery` — which `grade_price_bps` entry the
 * warehouse operator attested.
 */
grade_index: u32;
  /**
 * Pre-agreed grade -> price-multiplier table, in basis points of
 * `total_amount`'s implied unit price (10_000 = full price), set at
 * `initialize` and never changed — "pre-agreed," per the PRD, means
 * agreed before delivery, not decided at settlement time. Ordered by
 * the caller; `confirm_delivery`'s `grade_index` selects an entry.
 * Each entry must be <= 10_000 (a grade can't be worth more than the
 * full agreed unit price) and there must be at least one entry.
 */
grade_price_bps: Array<u32>;
  /**
 * Set by `ready_for_delivery` when the remainder is non-zero and by
 * `fund_remainder` once the buyer's second deposit lands. **Also set by
 * `ready_for_delivery` itself when the remainder is zero** (#275): a
 * deposit covering all of `total_amount` (advance bps summing to 10 000)
 * leaves nothing to fund, so the window opens already satisfied and the
 * buyer can't be defaulted for skipping a call that moves no tokens.
 */
remainder_deadline: u64;
  remainder_funded: boolean;
  /**
 * How long the buyer has to fund the remainder once
 * `ready_for_delivery` opens the window. Independent of
 * `claim_window_secs` — this is a payment deadline, not a claim one.
 */
remainder_window_secs: u64;
  /**
 * Set by `confirm_delivery`: the combined quantity x grade
 * multiplier, in basis points of `total_amount`, that `settle` pays
 * out against. `min(delivered_quantity, contracted_quantity) /
 * contracted_quantity * grade_price_bps[grade_index]` — capped at
 * 10_000 by construction (over-delivery isn't paid extra in v1; see
 * `confirm_delivery`'s doc comment).
 */
settlement_bps: u32;
  status: Status;
  token: string;
  total_amount: i128;
  warehouse_operator: string;
}


/**
 * A live rate read from the configured Reflector oracle, returned by
 * `oracle_rate`. Mirrors Reflector's own `PriceData` shape (see
 * `reflector.rs`) rather than reusing that type directly, since this is
 * this contract's own public interface, not a re-export of Reflector's.
 */
export interface OracleRate {
  /**
 * `price / 10^decimals` is the actual rate, where `decimals` comes
 * from the oracle contract's own `decimals()` (14, for Reflector's
 * fiat exchange rate oracle as of this writing) — not duplicated
 * into `OracleConfig`, since it's a property of the oracle, not of
 * this commitment, and callers can read it directly off the oracle
 * contract if they need it.
 */
price: i128;
  /**
 * The ledger timestamp the oracle recorded this quote at — already
 * checked against `max_age_secs` by the time this is returned, but
 * surfaced anyway so a caller can display "as of" freshness.
 */
timestamp: u64;
}



/**
 * One advance tranche's claim state. `deadline == 0` means not opened yet.
 */
export interface TrancheView {
  claimed: boolean;
  deadline: u64;
  expired: boolean;
}


/**
 * PRD §4.2 option (b)'s FX-resolution state. All fields are zero/false for a
 * commitment initialized without an `oracle_config`; `configured` tells the
 * two apart without a second storage read.
 */
export interface CommitmentFx {
  adjusted_total: i128;
  configured: boolean;
  quote_stale: boolean;
  resolved: boolean;
  shortfall_amount: i128;
  shortfall_deadline: u64;
  shortfall_funded: boolean;
}


/**
 * Which Reflector oracle instance and asset symbol `oracle_rate` and
 * `resolve_fx_shortfall` read from, how old a quote they'll accept, and
 * what the deal is actually worth in that currency. Set once, at
 * `initialize`.
 */
export interface OracleConfig {
  /**
 * The deal's true value in `price_asset`, in the same "smallest
 * unit, 7 decimal places" convention `total_amount` already uses
 * for the settlement token (so e.g. 1 NGN == 10_000_000 units here,
 * matching how 1 XLM == 10_000_000 stroops) — **not** the same
 * number as `total_amount`, which is `denominated_amount`'s value
 * *at whatever rate happened to hold when the deal was funded*.
 * `resolve_fx_shortfall` reprices this at a fresh rate; the gap
 * between that and what's actually escrowed is PRD §4.2's FX risk,
 * make concrete. This field always uses the 7-decimal convention;
 * `resolve_fx_shortfall` converts the result to the settlement token's
 * own `decimals()` (which is not necessarily 7 — see #258) before
 * comparing it with the escrowed balance. See that function's doc
 * comment for the worked example.
 * 
 * **Not** the oracle's `decimals()` — that is a property of the
 * oracle, read live at settlement and assumed immutable (see
 * `docs/reflector-decimals-assumption.md`, issue #63); this field is
 * the deal's value in the same 7
 */
denominated_amount: i128;
  /**
 * Maximum age, in seconds, a quote may have and still be accepted —
 * PRD §16.3's oracle staleness bound. Must be > 0.
 */
max_age_secs: u64;
  /**
 * The Reflector oracle contract to call — e.g. the testnet
 * "Fiat exchange rates" instance, `CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W`
 * (see `reflector.rs`).
 */
oracle_contract: string;
  /**
 * The asset symbol to quote against that oracle's `base()` — e.g.
 * `NGN`. Always queried as `ReflectorAsset::Other`, since this
 * contract's use case is fiat/FX symbols, not Stellar-native assets.
 */
price_asset: string;
}



/**
 * The timestamp, if any, that bounds a callable action. The variant says
 * which side of the boundary the action lives on, so a client can tell
 * "claim closes at T" from "expiry opens just after T" without knowing
 * each function's guard by heart.
 */
export type ActionDeadline = {tag: "None", values: void} | {tag: "Inclusive", values: readonly [u64]} | {tag: "Exclusive", values: readonly [u64]};


/**
 * A stable, versioned read model for a commitment (#256).
 * 
 * [`Commitment`] is the storage struct: every field exists because some guard
 * or payout needs it on chain, and its shape is free to change for storage
 * reasons. Off-chain clients read this view instead, so a storage-layout
 * change does not rewrite their ABI.
 * 
 * **Stability promise.** Fields are only added, or their meaning changed, in
 * a release that increments [`COMMITMENT_VIEW_SCHEMA_VERSION`]; readers
 * should refuse a `schema_version` newer than they understand and may assume
 * that a `schema_version` they do understand keeps the same field meanings.
 * The storage struct can change without a version bump — only this view's
 * shape is contractual.
 * 
 * The grouping is semantic, not storage order: `parties` (who), `terms`
 * (agreed before funding), `tranches` (advance claim state), `funding`
 * (two-phase payment), `delivery` (the operator's attestation), `fx` (oracle
 * conversion), and `dispute` (the bounded freeze). `status` and `created_at`
 * stay top-level because they des
 */
export interface CommitmentView {
  created_at: u64;
  delivery: CommitmentDelivery;
  dispute: CommitmentDispute;
  funding: CommitmentFunding;
  fx: CommitmentFx;
  parties: CommitmentParties;
  schema_version: u32;
  status: Status;
  terms: CommitmentTerms;
  tranches: CommitmentTranches;
}

/**
 * Whether a dispute ended by unanimous consent or by the window expiring (#208).
 */
export type DisputeOutcome = {tag: "Resolved", values: void} | {tag: "Expired", values: void};




/**
 * The deal's agreed terms and windows, fixed at `initialize`.
 * 
 * **The one definition of "the terms" (#270).** `initialize` takes this
 * struct instead of a run of eight positional arguments, and
 * `get_commitment_view` returns the same struct as `terms` — so the
 * generated bindings, the read model, and `docs/0002-interface-spec.md`
 * all name the same fields. That is the whole point: `advance1_bps` and
 * `advance2_bps` are adjacent `u32`s, and `claim_window_secs`,
 * `remainder_window_secs` and `delivery_window_secs` are adjacent `u64`s,
 * so a positional swap type-checked and silently produced a different
 * deal. A named field cannot be swapped silently, and generated clients
 * carry the names through.
 * 
 * `delivery_window_secs` is the *window length*, not the absolute
 * deadline: `initialize` computes `delivery_deadline = created_at +
 * delivery_window_secs` once and stores that absolute value in
 * [`Commitment`]. The view reconstructs the window as
 * `delivery_deadline - created_at`, which is exactly the value passed
 * here, so the read 
 */
export interface CommitmentTerms {
  /**
 * Basis points of `total_amount` released at lock-in. 0-10_000, and
 * `advance1_bps + advance2_bps <= 10_000`.
 */
advance1_bps: u32;
  /**
 * Basis points released at the mid-season checkpoint. Together with
 * `advance1_bps` this defines the deposit `lock` escrows.
 */
advance2_bps: u32;
  /**
 * How long the cooperative has to claim an advance once it opens.
 */
claim_window_secs: u64;
  /**
 * The quantity the buyer is contracting for, in caller-defined units.
 */
contracted_quantity: u32;
  /**
 * How long after `created_at` the cooperative has to reach
 * `Delivered`. Must be > 0.
 */
delivery_window_secs: u64;
  /**
 * Pre-agreed grade -> price multiplier table, in basis points
 * (10_000 = full price). Non-empty, at most `MAX_GRADE_ENTRIES`
 * entries, every entry <= 10_000.
 */
grade_price_bps: Array<u32>;
  /**
 * How long the buyer has to fund the remainder once
 * `ready_for_delivery` opens the window.
 */
remainder_window_secs: u64;
  total_amount: i128;
}




/**
 * One farmer member's recorded share of a commitment, captured by
 * `set_allocation`. `member_hash` is a per-member salted hash — never a
 * bare phone number or other PII — computed off-chain (API) as
 * `HMAC-SHA256(salt, phone_number)` with a fresh random salt per
 * member. The salt and the phone-number mapping live only in the API's
 * Postgres, which is what makes this genuinely erasable per NDPA s.34:
 * delete the off-chain row and this hash becomes permanently
 * unlinkable to a real person, since the salt is gone and brute-
 * forcing a random salt is infeasible regardless of how small the
 * phone-number keyspace is.
 */
export interface AllocationMember {
  member_hash: Buffer;
  /**
 * Basis points of the commitment's total payout this member is
 * entitled to, off-chain. Record-only in v1 — see `set_allocation`'s
 * doc comment for why `settle` doesn't pro-rate against this.
 */
share_bps: u32;
}



/**
 * Dispute freeze state; `pre_status` is `Draft` when not disputed.
 */
export interface CommitmentDispute {
  deadline: u64;
  pre_status: Status;
}


/**
 * Two-phase funding state: the deposit is escrowed by `lock`, the remainder
 * by `fund_remainder` once `ready_for_delivery` opens its window.
 */
export interface CommitmentFunding {
  remainder_deadline: u64;
  remainder_funded: boolean;
}


/**
 * The three named parties plus the settlement token.
 */
export interface CommitmentParties {
  buyer: string;
  cooperative: string;
  token: string;
  warehouse_operator: string;
}




/**
 * The warehouse operator's delivery attestation, consumed by `settle`.
 */
export interface CommitmentDelivery {
  delivered_quantity: u32;
  grade_index: u32;
  settlement_bps: u32;
}


export interface CommitmentTranches {
  advance1: TrancheView;
  advance2: TrancheView;
}




export interface Client {
  /**
   * Construct and simulate a lock transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Draft -> Locked. Pulls the buyer's **deposit** into the contract —
   * `advance1_bps + advance2_bps` of `total_amount`, not the full
   * amount. See module docs for why this changed from earlier versions
   * of this contract, which escrowed everything here.
   * 
   * Requires buyer auth.
   * 
   * **Refused once `delivery_deadline` has strictly passed (#277)**: the
   * delivery window counts from `initialize`, not from this call (see
   * `Commitment::delivery_deadline` for why), so locking after it would
   * buy the buyer a same-second `reclaim_on_nondelivery` and a free
   * `Forfeited` against a cooperative that never had time to deliver.
   * The deadline second itself is still in time — `now > delivery_deadline`
   * is the only rejection (`RemainderWindowPassed`/`ClaimWindowPassed`
   * use the same inclusive cut-off), so
   * `lock_exactly_at_the_delivery_deadline_still_succeeds` pins the
   * boundary from both sides. Rejecting here is the guard half of the
   * fix; the alternative considered and not taken was re-anchoring
   * `delivery_deadline` to `max(delivery_deadline, loc
   */
  lock: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a cancel transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Mutual cancellation — PRD §7: "Defined unwind: advance settled per
   * agreed schedule, remaining escrow returned, no penalty, logged."
   * Allowed from any pre-delivery state (`Draft` through
   * `ReadyForDelivery`). Not from `Delivered` onward — at that point
   * `settle` is the correct path, an unwind doesn't apply anymore.
   * 
   * Requires **both** the buyer's and the cooperative's auth in the same
   * call, since this is mutual, not unilateral — unlike `reclaim_*`,
   * which is the buyer's unilateral right once a claim window lapses on
   * its own.
   * 
   * "Advance settled per agreed schedule, no penalty": whatever's
   * already been claimed stays with the cooperative — this doesn't claw
   * anything back. "Remaining escrow returned": whatever's still in the
   * contract (zero, if nothing was ever locked) goes back to the buyer,
   * via the same balance-based transfer `settle` uses, so it's correct
   * regardless of claim/reclaim history or how much of the deposit vs.
   * remainder had been funded. "Logged": the state transition and the
   * transfer both land on t
   */
  cancel: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a settle transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Delivered -> Settled. Splits the contract's remaining token balance
   * (the funded remainder — advances already left the contract via
   * `claim_tranche`/`reclaim_tranche` by this point) between the
   * cooperative and a shortfall refund to the buyer, per
   * `confirm_delivery`'s `settlement_bps`.
   * 
   * "Advance not clawed back" (PRD §7, partial delivery): whatever the
   * cooperative already claimed via `claim_advance_1`/`claim_advance_2`
   * stays theirs regardless of the final `settlement_bps` — this only
   * adjusts the *remainder* payment. `adjusted_total = total_amount *
   * settlement_bps / 10_000` is the full contract value the delivery
   * actually earned; subtracting what the cooperative already claimed
   * gives what's still owed to them from the remainder (floored at
   * zero, since claimed advances are never clawed back even if a severe
   * shortfall means they technically over-earned already). Whatever of
   * the remainder balance isn't owed goes back to the buyer as a
   * shortfall refund.
   * 
   * **Requires both advance tranches already resolved** (each 
   */
  settle: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_status transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * No auth required — read-only view, callable by anyone.
   */
  get_status: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Status>>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Creates the commitment in `Draft`. No funds move yet.
   * 
   * Requires the buyer's auth because they're the one committing to a
   * future deposit — the cooperative and warehouse operator don't need
   * to co-sign contract creation itself, only the state transitions
   * that matter to them (`mark_checkpoint`, `confirm_delivery`).
   * 
   * The three party addresses are *not* required to be distinct, on
   * purpose. Role separation is enforced where it matters — at each
   * state transition, by that role's own `require_auth()` — rather than
   * by a distinctness check here. Collapsing buyer, cooperative and
   * warehouse onto one principal is a legitimate deployment choice (a
   * cooperative that also runs the warehouse, or a minimal single-signer
   * validation deployment); `initialize` records the parties it is handed
   * and does not second-guess them. The consequence is accepted, not
   * hidden: with one principal in all three roles every multi-party
   * consent gate reduces to that single signer. See the tests
   * `initialize_allows_buyer_cooperative_and_warehouse_t
   */
  initialize: ({buyer, cooperative, warehouse_operator, token, terms, oracle_config}: {buyer: string, cooperative: string, warehouse_operator: string, token: string, terms: CommitmentTerms, oracle_config: Option<OracleConfig>}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a oracle_rate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Reads the current rate for this commitment's configured oracle
   * asset, enforcing the staleness bound. A pure read against
   * Reflector's own stored history, not a state transition — callable
   * in any status, including after `Settled`.
   * 
   * No auth required — permissionless read.
   */
  oracle_rate: (options?: MethodOptions) => Promise<AssembledTransaction<Result<OracleRate>>>

  /**
   * Construct and simulate a flag_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Dispute flagging (PRD's must-have "dispute flagging with defined
   * escalation"). Any **one** of the three named parties — `flagger`
   * must equal `buyer`, `cooperative`, or `warehouse_operator` — can
   * freeze the commitment by moving it to `Status::Disputed`, which
   * blocks every other state-changing call by construction: every one
   * of them matches a specific required status (or set of statuses)
   * and `Disputed` is never among them.
   * 
   * This contract still doesn't arbitrate *what* the dispute is about
   * — see the module doc's explanation of why that's a deliberate
   * non-goal, not an oversight. What this buys is the "flagging" half
   * only: a unilateral pause, so a contested situation doesn't let
   * some other deadline-triggered function (e.g. `settle`,
   * `expire_remainder_window`) run to a conclusion while it's being
   * sorted out off-chain. Reachable from any state with funds already
   * at stake or a claim outstanding (`Locked` through `Delivered`) —
   * not `Draft` (nothing escrowed yet to freeze) and not any terminal
   * status or `Disput
   */
  flag_dispute: ({flagger}: {flagger: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a fund_remainder transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Buyer escrows `total_amount - deposit` — the second half of
   * two-phase funding. Must happen within the window
   * `ready_for_delivery` opened, or it's a default (see
   * `expire_remainder_window`). Requires buyer auth — this is new
   * money the buyer specifically owes.
   * 
   * The window's end is **inclusive of `remainder_deadline` itself**:
   * `now > remainder_deadline` is the only rejection, so a payment
   * landing in the deadline second still counts.
   * `expire_remainder_window` needs strictly more than the deadline,
   * which is what keeps the boundary from being ambiguous — the two
   * calls are never both legal at the same timestamp.
   * 
   * **A zero remainder is already funded (#275)**: when
   * `advance1_bps + advance2_bps == 10_000`, `lock` has escrowed the
   * whole `total_amount`, `ready_for_delivery` marked the remainder
   * funded on the buyer's behalf, and this call refuses with
   * `RemainderAlreadyFunded` instead of transferring zero tokens. The
   * deal can go straight to `confirm_delivery` — no no-op call is
   * required, and none can be held agains
   */
  fund_remainder: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The recorded allocation ledger, if `set_allocation` has run.
   * 
   * No auth required — read-only view, callable by anyone.
   */
  get_allocation: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<AllocationMember>>>>

  /**
   * Construct and simulate a get_commitment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * No auth required — read-only view, returns the full commitment state.
   * 
   * Kept for backward compatibility: existing clients read this storage
   * struct directly, and it remains the canonical on-chain record. New
   * clients should prefer [`Self::get_commitment_view`], whose shape is
   * versioned independently of the storage layout.
   */
  get_commitment: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Commitment>>>

  /**
   * Construct and simulate a reassign_buyer transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Buyer-position assignability (PRD §4.8): transfers `buyer` to
   * `new_buyer`. Deliberately **not** a market — there's no order book,
   * no listing, no on-chain price discovery here, just a novation of
   * who holds the position. No funds move; this only ever rewrites who
   * `buyer` refers to for every future `reclaim_*`/`cancel`/
   * `fund_remainder` auth check.
   * 
   * Requires **three** signatures in the same call, not two: the
   * current buyer's (they're giving up the position), the
   * cooperative's (PRD's explicit "with cooperative consent"), and the
   * new buyer's. That third one isn't named in the PRD line this
   * implements, but leaving it out would let the current buyer and
   * cooperative saddle a third party with a position — including its
   * obligations — without that party ever agreeing to take it on. Two
   * consents where the PRD asked for two would be the smaller change;
   * three is the safer one, and safety wins here.
   * 
   * Reachable from the same states as `cancel` (`Draft` through
   * `ReadyForDelivery`) and for the same reason: past `Deliver
   */
  reassign_buyer: ({new_buyer}: {new_buyer: string}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a set_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cooperative-authored record of member farmers' entitlement shares
   * — PRD's "member allocation ledger capture at lock-in." **Record-
   * only in v1**: `settle` still pays the cooperative wallet a lump
   * sum; this doesn't pro-rate an on-chain payout across members. That
   * matches PRD §4.9's own stated v1 default (Transparency Ladder
   * Rung 1 — "Payment settles to the cooperative wallet. Each
   * member's entitlement is on chain and readable; members receive an
   * SMS stating their share") — not an open question this contract
   * deferred, but the documented default. Pro-rated on-chain payout
   * (Rung 2+) would need real design work (N token transfers instead
   * of one, rounding-remainder handling) and isn't built.
   * 
   * One-time and immutable once set — no amend function exists.
   * `members` must be non-empty, may hold at most
   * `MAX_ALLOCATION_MEMBERS` entries, and the shares must sum to at
   * most 10_000 bps.
   * Deliberately **not** required before `lock`: a solo-farmer
   * commitment with no cooperative pooling shouldn't be forced through
   * a ledge
   */
  set_allocation: ({members}: {members: Array<AllocationMember>}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a claim_advance_1 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cooperative claims tranche 1, if within the window. Requires
   * cooperative auth.
   * 
   * Boundary (#111): "within" is inclusive of the deadline instant —
   * `claim_tranche` rejects only when `timestamp > advance1_deadline`, so
   * `advance1_deadline` itself is still claimable, and it is the last
   * instant that is. Pinned by `tests/deadline_boundary.rs`.
   */
  claim_advance_1: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a claim_advance_2 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cooperative claims tranche 2, if within the window. Requires
   * cooperative auth — same mechanics as `claim_advance_1`, including its
   * inclusive `advance2_deadline` boundary (#111).
   */
  claim_advance_2: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a mark_checkpoint transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Advance1Released -> CheckpointPassed. Requires the warehouse
   * operator's attestation — this is a judgment call, not a mechanical
   * state advance, so unlike opening an advance tranche it *is*
   * auth-gated. Independent of whether tranche 1 has actually been
   * claimed yet — crop progress doesn't wait on paperwork.
   */
  mark_checkpoint: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a resolve_dispute transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Resolves a dispute by unanimous consent — **all three** named
   * parties' auth, not a majority or any single one, since resuming
   * is the mirror of `flag_dispute`'s unilateral freeze and shouldn't
   * itself be unilateral. Restores `status` to exactly whatever it
   * was the moment `flag_dispute` ran — this contract doesn't decide,
   * or let the parties redirect it to, a *different* outcome (e.g.
   * straight to `Cancelled`) from inside this call; that would be
   * arbitration by another name. If the parties' off-chain resolution
   * is "unwind the deal," they call the existing `cancel()` next,
   * same as they would have without ever disputing.
   * 
   * **Deadline-free on purpose**: unlike `expire_dispute_window`, this
   * carries no `dispute_deadline` check at all, so it stays callable in
   * the deadline second itself and after it. Unanimous consent is the
   * only gate — the timeout exists to break a deadlock, never to take
   * the decision away from three parties who have already agreed.
   */
  resolve_dispute: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a confirm_delivery transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * ReadyForDelivery -> Delivered. Warehouse-operator-attested, same
   * reasoning as `mark_checkpoint`. Requires the remainder to already
   * be funded — delivery shouldn't be confirmable while the buyer
   * still owes money on the deal.
   * 
   * Applies the PRD §7 shortfall/grade adjustment schedule: `delivered_quantity`
   * is compared against `contracted_quantity` (proportional, capped at
   * 100% — over-delivery isn't paid extra in v1, matching "excess
   * otherwise released to cooperative" off-chain), `grade_index` selects
   * an entry from the pre-agreed `grade_price_bps` table, and the two
   * multiply together into `settlement_bps`, which `settle` pays out
   * against. The warehouse operator's attestation is authoritative here
   * — a grade or quantity dispute is the operator's own appeals
   * process, not a HarvestLock dispute (PRD's edge-case table).
   * 
   * **Storage access pattern**:
   * Reads the `Commitment` struct exactly once from instance storage upon
   * entry via `Self::load`. All validation checks, shortfall scaling, and
   * `settlement_bps` multiplier ca
   */
  confirm_delivery: ({delivered_quantity, grade_index}: {delivered_quantity: u32, grade_index: u32}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a available_actions transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * No auth required — read-only view, callable by anyone.
   * 
   * Returns every *state-changing* call the current commitment permits,
   * each with the auth it needs and the timestamp that bounds it (if
   * any). This is the contract's answer to the hand-written "who can
   * call what, when" table in `docs/griefing-and-inaction.md` §1: a
   * client can render exactly the legal buttons instead of rebuilding
   * the state machine.
   * 
   * Every entry is produced by the same private predicate the real
   * guard calls (`can_lock`, `can_claim_tranche`, `can_settle`, ...), so
   * the view cannot drift from the checks that actually authorize the
   * call. A predicate is `Ok` only when every state-derived guard would
   * pass; argument-specific validation (a valid `grade_index`, a
   * non-empty allocation list, a `new_buyer` address, a `flagger` that
   * is one of the named parties) remains at the call site because it
   * isn't knowable from the commitment.
   * 
   * Two deliberate exclusions: `initialize` (there is no commitment to
   * derive actions from before it runs — the view return
   */
  available_actions: (options?: MethodOptions) => Promise<AssembledTransaction<Array<Action>>>

  /**
   * Construct and simulate a fund_fx_shortfall transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Buyer pays in the shortfall `resolve_fx_shortfall` computed.
   * Buyer-gated, matching `fund_remainder`'s own reasoning: this is
   * new money the buyer specifically owes, not a mechanical step
   * anyone could trigger.
   * 
   * Deadline-**inclusive**, exactly like `fund_remainder`:
   * `now > fx_shortfall_deadline` is the only rejection, so the
   * deadline second itself is still the buyer's to pay in.
   */
  fund_fx_shortfall: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_oracle_config transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The configured oracle, if `initialize` set one.
   * 
   * No auth required — read-only view, callable by anyone.
   */
  get_oracle_config: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Option<OracleConfig>>>>

  /**
   * Construct and simulate a reclaim_advance_1 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Buyer reclaims tranche 1, if the window has passed unclaimed.
   * Requires buyer auth.
   * 
   * Boundary (#111): the exact complement of `claim_advance_1` —
   * `reclaim_tranche` rejects while `timestamp <= advance1_deadline`, so
   * the deadline instant belongs to the cooperative's claim and the buyer's
   * reclaim first becomes possible at `advance1_deadline + 1`. No timestamp
   * ever admits both, and none admits neither.
   */
  reclaim_advance_1: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a reclaim_advance_2 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Buyer reclaims tranche 2, if the window has passed unclaimed.
   * Requires buyer auth — same mechanics as `reclaim_advance_1`, including
   * the exclusive `advance2_deadline` boundary (#111).
   */
  reclaim_advance_2: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a release_advance_1 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Locked -> Advance1Released. Opens tranche 1's claim window — does
   * **not** move funds. The cooperative claims via `claim_advance_1`,
   * or the buyer reclaims via `reclaim_advance_1` after the window
   * passes unclaimed.
   * 
   * Deliberately not auth-gated: it starts a clock, it doesn't move
   * money or grant anyone anything they weren't already entitled to
   * under the agreed terms.
   * 
   * Security-reviewed on the record (#2): that reasoning holds under
   * adversarial timing — a third party can pick the instant the window
   * opens but gains nothing by it (no funds move, the deadline is
   * written exactly once so the clock can never be restarted or
   * extended, the lapse beneficiary stays the buyer, and the
   * cooperative always gets the full `claim_window_secs` from whenever
   * the window opened). The full reader audit and the pinned scenarios
   * live in `docs/permissionless-release-review.md` and
   * `contracts/escrow/tests/permissionless_release_review.rs`.
   * Auth-gating would add a timing veto the non-custodial design
   * deliberately gives nobody (PRD §4.5
   */
  release_advance_1: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a release_advance_2 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * CheckpointPassed -> Advance2Released. Same mechanics as
   * `release_advance_1`, for tranche 2. Permissionless like
   * `release_advance_1` — deliberately not auth-gated, it only starts
   * a clock. The #2 security review covers both calls together (they
   * share `open_tranche`); see `release_advance_1`'s doc comment and
   * `docs/permissionless-release-review.md`. Note that tranche 2's
   * clock additionally cannot be pulled early at all: reaching
   * `CheckpointPassed` requires the warehouse operator's auth-gated
   * `mark_checkpoint` first.
   */
  release_advance_2: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a ready_for_delivery transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Advance2Released -> ReadyForDelivery. The cooperative signaling
   * "setting out for delivery" — opens the remainder-payment window
   * (`remainder_window_secs` from now). Auth-gated, unlike
   * `release_advance_*`: this one has real stakes for the buyer (it
   * starts the clock that can end in `Defaulted`), so it shouldn't be
   * triggerable by just anyone.
   * 
   * **A zero remainder is funded here, not by a token transfer (#275).**
   * `lock` escrows `advance1_bps + advance2_bps` of `total_amount` and
   * `fund_remainder` escrows the rest; when an advance split sums to
   * exactly 10 000 bps the deposit *is* `total_amount`, so there is no
   * remainder to send. Requiring `fund_remainder` anyway would let a
   * fully-prepaid buyer be marked `Defaulted` — and lose the whole
   * deposit to the cooperative — by not making a call that moves zero
   * tokens before `remainder_deadline`. So when the remainder is zero,
   * this call sets `remainder_funded` itself, which keeps
   * `expire_remainder_window`'s buyer-default (`RemainderAlreadyFunded`)
   * and `confirm_delivery`'s
   */
  ready_for_delivery: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a get_commitment_view transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * No auth required — the versioned read model (#256).
   * 
   * Same state as `get_commitment`, reshaped into [`CommitmentView`] with
   * an explicit `schema_version` and semantic sub-structs, so a field added
   * to `Commitment` for storage reasons does not change the client-facing
   * ABI. `fx.configured` is read from the same instance-storage entry that
   * gates settlement's FX path, so a caller can distinguish "no oracle"
   * from "oracle resolved with zeros" without a second call.
   */
  get_commitment_view: (options?: MethodOptions) => Promise<AssembledTransaction<Result<CommitmentView>>>

  /**
   * Construct and simulate a resolve_fx_shortfall transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * PRD §4.2 option (b), the actual conversion step: reads a fresh
   * oracle rate, reprices `oracle_config.denominated_amount` (adjusted
   * by `settlement_bps`, same shortfall/grade math `settle` itself
   * uses) into the settlement token, and compares that against what's
   * actually escrowed. Whatever gap remains — the buyer's FX risk,
   * made concrete — becomes `fx_shortfall_amount` for `fund_fx_shortfall`
   * to collect.
   * 
   * Worked example, using this session's live-tested numbers: a deal
   * worth 1,000,000 NGN (`denominated_amount = 10_000_000_000_000` at
   * the 7-decimal convention), fully delivered at full grade
   * (`settlement_bps = 10_000`), Reflector quoting `price =
   * 66_700_000_000` at `decimals = 14` (≈1 NGN = 0.000667 USD, i.e.
   * ≈1,500 NGN/USD): `ngn_owed = 10_000_000_000_000` (unchanged, full
   * bps), `fx_adjusted_total = 10_000_000_000_000 * 66_700_000_000 /
   * 10^14 = 6_670_000_000` — 667 USDC, matching ₦1,000,000 at
   * ≈1,500 NGN/USD by hand. If `total_amount` had been funded
   * assuming a slightly better rate, escrow might hol
   */
  resolve_fx_shortfall: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a expire_dispute_window transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The dispute-side escape hatch, same shape as
   * `expire_remainder_window`/`expire_fx_shortfall_window`:
   * permissionless, deadline-gated. If the three parties can't reach
   * the unanimous consent `resolve_dispute` requires before
   * `dispute_deadline`, this does **not** guess who was at fault —
   * consistent with `flag_dispute`'s doc comment, this contract still
   * isn't arbitrating anything. It just restores the pre-dispute
   * status, exactly like `resolve_dispute` would, so the freeze can't
   * become permanent by one party simply refusing to ever consent.
   * Whatever normal deadline-triggered mechanism would otherwise have
   * applied (`expire_remainder_window`, `reclaim_on_nondelivery`,
   * etc.) is free to run again from there.
   * 
   * Deadline-**exclusive**: `now > dispute_deadline` is required, so in
   * the deadline second itself this still refuses with
   * `DisputeWindowNotPassed` while `resolve_dispute` remains available —
   * the boundary second is never a dead zone where neither resume path
   * works.
   */
  expire_dispute_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a replace_oracle_config transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Replaces a broken oracle configuration with a new one, with two-party
   * consent (issue #215). Addresses F1 (unreachable oracle) by letting the
   * buyer and cooperative substitute a working `oracle_config` before
   * `fx_resolved` is set.
   * 
   * Only legal in `Status::Delivered` while `fx_resolved` is false.
   * The new config is validated the same way `initialize` does
   * (`max_age_secs != 0`). Requires buyer and cooperative to co-sign.
   */
  replace_oracle_config: ({new_config}: {new_config: OracleConfig}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a expire_delivery_window transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Permissionless fallback that forfeits the escrowed funds to the buyer
   * when the cooperative never called `reclaim_on_nondelivery` and the
   * buyer's key is lost or inaccessible. Callable by anyone after
   * `delivery_deadline + remainder_window_secs` (the grace period).
   * Without this hatch, a lost buyer key would lock funds forever.
   */
  expire_delivery_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a reclaim_on_nondelivery transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The seller-non-delivery path: `delivery_deadline` has *strictly*
   * passed (`now > delivery_deadline`; the deadline second itself still
   * refuses with `DeliveryDeadlineNotPassed`) without `confirm_delivery`
   * ever having run. Returns whatever's currently escrowed to the buyer
   * and sets `Forfeited`.
   * 
   * Buyer-gated, not permissionless — unlike `expire_remainder_window`,
   * the beneficiary here is a specific party (the buyer) reclaiming
   * what's rightfully theirs, the same reasoning `reclaim_tranche` uses.
   * 
   * Not reachable from `Draft`: nothing's ever been escrowed at that
   * point, so there's nothing to reclaim and no real event has
   * occurred worth recording as a forfeiture. Since `lock` refuses once
   * `delivery_deadline` has passed (#277), there is also no way to reach
   * `Locked` with the delivery window already closed, so this sweep can
   * only ever fire against a window the cooperative actually had.
   */
  reclaim_on_nondelivery: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a expire_remainder_window transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The buyer-default path: `remainder_deadline` has *strictly* passed
   * (`now > remainder_deadline`) with the remainder never funded — in
   * the deadline second itself this refuses with
   * `RemainderWindowNotPassed`, so the buyer's own funding window is
   * never cut short by a second. Sweeps whatever's currently escrowed
   * (the deposit, or whatever of it the cooperative hasn't already
   * claimed) to the cooperative and sets `Defaulted` — uncontested by
   * construction, so unlike `cancel`/`reassign_buyer` this needs no
   * consent from anyone.
   * 
   * Permissionless, deliberately: the outcome (sweep to cooperative)
   * doesn't depend on who calls it, only on whether the deadline has
   * passed, the same reasoning `reclaim_tranche` would use if it
   * weren't already scoped to benefit the buyer specifically. Anyone —
   * including an off-chain watcher — can trigger it once the fact of
   * the matter (deadline passed, unfunded) is true.
   * 
   * **Unreachable for a fully-prepaid commitment (#275)**: when the
   * advance bps sum to 10 000 there is no remainder, `ready_
   */
  expire_remainder_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a refresh_fx_quote_status transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Records whether settlement's FX resolution is currently blocked
   * on a stale oracle quote (#1) — the "waiting on a fresh quote"
   * signal, made readable instead of inferable only from a failed
   * `resolve_fx_shortfall` call.
   * 
   * Why this exists as its own call: Soroban rolls back every state
   * write an invocation makes when it returns `Err`, so
   * `resolve_fx_shortfall`'s `OracleStale` refusal cannot persist any
   * marker of its own — by the time a caller sees that error, nothing
   * was recorded. This probe is the write that refusal can't be: it
   * runs the same staleness check and records the answer in
   * `Commitment::fx_quote_stale`, which `get_commitment` surfaces to
   * the API/indexers/UI.
   * 
   * Sets the flag when the quote is stale; clears it when a fresh
   * quote is read — so the flag self-corrects the moment Reflector
   * publishes a fresh one, with no reverse transition to learn. A
   * missing quote (`OraclePriceUnavailable`) propagates as an error
   * instead of setting anything: "the oracle doesn't quote this asset
   * at all" is a permanent confi
   */
  refresh_fx_quote_status: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a extend_delivery_deadline transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Extends the delivery window by mutual consent while the commitment is
   * still pre-delivery and the original deadline has not passed. The
   * deadline is monotonic: a consent call can only add time, never revoke
   * time that was already promised to the cooperative.
   */
  extend_delivery_deadline: ({new_deadline}: {new_deadline: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

  /**
   * Construct and simulate a expire_fx_shortfall_window transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The FX-shortfall buyer-default path: `fx_shortfall_deadline` has
   * *strictly* passed (`now > fx_shortfall_deadline`; the deadline
   * second itself still refuses with `FxShortfallWindowNotPassed`) with
   * the top-up never funded. Sweeps whatever's currently
   * escrowed to the cooperative and sets `Status::Defaulted` — the
   * *same* status `expire_remainder_window`'s buyer-default already
   * uses, by explicit product decision (see module doc), which means
   * the API's existing reputation consequence (immediate permanent
   * bar) applies here with no changes needed there at all.
   * 
   * Permissionless, same reasoning as `expire_remainder_window`: the
   * outcome doesn't depend on who calls it, only on whether the
   * deadline has passed.
   */
  expire_fx_shortfall_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

}
export class Client extends ContractClient {
  static async deploy<T = Client>(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions &
      Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
      }
  ): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(null, options)
  }
  constructor(public readonly options: ContractClientOptions) {
    super(
      new ContractSpec([ "AAAABAAAAAAAAAAAAAAABUVycm9yAAAAAAAAMAAAAAAAAAASQWxyZWFkeUluaXRpYWxpemVkAAAAAAABAAAAAAAAAA5Ob3RJbml0aWFsaXplZAAAAAAAAgAAAAAAAAAMSW52YWxpZFN0YXRlAAAAAwAAAAAAAAAKSW52YWxpZEJwcwAAAAAABAAAAAAAAAAKWmVyb0Ftb3VudAAAAAAABQAAAAAAAAANSW52YWxpZFdpbmRvdwAAAAAAAAYAAAAAAAAADE5vdFlldE9wZW5lZAAAAAcAAAAAAAAADkFscmVhZHlDbGFpbWVkAAAAAAAIAAAAAAAAAA5BbHJlYWR5RXhwaXJlZAAAAAAACQAAAAAAAAARQ2xhaW1XaW5kb3dQYXNzZWQAAAAAAAAKAAAAAAAAABRDbGFpbVdpbmRvd05vdFBhc3NlZAAAAAsAAADiQSB0cmFuY2hlIGlzIG5laXRoZXIgY2xhaW1lZCBub3IgZXhwaXJlZCB5ZXQg4oCUIGBzZXR0bGVgIHJlZnVzZXMgdG8KcnVuIHVudGlsIHNvbWVvbmUgcmVzb2x2ZXMgaXQgdmlhIGBjbGFpbV9hZHZhbmNlXypgIG9yCmByZWNsYWltX2FkdmFuY2VfKmAuIFNlZSBgc2V0dGxlYCdzIGRvYyBjb21tZW50IGZvciB3aHkgdGhpcyBpcyBhCmhhcmQgcmVxdWlyZW1lbnQsIG5vdCBqdXN0IGEgbmljZXR5LgAAAAAAEVRyYW5jaGVVbnJlc29sdmVkAAAAAAAADAAAAAAAAAAWUmVtYWluZGVyQWxyZWFkeUZ1bmRlZAAAAAAADQAAAHlgZnVuZF9yZW1haW5kZXJgIGNhbGxlZCBhZnRlciBgcmVtYWluZGVyX2RlYWRsaW5lYCBoYXMgKnN0cmljdGx5KgpwYXNzZWQg4oCUIHRoZSBkZWFkbGluZSBzZWNvbmQgaXRzZWxmIGlzIHN0aWxsIGluIHRpbWUuAAAAAAAAFVJlbWFpbmRlcldpbmRvd1Bhc3NlZAAAAAAAAA4AAADYYGV4cGlyZV9yZW1haW5kZXJfd2luZG93YCBjYWxsZWQgYXQgb3IgYmVmb3JlCmByZW1haW5kZXJfZGVhZGxpbmVgIOKAlCB0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBzdGlsbCBiZWxvbmdzIHRvCnRoZSBidXllcidzIGZ1bmRpbmcgd2luZG93LCBzbyBleHBpcnkgbmVlZHMgc3RyaWN0bHkgbW9yZSB0aGFuIHRoZQpkZWFkbGluZSwgbm90IG1lcmVseSB0aGUgZGVhZGxpbmUuAAAAGFJlbWFpbmRlcldpbmRvd05vdFBhc3NlZAAAAA8AAABhYGNvbmZpcm1fZGVsaXZlcnlgIGNhbGxlZCB3aGlsZSBgUmVhZHlGb3JEZWxpdmVyeWAgYnV0IHRoZSBidXllcgpoYXNuJ3QgZnVuZGVkIHRoZSByZW1haW5kZXIgeWV0LgAAAAAAABJSZW1haW5kZXJOb3RGdW5kZWQAAAAAABAAAACBYHJlY2xhaW1fb25fbm9uZGVsaXZlcnlgIGNhbGxlZCBhdCBvciBiZWZvcmUgYGRlbGl2ZXJ5X2RlYWRsaW5lYCDigJQKdGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgc3RpbGwgYmVsb25ncyB0byB0aGUgY29vcGVyYXRpdmUuAAAAAAAAGURlbGl2ZXJ5RGVhZGxpbmVOb3RQYXNzZWQAAAAAAAARAAAAL2Bjb250cmFjdGVkX3F1YW50aXR5YCB3YXMgemVybyBhdCBgaW5pdGlhbGl6ZWAuAAAAAA9JbnZhbGlkUXVhbnRpdHkAAAAAEgAAAIhgZ3JhZGVfcHJpY2VfYnBzYCB3YXMgZW1wdHksIG9yIGNvbnRhaW5lZCBhbiBlbnRyeSBvdmVyIDEwXzAwMAooYSBncmFkZSB3b3J0aCBtb3JlIHRoYW4gdGhlIHByZS1hZ3JlZWQgZnVsbCB1bml0IHByaWNlKSBhdApgaW5pdGlhbGl6ZWAuAAAAFEludmFsaWRHcmFkZVNjaGVkdWxlAAAAEwAAAEhgY29uZmlybV9kZWxpdmVyeWAncyBgZ3JhZGVfaW5kZXhgIGRpZG4ndCBsYW5kIGluc2lkZQpgZ3JhZGVfcHJpY2VfYnBzYC4AAAARSW52YWxpZEdyYWRlSW5kZXgAAAAAAAAUAAAAaGBzZXRfYWxsb2NhdGlvbmAgY2FsbGVkIGEgc2Vjb25kIHRpbWUg4oCUIHRoZSBhbGxvY2F0aW9uIGxlZGdlciBpcwpvbmUtdGltZSBhbmQgaW1tdXRhYmxlIG9uY2UgcmVjb3JkZWQuAAAAFEFsbG9jYXRpb25BbHJlYWR5U2V0AAAAFQAAAGBgc2V0X2FsbG9jYXRpb25gJ3MgYG1lbWJlcnNgIHdhcyBlbXB0eSwgb3IgdGhlIHN1bSBvZiBldmVyeQplbnRyeSdzIGBzaGFyZV9icHNgIGV4Y2VlZGVkIDEwXzAwMC4AAAARSW52YWxpZEFsbG9jYXRpb24AAAAAAAAWAAAAOWBnZXRfYWxsb2NhdGlvbmAgY2FsbGVkIGJlZm9yZSBgc2V0X2FsbG9jYXRpb25gIGV2ZXIgcmFuLgAAAAAAABBBbGxvY2F0aW9uTm90U2V0AAAAFwAAAd9gaW5pdGlhbGl6ZWAncyBgb3JhY2xlX2NvbmZpZ2Agd2FzIGBTb21lYCB3aXRoIGVpdGhlcjoKLSBgbWF4X2FnZV9zZWNzID09IDBgOiBhIHplcm8gc3RhbGVuZXNzIGJvdW5kIHdvdWxkIHJlamVjdCBldmVyeQpyZWFsIHF1b3RlLCB3aGljaCBpcyBuZXZlciB3aGF0IGEgY2FsbGVyIGFjdHVhbGx5IHdhbnRzICh0aGF0J3MKd2hhdCBsZWF2aW5nIGBvcmFjbGVfY29uZmlnYCBhcyBgTm9uZWAgaXMgZm9yKTsgb3IKLSBgZGVub21pbmF0ZWRfYW1vdW50IDw9IDBgIG9yIGA+IE1BWF9ERU5PTUlOQVRFRF9BTU9VTlRgICgjMjQ3KToKYSBub24tcG9zaXRpdmUgdmFsdWUgd291bGQgbWFrZSBgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIG93ZSB0aGUKY29vcGVyYXRpdmUgbm90aGluZyBhZnRlciBhIGZ1bGwgZGVsaXZlcnksIGFuZCBhbiBvdmVyc2l6ZWQgb25lCndvdWxkIG92ZXJmbG93IGBicHNfYW1vdW50YCBhbmQgdHJhcCBhdCBzZXR0bGVtZW50LgAAAAATSW52YWxpZE9yYWNsZUNvbmZpZwAAAAAYAAAAr2BvcmFjbGVfcmF0ZWAgb3IgYHJlc29sdmVfZnhfc2hvcnRmYWxsYCBjYWxsZWQgYnV0IGBpbml0aWFsaXplYApuZXZlciBzZXQgYW4gYG9yYWNsZV9jb25maWdgIGZvciB0aGlzIGNvbW1pdG1lbnQg4oCUIHRoZSBvbmx5IHR3bwpmdW5jdGlvbnMgdGhhdCByZWFkIHRoZSBvcmFjbGUgYXQgYWxsICgjMTI0KS4AAAAAE09yYWNsZU5vdENvbmZpZ3VyZWQAAAAAGQAAALZUaGUgY29uZmlndXJlZCBSZWZsZWN0b3Igb3JhY2xlIHJldHVybmVkIGBOb25lYCBmb3IKYG9yYWNsZV9jb25maWcucHJpY2VfYXNzZXRgIOKAlCBpdCBkb2Vzbid0IHF1b3RlIHRoYXQgYXNzZXQgYXQgYWxsCihjYWxsIHRoZSBvcmFjbGUncyBvd24gYGFzc2V0cygpYCB0byBjaGVjayB3aGF0IGl0IGRvZXMgcXVvdGUpLgAAAAAAFk9yYWNsZVByaWNlVW5hdmFpbGFibGUAAAAAABoAAADdVGhlIG1vc3QgcmVjZW50IHF1b3RlIGlzIG9sZGVyIHRoYW4gYG9yYWNsZV9jb25maWcubWF4X2FnZV9zZWNzYAphbGxvd3MgKFBSRCDCpzE2LjMncyBzdGFsZW5lc3MgYm91bmQpIOKAlCByZWZ1c2VkIHJhdGhlciB0aGFuCnJldHVybmVkLCBzaW5jZSBhIGNhbGxlciBzaWxlbnRseSBnZXR0aW5nIGEgdG9vLW9sZCByYXRlIGlzIHdvcnNlCnRoYW4gZ2V0dGluZyBubyByYXRlIGF0IGFsbC4AAAAAAAALT3JhY2xlU3RhbGUAAAAAGwAAANpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIGNhbGxlZCBhIHNlY29uZCB0aW1lIOKAlCBvbmUtdGltZSBhbmQKaW1tdXRhYmxlLCBzYW1lIHJlYXNvbmluZyBhcyBgc2V0X2FsbG9jYXRpb25gOiByZS1yZXNvbHZpbmcgbGF0ZXIKYWdhaW5zdCBhIGRpZmZlcmVudCByYXRlIHdvdWxkIGxldCBlaXRoZXIgc2lkZSBnYW1lIHRoZSBudW1iZXIKYnkgY2hvb3Npbmcgd2hlbiB0byBjYWxsIGl0LgAAAAAAEUZ4QWxyZWFkeVJlc29sdmVkAAAAAAAAHAAAAItgZnVuZF9meF9zaG9ydGZhbGxgL2BleHBpcmVfZnhfc2hvcnRmYWxsX3dpbmRvd2AvYHNldHRsZWAgY2FsbGVkCmJlZm9yZSBgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIGhhcyBydW4gZm9yIGFuIG9yYWNsZS1jb25maWd1cmVkCmNvbW1pdG1lbnQuAAAAAA1GeE5vdFJlc29sdmVkAAAAAAAAHQAAAEBgc2V0dGxlYCBjYWxsZWQgd2hpbGUgYSByZXNvbHZlZCBGWCBzaG9ydGZhbGwgaXMgc3RpbGwgdW5mdW5kZWQuAAAAE0Z4U2hvcnRmYWxsVW5mdW5kZWQAAAAAHgAAAN1gZnVuZF9meF9zaG9ydGZhbGxgL2BleHBpcmVfZnhfc2hvcnRmYWxsX3dpbmRvd2AgY2FsbGVkIGJ1dApgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIGZvdW5kIG5vIHNob3J0ZmFsbCBhdCBhbGwgKHRoZSBlc2Nyb3dlZAphbW91bnQgYWxyZWFkeSBjb3ZlcmVkIHRoZSBmcmVzaC1yYXRlLWNvbnZlcnRlZCBvYmxpZ2F0aW9uKSDigJQKdGhlcmUncyBub3RoaW5nIHRvIGZ1bmQgb3IgZXhwaXJlLgAAAAAAAA1Ob0Z4U2hvcnRmYWxsAAAAAAAAHwAAAClgZnVuZF9meF9zaG9ydGZhbGxgIGNhbGxlZCBhIHNlY29uZCB0aW1lLgAAAAAAABhGeFNob3J0ZmFsbEFscmVhZHlGdW5kZWQAAAAgAAAA1mBmdW5kX2Z4X3Nob3J0ZmFsbGAgY2FsbGVkIGFmdGVyIGBmeF9zaG9ydGZhbGxfZGVhZGxpbmVgIGhhcwoqc3RyaWN0bHkqIHBhc3NlZCDigJQgdG9vIGxhdGUgdG8gY3VyZSwgdGhvdWdoIHRoZSBkZWFkbGluZSBzZWNvbmQKaXRzZWxmIGlzIHN0aWxsIGluIHRpbWUuIGBleHBpcmVfZnhfc2hvcnRmYWxsX3dpbmRvd2AgaXMgdGhlIG9ubHkKcGF0aCBsZWZ0IGZyb20gaGVyZS4AAAAAABdGeFNob3J0ZmFsbFdpbmRvd1Bhc3NlZAAAAAAhAAAA3WBleHBpcmVfZnhfc2hvcnRmYWxsX3dpbmRvd2AgY2FsbGVkIGF0IG9yIGJlZm9yZQpgZnhfc2hvcnRmYWxsX2RlYWRsaW5lYCDigJQgdGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgc3RpbGwgYmVsb25ncwp0byB0aGUgYnV5ZXIncyBmdW5kaW5nIHdpbmRvdywgbWF0Y2hpbmcgYGZ1bmRfZnhfc2hvcnRmYWxsYCdzIG93bgppbmNsdXNpdmUgY3V0LW9mZiAoYG5vdyA+IGRlYWRsaW5lYCkuAAAAAAAAGkZ4U2hvcnRmYWxsV2luZG93Tm90UGFzc2VkAAAAAAAiAAAAqWBmbGFnX2Rpc3B1dGVgJ3MgYGZsYWdnZXJgIGFyZ3VtZW50IHdhc24ndCB0aGUgYnV5ZXIsIGNvb3BlcmF0aXZlLApvciB3YXJlaG91c2Ugb3BlcmF0b3Ig4oCUIG9ubHkgYSBuYW1lZCBwYXJ0eSB0byB0aGUgZGVhbCBjYW4gZmxhZwpvbmUsIG5vdCBhbiBhcmJpdHJhcnkgdGhpcmQgYWRkcmVzcy4AAAAAAAAJTm90QVBhcnR5AAAAAAAAIwAAAVZgZXhwaXJlX2Rpc3B1dGVfd2luZG93YCBjYWxsZWQgYXQgb3IgYmVmb3JlIGBkaXNwdXRlX2RlYWRsaW5lYCDigJQKdGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgaXMgc3RpbGwgaW5zaWRlIHRoZSBmcmVlemUuIE9ubHkgdGhhdAp0aW1lb3V0IHBhdGggY2FuIHByb2R1Y2UgdGhpczogYHJlc29sdmVfZGlzcHV0ZWAgaGFzIG5vIGRlYWRsaW5lCmdhdGUgYXQgYWxsLiAoRWl0aGVyIGNhbGwgbWFkZSB3aGlsZSBgc3RhdHVzYCBpc24ndCBgRGlzcHV0ZWRgIGlzCmNvdmVyZWQgYnkgdGhlIHNhbWUgYEludmFsaWRTdGF0ZWAgZXZlcnkgb3RoZXIgc3RhdHVzLWdhdGVkIGNhbGwKYWxyZWFkeSB1c2VzLikAAAAAABZEaXNwdXRlV2luZG93Tm90UGFzc2VkAAAAAAAkAAAArmBpbml0aWFsaXplYCdzIGBncmFkZV9wcmljZV9icHNgIGhlbGQgbW9yZSB0aGFuCmBNQVhfR1JBREVfRU5UUklFU2AgZW50cmllcyDigJQgbG9uZ2VyIHRoYW4gYW55IHJlYWwgZ3JhZGluZwpzY2hlZHVsZSBuZWVkcywgYW5kIHVuYm91bmRlZCB3b3JrIHRoZSBjb250cmFjdCBzaG91bGRuJ3QgYWNjZXB0LgAAAAAAFEdyYWRlU2NoZWR1bGVUb29Mb25nAAAAJQAAALNgc2V0X2FsbG9jYXRpb25gJ3MgYG1lbWJlcnNgIGhlbGQgbW9yZSB0aGFuCmBNQVhfQUxMT0NBVElPTl9NRU1CRVJTYCBlbnRyaWVzIOKAlCBiZXlvbmQgdGhlIFBSRCdzIHNwZWNpZmllZApjb29wZXJhdGl2ZSBzY2FsZSwgYW5kIHVuYm91bmRlZCBzdG9yYWdlIHRoZSBjb250cmFjdCBzaG91bGRuJ3QKYWNjZXB0LgAAAAASQWxsb2NhdGlvblRvb0xhcmdlAAAAAAAmAAAA/GBpbml0aWFsaXplYCdzIGB0b3RhbF9hbW91bnRgIGV4Y2VlZGVkIGBNQVhfVE9UQUxfQU1PVU5UYAooYGkxMjg6Ok1BWCAvIDEwXzAwMGApIOKAlCBhIGNvbW1pdG1lbnQgd2hvc2UgYGJwc19hbW91bnRgIGFyaXRobWV0aWMKd291bGQgb3ZlcmZsb3csIGFuZCB0aGVyZWZvcmUgb25lIHRoYXQgY291bGQgbmV2ZXIgYmUgbG9ja2VkIG9yCnNldHRsZWQuIFJlZnVzZWQgYXQgdGhlIGNhbGwgdGhhdCBzdXBwbGllZCB0aGUgdmFsdWUgKCMyNDkpLgAAAA5BbW91bnRUb29MYXJnZQAAAAAAJwAAAIhUaGUgY29uZmlndXJlZCBSZWZsZWN0b3Igb3JhY2xlIHJldHVybmVkIGEgemVybyBvciBuZWdhdGl2ZSBwcmljZTsKbm9uLXBvc2l0aXZlIGV4Y2hhbmdlIHJhdGVzIGNhbm5vdCBwcm9kdWNlIGEgdmFsaWQgc2V0dGxlbWVudCBhbW91bnQuAAAAEk9yYWNsZVByaWNlSW52YWxpZAAAAAAAKAAAAOVgZXhwaXJlX2RlbGl2ZXJ5X3dpbmRvd2AgY2FsbGVkIGJlZm9yZSBgZGVsaXZlcnlfZGVhZGxpbmUgKwpyZW1haW5kZXJfd2luZG93X3NlY3NgIGhhcyBzdHJpY3RseSBwYXNzZWQuIFRoZSBwZXJtaXNzaW9ubGVzcwpmYWxsYmFjayBpcyBpbnRlbnRpb25hbGx5IGRlbGF5ZWQgcGFzdCB0aGUgYnV5ZXIncyBvd24gcmVjbGFpbQp3aW5kb3cgdG8gZ2l2ZSB0aGUgYnV5ZXIgdGltZSB0byBhY3QgZmlyc3QuAAAAAAAAHERlbGl2ZXJ5R3JhY2VXaW5kb3dOb3RQYXNzZWQAAAApAAAAxGBmbGFnX2Rpc3B1dGVgIGNhbGxlZCBhZnRlciB0aGUgcGVyLWNvbW1pdG1lbnQgYE1BWF9ESVNQVVRFX0ZMQUdTYApjYXAgaGFzIGJlZW4gcmVhY2hlZC4gT25seSBgZXhwaXJlX2Rpc3B1dGVfd2luZG93YCBsYXBzYWxzIGNvdW50CmFnYWluc3QgdGhlIGNhcDsgYHJlc29sdmVfZGlzcHV0ZWAgKHVuYW5pbW91cyBjb25zZW50KSBkb2VzIG5vdC4AAAARRGlzcHV0ZUNhcFJlYWNoZWQAAAAAAAAqAAAA+GBjb25maXJtX2RlbGl2ZXJ5YCBjYWxsZWQgYWZ0ZXIgYGRlbGl2ZXJ5X2RlYWRsaW5lYCBoYXMgKnN0cmljdGx5KgpwYXNzZWQgKGBub3cgPiBkZWxpdmVyeV9kZWFkbGluZWApLiBUaGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBzdGlsbApiZWxvbmdzIHRvIHRoZSBvcGVyYXRvcidzIGNvbmZpcm1hdGlvbiB3aW5kb3c7IHN0cmljdGx5IGFmdGVyIGl0LCBvbmx5CmByZWNsYWltX29uX25vbmRlbGl2ZXJ5YCBpcyBsZWdhbCAoIzE5OSkuAAAAFkRlbGl2ZXJ5RGVhZGxpbmVQYXNzZWQAAAAAACsAAACMVGhlIG9yYWNsZSBjb252ZXJzaW9uJ3MgbnVtZXJhdG9yIG9yIGRlY2ltYWwgZGl2aXNvciBjYW5ub3QgYmUKcmVwcmVzZW50ZWQgYXMgYW4gYGkxMjhgOyBzZXR0bGVtZW50IG11c3Qgbm90IHRyYXAgb24gdW51c2FibGUgb3JhY2xlCnZhbHVlcy4AAAAaT3JhY2xlQ29udmVyc2lvbk91dE9mUmFuZ2UAAAAAACwAAACLVGhlIGNvbmZpZ3VyZWQgUmVmbGVjdG9yIG9yYWNsZSByZXR1cm5lZCBhIHF1b3RlIHRpbWVzdGFtcCBsYXRlciB0aGFuCnRoZSBjdXJyZW50IGxlZGdlciB0aW1lc3RhbXA7IGZ1dHVyZSBxdW90ZXMgY2Fubm90IHNhdGlzZnkgZnJlc2huZXNzLgAAAAAXT3JhY2xlVGltZXN0YW1wSW5GdXR1cmUAAAAALQAAAuhUaGUgY29uZmlndXJlZCBvcmFjbGUgY29udHJhY3QgY291bGQgbm90IGFuc3dlciBhdCBhbGwgKCMyNDUpOiB0aGUKY3Jvc3MtY29udHJhY3QgYGxhc3RwcmljZWAgb3IgYGRlY2ltYWxzYCBjYWxsIGZhaWxlZCBpbiBhIHdheSB0aGUKaG9zdCBsZXRzIHRoZSBjYWxsZXIgcmVjb3ZlciBmcm9tLiBUaGF0IGNvdmVycyBhbgpgb3JhY2xlX2NvbmZpZy5vcmFjbGVfY29udHJhY3RgIHdpdGggbm8gY29udHJhY3QgZGVwbG95ZWQgYmVoaW5kIGl0LAphIGNhbGxlZSB0aGF0IHBhbmljcyBvciB0cmFwcywgYW5kIGEgY2FsbGVlIHdob3NlIHJlcGx5IGRvZXNuJ3QKZGVjb2RlIGFzIHRoZSBleHBlY3RlZCB0eXBlLiBEaXN0aW5jdCBmcm9tIGBPcmFjbGVQcmljZVVuYXZhaWxhYmxlYCwKd2hpY2ggbWVhbnMgdGhlIG9yYWNsZSAqZGlkKiBhbnN3ZXIgYW5kIHF1b3RlcyBubyBzdWNoIGFzc2V0LgoKTm90IGV2ZXJ5IGZhaWx1cmUgY2FuIGJlY29tZSB0aGlzIGVycm9yOiB0aGUgaG9zdCBzdGlsbCBhYm9ydHMgdGhlCndob2xlIGludm9jYXRpb24sIHdpdGggbm8gY29udHJhY3QgZXJyb3IgdG8gcmV0dXJuLCB3aGVuIHRoZQpjYWxsZWUgdXNlcyB1cCB0aGUgdHJhbnNhY3Rpb24ncyBzaGFyZWQgQ1BVL21lbW9yeSBidWRnZXQuIFNlZSB0aGUKaGVhZGVyIG9mIGB0ZXN0cy9vcmFjbGVfZmF1bHRfaW5qZWN0aW9uLnJzYCBmb3Igd2hpY2ggZmFpbHVyZXMgYXJlCnJlY292ZXJhYmxlIGFuZCB3aGljaCBzdGlsbCBhYm9ydC4AAAART3JhY2xlVW5hdmFpbGFibGUAAAAAAAAuAAAB4mBsb2NrYCBjYWxsZWQgYWZ0ZXIgYGRlbGl2ZXJ5X2RlYWRsaW5lYCBoYXMgKnN0cmljdGx5KiBwYXNzZWQKKGBub3cgPiBkZWxpdmVyeV9kZWFkbGluZWApIOKAlCB0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBpcyBzdGlsbCBpbgp0aW1lLCBtYXRjaGluZyBldmVyeSBvdGhlciBkZWFkbGluZSBoZXJlLiBUaGUgZGVsaXZlcnkgd2luZG93IGlzIHRoZQpjb29wZXJhdGl2ZSdzIHRpbWUgdG8gcGVyZm9ybSBhbmQgaXQgc3RhcnRzIGF0IGBpbml0aWFsaXplYCAoc2VlCmBDb21taXRtZW50OjpkZWxpdmVyeV9kZWFkbGluZWApLCBzbyBhIGxvY2sgdGhhdCBsYW5kcyBhZnRlciBpdCB3b3VsZApoYW5kIHRoZSBidXllciBhbiBpbW1lZGlhdGUgYHJlY2xhaW1fb25fbm9uZGVsaXZlcnlgIOKAlCBhIGBGb3JmZWl0ZWRgCnN0cmlrZSBhZ2FpbnN0IGEgY29vcGVyYXRpdmUgdGhhdCBuZXZlciBoYWQgYSB3aW5kb3cgdG8gZGVsaXZlciBpbgooIzI3NykuAAAAAAAZTG9ja0FmdGVyRGVsaXZlcnlEZWFkbGluZQAAAAAAAC8AAABaYGV4dGVuZF9kZWxpdmVyeV9kZWFkbGluZWAgd2FzIGFza2VkIHRvIG1vdmUgdGhlIGRlYWRsaW5lIGJhY2t3YXJkcwpvciBsZWF2ZSBpdCB1bmNoYW5nZWQuAAAAAAAbRGVsaXZlcnlEZWFkbGluZU5vdEV4dGVuZGVkAAAAADA=",
        "AAAAAQAAADxPbmUgc3RhdGUtY2hhbmdpbmcgY2FsbCB0aGF0IHRoZSBjdXJyZW50IGNvbW1pdG1lbnQgcGVybWl0cy4AAAAAAAAABkFjdGlvbgAAAAAAAwAAAAAAAAAEYXV0aAAAB9AAAAAJQXV0aFBhcnR5AAAAAAAAAAAAAAhkZWFkbGluZQAAB9AAAAAOQWN0aW9uRGVhZGxpbmUAAAAAADNUaGUgcHVibGljIGZ1bmN0aW9uIHRvIGludm9rZSwgYXMgYSBTb3JvYmFuIHN5bWJvbC4AAAAACGZ1bmN0aW9uAAAAEQ==",
        "AAAAAgAAAAAAAAAAAAAABlN0YXR1cwAAAAAADAAAAAAAAAAAAAAABURyYWZ0AAAAAAAAAAAAAAAAAAAGTG9ja2VkAAAAAAAAAAAAAAAAABBBZHZhbmNlMVJlbGVhc2VkAAAAAAAAAAAAAAAQQ2hlY2twb2ludFBhc3NlZAAAAAAAAAAAAAAAEEFkdmFuY2UyUmVsZWFzZWQAAAAAAAAAZ1RoZSBjb29wZXJhdGl2ZSBoYXMgc2lnbmFsZWQgaW50ZW50IHRvIGRlbGl2ZXIg4oCUIG9wZW5zIHRoZQpyZW1haW5kZXItcGF5bWVudCB3aW5kb3cuIFNlZSBtb2R1bGUgZG9jcy4AAAAAEFJlYWR5Rm9yRGVsaXZlcnkAAAAAAAAAAAAAAAlEZWxpdmVyZWQAAAAAAAAAAAAAAAAAAAdTZXR0bGVkAAAAAAAAAAAAAAAACUNhbmNlbGxlZAAAAAAAAAAAAACAQnV5ZXIgZmFpbGVkIHRvIGZ1bmQgdGhlIHJlbWFpbmRlciBiZWZvcmUgaXRzIGRlYWRsaW5lLiBVbmNvbnRlc3RlZApieSBjb25zdHJ1Y3Rpb24g4oCUIHRoZSBkZWFkbGluZSBlaXRoZXIgcGFzc2VkIG9yIGl0IGRpZG4ndC4AAAAJRGVmYXVsdGVkAAAAAAAAAAAAAWlDb29wZXJhdGl2ZSBuZXZlciByZWFjaGVkIGBEZWxpdmVyZWRgIGJlZm9yZSB0aGUgb3ZlcmFsbCBkZWxpdmVyeQpkZWFkbGluZS4gQWxzbyB1bmNvbnRlc3RlZCBieSBjb25zdHJ1Y3Rpb24uIERlbGliZXJhdGVseSBhCnNlcGFyYXRlIHZhcmlhbnQgZnJvbSBgRGVmYXVsdGVkYCwgbm90IGEgcmV1c2Ugb2YgaXQg4oCUIHRoZSB0d28KcmVwcmVzZW50IG9wcG9zaXRlIHBhcnRpZXMnIGZhaWx1cmUsIGFuZCBjb2xsYXBzaW5nIHRoZW0gaW50byBvbmUKc3RhdHVzIHdvdWxkIG1ha2UgYW4gYWxyZWFkeS1zZXR0bGVkIGNvbW1pdG1lbnQncyBoaXN0b3J5CmFtYmlndW91cyBhYm91dCB3aG8gYWN0dWFsbHkgZmFpbGVkIHRvIHBlcmZvcm0uAAAAAAAACUZvcmZlaXRlZAAAAAAAAAAAAAAAAAAACERpc3B1dGVk",
        "AAAABQAAAUpFbWl0dGVkIGFmdGVyIGV2ZXJ5IGZ1bmQgdHJhbnNmZXIgaW4gYSB0ZXJtaW5hbCBvciBwYXlvdXQgcGF0aCDigJQKYHNldHRsZWAgKG9uY2UgcGVyIG5vbi16ZXJvIGxlZyksIGBjYW5jZWxgLCBgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgLApgcmVjbGFpbV9vbl9ub25kZWxpdmVyeWAsIGFuZCBgZXhwaXJlX2Z4X3Nob3J0ZmFsbF93aW5kb3dgLgpgcmVhc29uYCBpcyBhIHNob3J0IHN5bWJvbCBuYW1pbmcgdGhlIGNhbGwgc28gYW4gaW5kZXhlciBjYW4gcmVjb25zdHJ1Y3QKZWFjaCBwYXJ0eSdzIGZpbmFsIGJhbGFuY2UgZnJvbSBlc2Nyb3cgZXZlbnRzIGFsb25lICgjMjA0KS4AAAAAAAAAAAAGUGF5b3V0AAAAAAABAAAABnBheW91dAAAAAAAAwAAAAAAAAAGcmVhc29uAAAAAAARAAAAAQAAAAAAAAACdG8AAAAAABMAAAAAAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAAg==",
        "AAAAAgAAAFpXaG9zZSBhdXRob3JpemF0aW9uIGEgY2FsbGFibGUgYWN0aW9uIG5lZWRzLCBhcyBhZHZlcnRpc2VkIGJ5CltgU2VsZjo6YXZhaWxhYmxlX2FjdGlvbnNgXS4AAAAAAAAAAAAJQXV0aFBhcnR5AAAAAAAACQAAAAAAAAA6Tm8gYHJlcXVpcmVfYXV0aCgpYCBhdCBhbGwg4oCUIGFueW9uZSBtYXkgc3VibWl0IHRoZSBjYWxsLgAAAAAABkFueW9uZQAAAAAAAAAAACFUaGUgY29tbWl0bWVudCdzIGN1cnJlbnQgYGJ1eWVyYC4AAAAAAAAFQnV5ZXIAAAAAAAAAAAAAH1RoZSBjb21taXRtZW50J3MgYGNvb3BlcmF0aXZlYC4AAAAAC0Nvb3BlcmF0aXZlAAAAAAAAAAAmVGhlIGNvbW1pdG1lbnQncyBgd2FyZWhvdXNlX29wZXJhdG9yYC4AAAAAABFXYXJlaG91c2VPcGVyYXRvcgAAAAAAAAAAAAA9Qm90aCB0aGUgYnV5ZXIgYW5kIHRoZSBjb29wZXJhdGl2ZSAoZGVkdXBsaWNhdGVkIGJ5IGFkZHJlc3MpLgAAAAAAABNCdXllckFuZENvb3BlcmF0aXZlAAAAAAAAAABaQWxsIHRocmVlIG5hbWVkIHBhcnRpZXMgKGJ1eWVyLCBjb29wZXJhdGl2ZSwgd2FyZWhvdXNlIG9wZXJhdG9yKSwKZGVkdXBsaWNhdGVkIGJ5IGFkZHJlc3MuAAAAAAAKQWxsUGFydGllcwAAAAAAAAAAAH1Bbnkgb25lIG9mIHRoZSB0aHJlZSBuYW1lZCBwYXJ0aWVzLCBhdXRoZW50aWNhdGluZyBhcyB0aGUgYGZsYWdnZXJgCml0IHBhc3Nlcy4gU3RyYW5nZXJzIGFyZSByZWZ1c2VkIHdpdGggYEVycm9yOjpOb3RBUGFydHlgLgAAAAAAAA1BbnlOYW1lZFBhcnR5AAAAAAAAAAAAAIJDdXJyZW50IGJ1eWVyLCBjb29wZXJhdGl2ZSwgKiphbmQqKiB0aGUgaW5jb21pbmcgYnV5ZXIgc3VwcGxpZWQgYXMKYG5ld19idXllcmAg4oCUIHRoZSB0aHJlZSBzaWduYXR1cmVzIGByZWFzc2lnbl9idXllcmAgcmVxdWlyZXMuAAAAAAAbQnV5ZXJDb29wZXJhdGl2ZUFuZE5ld0J1eWVyAAAAAAAAAACLQ3VycmVudCBidXllciwgY29vcGVyYXRpdmUsICoqYW5kKiogdGhlIGluY29taW5nIG9wZXJhdG9yIHN1cHBsaWVkIGFzCmBuZXdfb3BlcmF0b3JgIOKAlCB0aGUgdGhyZWUgc2lnbmF0dXJlcyBgcmVhc3NpZ25fb3BlcmF0b3JgIHJlcXVpcmVzLgAAAAAeQnV5ZXJDb29wZXJhdGl2ZUFuZE5ld09wZXJhdG9yAAA=",
        "AAAAAQAABABPbi1jaGFpbiBzdGF0ZSBmb3IgYSBzaW5nbGUgZXNjcm93IGNvbW1pdG1lbnQgaW5zdGFuY2UuCgoqKkZvb3RwcmludCBBdWRpdCAmIFN0b3JhZ2UgTmVjZXNzaXR5IEZpbmRpbmcqKjoKQSBjb21wcmVoZW5zaXZlIHJldmlldyBvZiBldmVyeSBmaWVsZCBpbiB0aGlzIHN0cnVjdCBjb25maXJtZWQgdGhhdCBhbGwgMzMKZmllbGRzIGFyZSBnZW51aW5lbHkgbmVjZXNzYXJ5IGluIG9uLWNoYWluIGluc3RhbmNlIHN0b3JhZ2UgYW5kIGNhbm5vdCBiZQptb3ZlZCBvZmYtY2hhaW4gb3IgZGVyaXZlZCBmcm9tIGV2ZW50czoKMS4gKipBdXRoICYgVG9rZW4gUm91dGluZyoqIChgYnV5ZXJgLCBgY29vcGVyYXRpdmVgLCBgd2FyZWhvdXNlX29wZXJhdG9yYCwgYHRva2VuYCk6CkVuZm9yY2VzIGByZXF1aXJlX2F1dGgoKWAgYW5kIHJvdXRlcyB0b2tlbiB0cmFuc2ZlcnMgYWNyb3NzIGV2ZXJ5IGxpZmVjeWNsZSBjYWxsLgoyLiAqKkZpbmFuY2lhbCBUZXJtcyAmIFNjaGVkdWxlcyoqIChgdG90YWxfYW1vdW50YCwgYGFkdmFuY2UxX2Jwc2AsIGBhZHZhbmNlMl9icHNgLApgY2xhaW1fd2luZG93X3NlY3NgLCBgcmVtYWluZGVyX3dpbmRvd19zZWNzYCwgYGRlbGl2ZXJ5X2RlYWRsaW5lYCwgYGNvbnRyYWN0ZWRfcXVhbnRpdHlgLApgZ3JhZGVfcHJpY2VfYnBzYCk6ClNtYXJ0IGNvbnRyYWN0cyBjYW5ub3QgcXVlcnkgaGlzdG9yaWNhbCBldmVudCBsb2dzOyBldmVyeSBndWFyZCwgZGVhZGxpbmUgY2hlY2ssCmFuZCBzZXR0bGVtZW50IHBheW91dCBlcXVhdGlvbiByZXF1aXJlcyB0aGVzZSBiYXNlbGluZSB0ZXJtcyBvbi1jaGFpbi4KMy4gKipTdGF0ZSAmIExpZmVjeWNsZSBGbGFncyoqIChgc3RhdHVzYCwgYGNyZWF0ZWRfYXRgLCBgYWR2YW5jZTFfZGVhZGxpbmVgLApgYWR2YW5jZTFfY2xhaW1lZGAsIGBhZHZhbmNlMV9leHBpcmVkYCwgYGFkdmFuY2UyX2RlYWRsaW5lYCwgYGFkdmFuY2UyX2NsYWltZWRgLApgYWR2YW5jZTJfZXhwaXJlZGAsIGByZW1haW5kZXJfZGVhZGxpbmVgLCBgcmVtYWluZGVyX2Z1bmRlAAAAAAAAAApDb21taXRtZW50AAAAAAAiAAAAP0Jhc2lzIHBvaW50cyAob2YgYHRvdGFsX2Ftb3VudGApIHJlbGVhc2VkIGF0IGxvY2staW4uIDAtMTBfMDAwLgAAAAAMYWR2YW5jZTFfYnBzAAAABAAAAAAAAAAQYWR2YW5jZTFfY2xhaW1lZAAAAAEAAACxMCA9IG5vdCB5ZXQgb3BlbmVkLiBPbmNlIGByZWxlYXNlX2FkdmFuY2VfMWAgcnVucywgdGhpcyBpcyBzZXQgdG8KdGhlIGxlZGdlciB0aW1lc3RhbXAgYWZ0ZXIgd2hpY2ggdGhlIGNvb3BlcmF0aXZlIGNhbiBubyBsb25nZXIKY2xhaW0gYW5kIHRoZSBidXllciBiZWNvbWVzIGVsaWdpYmxlIHRvIHJlY2xhaW0uAAAAAAAAEWFkdmFuY2UxX2RlYWRsaW5lAAAAAAAABgAAAAAAAAAQYWR2YW5jZTFfZXhwaXJlZAAAAAEAAADyQmFzaXMgcG9pbnRzIHJlbGVhc2VkIGF0IHRoZSBtaWQtc2Vhc29uIGNoZWNrcG9pbnQuIDAtMTBfMDAwLgpgYWR2YW5jZTFfYnBzICsgYWR2YW5jZTJfYnBzYCBtdXN0IG5vdCBleGNlZWQgMTBfMDAwIOKAlCBlbmZvcmNlZCBhdApgaW5pdGlhbGl6ZWAuIFRvZ2V0aGVyIHRoZXNlIHR3byBkZWZpbmUgdGhlICpkZXBvc2l0KiDigJQgdGhlCnBvcnRpb24gYGxvY2tgIGFjdHVhbGx5IGVzY3Jvd3M7IHNlZSBtb2R1bGUgZG9jcy4AAAAAAAxhZHZhbmNlMl9icHMAAAAEAAAAAAAAABBhZHZhbmNlMl9jbGFpbWVkAAAAAQAAAAAAAAARYWR2YW5jZTJfZGVhZGxpbmUAAAAAAAAGAAAAAAAAABBhZHZhbmNlMl9leHBpcmVkAAAAAQAAAAAAAAAFYnV5ZXIAAAAAAAATAAADVEhvdyBsb25nIHRoZSBjb29wZXJhdGl2ZSBoYXMgdG8gY2xhaW0gYW4gYWR2YW5jZSBvbmNlIGl0IG9wZW5zLAppbiBzZWNvbmRzLiBTYW1lIHdpbmRvdyBmb3IgYm90aCB0cmFuY2hlcyDigJQgUFJEIGRvZXNuJ3QgY2FsbCBmb3IKZGlmZmVyZW50IHdpbmRvd3MgcGVyIHRyYW5jaGUsIHNvIG9uZSBzaGFyZWQgdmFsdWUgaXMgdGhlIHNpbXBsZXIKY2hvaWNlIHVudGlsIGEgcmVhc29uIHRvIHNwbGl0IHRoZW0gc2hvd3MgdXAuCgpCb3VuZGFyeSBjb252ZW50aW9uICgjMTExLCBwaW5uZWQgYnkgYHRlc3RzL2RlYWRsaW5lX2JvdW5kYXJ5LnJzYCk6CnRoZSBkZWFkbGluZSBpcyBgcmVsZWFzZSB0aW1lICsgY2xhaW1fd2luZG93X3NlY3NgLCBhbmQgKmNsYWltaW5nKiBpcwphbGxvd2VkIHVwIHRvICoqYW5kIGluY2x1ZGluZyoqIHRoYXQgaW5zdGFudCAoYHRpbWVzdGFtcCA+IGRlYWRsaW5lYAppcyB0aGUgcmVqZWN0aW9uIGd1YXJkKSB3aGlsZSAqcmVjbGFpbWluZyogaXMgYWxsb3dlZCBvbmx5ICoqc3RyaWN0bHkKYWZ0ZXIqKiBpdCAoYHRpbWVzdGFtcCA8PSBkZWFkbGluZWAgaXMgdGhlIHJlamVjdGlvbiBndWFyZCkuIFRoZSB0d28KZ3VhcmRzIGFyZSBleGFjdCBjb21wbGVtZW50cywgc28gZXZlcnkgbGVkZ2VyIHRpbWVzdGFtcCBiZWxvbmdzIHRvCmV4YWN0bHkgb25lIHBhcnR5LiBDb25zZXF1ZW5jZSB3b3J0aCBrbm93aW5nOiB0aGUgY29vcGVyYXRpdmUncwplZmZlY3RpdmUgd2luZG93IHNwYW5zIGBjbGFpbV93aW5kb3dfc2VjcyArIDFgIGRpc3RpbmN0IHRpbWVzdGFtcHMKKHRoZSBvcGVuaW5nIG9uZSBhbmQgdGhlIGRlYWRsaW5lIG9uZSksIG5vdCBgY2xhaW1fd2luZG93X3NlY3NgLgAAABFjbGFpbV93aW5kb3dfc2VjcwAAAAAAAAYAAADDVGhlIHF1YW50aXR5IChjYWxsZXItZGVmaW5lZCB1bml0LCBlLmcuIGtnKSB0aGUgYnV5ZXIgaXMKY29udHJhY3RpbmcgZm9yLiBTZXQgYXQgYGluaXRpYWxpemVgLCBuZXZlciBjaGFuZ2VzLiBUaGUKZGVub21pbmF0b3IgZm9yIGBjb25maXJtX2RlbGl2ZXJ5YCdzIHByb3BvcnRpb25hbCBzaG9ydGZhbGwKYWRqdXN0bWVudCAoUFJEIMKnNykuAAAAABNjb250cmFjdGVkX3F1YW50aXR5AAAAAAQAAAAAAAAAC2Nvb3BlcmF0aXZlAAAAABMAAAAAAAAACmNyZWF0ZWRfYXQAAAAAAAYAAAC9U2V0IGJ5IGBjb25maXJtX2RlbGl2ZXJ5YC4gMCB1bnRpbCB0aGVuIChhbHNvIGEgbGVnaXRpbWF0ZSByZWFsCnZhbHVlIOKAlCB0b3RhbCBjcm9wIGZhaWx1cmUg4oCUIGJ1dCBub3RoaW5nIHJlYWRzIHRoaXMgZmllbGQgYmVmb3JlCmBTdGF0dXM6OkRlbGl2ZXJlZGAsIHNvIG5vIHNlcGFyYXRlIHNlbnRpbmVsIGlzIG5lZWRlZCkuAAAAAAAAEmRlbGl2ZXJlZF9xdWFudGl0eQAAAAAABAAAA8lBYnNvbHV0ZSBkZWFkbGluZSAoYGNyZWF0ZWRfYXQgKyBkZWxpdmVyeV93aW5kb3dfc2Vjc2ApLCBjb21wdXRlZApvbmNlIGF0IGBpbml0aWFsaXplYCDigJQgdW5saWtlIHRoZSB0cmFuY2hlL3JlbWFpbmRlciBkZWFkbGluZXMsCnRoaXMgb25lIGRvZXNuJ3QgZGVwZW5kIG9uIHNvbWUgb3RoZXIgY2FsbCBoYXBwZW5pbmcgZmlyc3QsIHNvCnRoZXJlJ3Mgbm8gIjAgPSB1bnNldCIgc3RhdGUgZm9yIGl0LgoKKipUaGUgZGVsaXZlcnkgd2luZG93IHN0YXJ0cyBhdCBgaW5pdGlhbGl6ZWAsIG5vdCBhdCBgbG9ja2AqKjogaXQgaXMKdGhlIGNvb3BlcmF0aXZlJ3MgdGltZSB0byByZWFjaCBgRGVsaXZlcmVkYCwgYW5jaG9yZWQgdG8gdGhlIG1vbWVudAp0aGUgZGVhbCB3YXMgY3JlYXRlZCAoYGNyZWF0ZWRfYXRgKSwgYW5kIGl0IGlzIHRoZXJlZm9yZSBhbHJlYWR5CnRpY2tpbmcgd2hpbGUgdGhlIGNvbW1pdG1lbnQgc2l0cyBpbiBgRHJhZnRgLiBUaGF0IGlzIGRlbGliZXJhdGUg4oCUCnRoZSB3aW5kb3cgaXMgdGhlIGJ1eWVyJ3MgcHJvdGVjdGlvbiBhZ2FpbnN0IGEgY29vcGVyYXRpdmUgdGhhdApuZXZlciBkZWxpdmVycywgYW5kIHJlLWFuY2hvcmluZyBpdCBhdCBgbG9ja2Agd291bGQgbGV0IGEgYnV5ZXIKZXh0ZW5kIGl0IGJ5IHNpbXBseSBsb2NraW5nIGxhdGUuIFRoZSBjb21wbGVtZW50YXJ5IGd1YXJkIGlzIG9uIHRoZQpvdGhlciBzaWRlIG9mIHRoYXQgdHJhZGUtb2ZmICgjMjc3KTogYGxvY2tgIHJlZnVzZXMgb25jZQpgbm93ID4gZGVsaXZlcnlfZGVhZGxpbmVgIHdpdGgKW2BFcnJvcjo6TG9ja0FmdGVyRGVsaXZlcnlEZWFkbGluZWBdLCBzbyBhIGJ1eWVyIGNhbiBuZXZlciBsb2NrIGludG8KYSB3aW5kb3cgdGhhdCBoYXMgYWxyZWFkeSBjbG9zZWQgYW5kIGltbWVkaWF0ZWx5IGZvcmZlaXQgdGhlCmNvb3BlcmF0aXZlJ3MgcG9zaXRpb24gdmlhIGByZWNsYWltX29uX25vbmRlbGl2ZXJ5YC4AAAAAAAARZGVsaXZlcnlfZGVhZGxpbmUAAAAAAAAGAAABIU51bWJlciBvZiBsYXBzZWQgZGlzcHV0ZSB3aW5kb3dzIGFjY3VtdWxhdGVkIHNvIGZhci4gYGZsYWdfZGlzcHV0ZWAKcmVmdXNlcyBvbmNlIHRoaXMgcmVhY2hlcyBgTUFYX0RJU1BVVEVfRkxBR1NgLiBPbmx5CmBleHBpcmVfZGlzcHV0ZV93aW5kb3dgIGxhcHNhbHMgaW5jcmVtZW50IGl0OyBgcmVzb2x2ZV9kaXNwdXRlYAoodW5hbmltb3VzIGNvbnNlbnQpIGRvZXMgbm90IOKAlCBnb29kLWZhaXRoIHJlc29sdXRpb24gc2hvdWxkIG5vdApzaG9ydGVuIHRoZSB3aW5kb3cgZm9yIGZ1dHVyZSBkaXNwdXRlcy4AAAAAAAANZGlzcHV0ZV9jb3VudAAAAAAAAAQAAADeMCB3aGVuZXZlciBgc3RhdHVzYCBpc24ndCBgRGlzcHV0ZWRgLiBSZXVzZXMgYHJlbWFpbmRlcl93aW5kb3dfc2Vjc2AKYXMgdGhlIHdpbmRvdyBsZW5ndGgsIHNhbWUgcmVhc29uaW5nIGFzIGBmeF9zaG9ydGZhbGxfZGVhZGxpbmVgCmFib3ZlIOKAlCBzZWUgYGZsYWdfZGlzcHV0ZWAncyBkb2MgY29tbWVudCBmb3Igd2h5IGEgZGlzcHV0ZSBmcmVlemUKbmVlZHMgYSBib3VuZCBhdCBhbGwuAAAAAAAQZGlzcHV0ZV9kZWFkbGluZQAAAAYAAAIBU2V0IGJ5IGBmbGFnX2Rpc3B1dGVgLCB0aGUgc3RhdHVzIHRvIHJlc3RvcmUgb24gYHJlc29sdmVfZGlzcHV0ZWAKb3IgYGV4cGlyZV9kaXNwdXRlX3dpbmRvd2AuIGBTdGF0dXM6OkRyYWZ0YCB3aGVuZXZlciBgc3RhdHVzYAppc24ndCBgRGlzcHV0ZWRgIOKAlCBhIHNhZmUgInVuc2V0IiBzZW50aW5lbCwgbm90IGEgcmVhbCBwb3NzaWJsZQp2YWx1ZSwgc2luY2UgYGZsYWdfZGlzcHV0ZWAgbmV2ZXIgYWNjZXB0cyBgRHJhZnRgIGFzIGEgc3RhdHVzIHRvCmRpc3B1dGUgKmZyb20qLiAoTm90IGBPcHRpb248U3RhdHVzPmA6IHRoZSBgY29udHJhY3R0eXBlYCBtYWNybydzCmdlbmVyYXRlZCBgU2NWYWxgIGNvbnZlcnNpb24gZG9lc24ndCBzYXRpc2Z5IHRoZSBibGFua2V0IGBPcHRpb25gCmltcGwgZm9yIGEgdW5pdC12YXJpYW50LW9ubHkgZW51bSBsaWtlIGBTdGF0dXNgLCB1bmxpa2UgYSBzdHJ1Y3QKc3VjaCBhcyBgT3JhY2xlQ29uZmlnYCDigJQgY29uZmlybWVkIGJ5IHRyeWluZyBpdCBmaXJzdC4pAAAAAAAAEmRpc3B1dGVfcHJlX3N0YXR1cwAAAAAH0AAAAAZTdGF0dXMAAAAAAS9gb3JhY2xlX2NvbmZpZy5kZW5vbWluYXRlZF9hbW91bnRgLCBhZGp1c3RlZCBieSBgc2V0dGxlbWVudF9icHNgCmFuZCBjb252ZXJ0ZWQgdG8gdGhlIHNldHRsZW1lbnQgdG9rZW4gYXQgdGhlIGZyZXNoIHJhdGUKYHJlc29sdmVfZnhfc2hvcnRmYWxsYCByZWFkLiAwIHVudGlsIHJlc29sdmVkIChhbHNvIHdoYXQgaXQnZCBiZQpmb3IgYSB0b3RhbCB3cml0ZS1vZmYsIHdoaWNoIGlzIHdoeSBgZnhfcmVzb2x2ZWRgIGlzIHRoZSBhY3R1YWwKImhhcyB0aGlzIHJ1biIgc2lnbmFsLCBub3QgdGhpcyBmaWVsZCBiZWluZyBub256ZXJvKS4AAAAAEWZ4X2FkanVzdGVkX3RvdGFsAAAAAAAACwAAAiJTZXQgYnkgYHJlZnJlc2hfZnhfcXVvdGVfc3RhdHVzYCAoIzEpOiB0aGUgbW9zdCByZWNlbnQgb3JhY2xlLWZyZXNobmVzcwpwcm9iZSBmb3VuZCBhIHF1b3RlIG9sZGVyIHRoYW4gYG9yYWNsZV9jb25maWcubWF4X2FnZV9zZWNzYCwgaS5lLgpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIGlzIGN1cnJlbnRseSBibG9ja2VkIG9uIFJlZmxlY3RvciBwdWJsaXNoaW5nIGEKZnJlc2ggb25lLiBDbGVhcmVkIGJ5IHRoZSBuZXh0IHByb2JlIHRoYXQgZmluZHMgYSBmcmVzaCBxdW90ZSAoYW5kIGJ5CmEgc3VjY2Vzc2Z1bCBgcmVzb2x2ZV9meF9zaG9ydGZhbGxgLCBzbyBpdCBjYW4gbmV2ZXIgbGluZ2VyIG9uY2UgdGhlCm9uZS10aW1lIHJlc29sdXRpb24gaGFzIHJ1bikuICoqSW5mb3JtYXRpb25hbCBvbmx5Kiog4oCUIG5vIGd1YXJkIHJlYWRzCml0OyBzZWUgYHJlZnJlc2hfZnhfcXVvdGVfc3RhdHVzYCdzIGRvYyBjb21tZW50IGZvciB3aHkgdGhpcyBpcyBhCmZsYWcgYWxvbmdzaWRlIGBmeF9yZXNvbHZlZGAgcmF0aGVyIHRoYW4gYSBkaXN0aW5jdCBgU3RhdHVzYC4AAAAAAA5meF9xdW90ZV9zdGFsZQAAAAAAAQAAANtTZXQgYnkgYHJlc29sdmVfZnhfc2hvcnRmYWxsYCwgb25seSBtZWFuaW5nZnVsIHdoZW4gYG9yYWNsZV9jb25maWdgCmlzIGBTb21lYC4gYHNldHRsZWAgdXNlcyB0aGlzIGluc3RlYWQgb2YgcmUtZGVyaXZpbmcgZnJvbQpgdG90YWxfYW1vdW50YCB3aGVuZXZlciBhbiBvcmFjbGUgaXMgY29uZmlndXJlZCDigJQgc2VlCmByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAncyBkb2MgY29tbWVudC4AAAAAC2Z4X3Jlc29sdmVkAAAAAAEAAADCSG93IG11Y2ggbW9yZSB0aGUgYnV5ZXIgb3dlcyBiZXlvbmQgd2hhdCdzIGFscmVhZHkgZXNjcm93ZWQsIHBlcgpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgJ3MgY29tcGFyaXNvbi4gMCBtZWFucyBubyB0b3AtdXAgbmVlZGVkIOKAlApgc2V0dGxlYCBjYW4gcnVuIHdpdGhvdXQgd2FpdGluZyBvbiBgZnVuZF9meF9zaG9ydGZhbGxgIGF0IGFsbC4AAAAAABNmeF9zaG9ydGZhbGxfYW1vdW50AAAAAAsAAAD1MCBpZiBgZnhfc2hvcnRmYWxsX2Ftb3VudGAgaXMgMCAobm8gZGVhZGxpbmUgbmVlZGVkKS4gUmV1c2VzCmByZW1haW5kZXJfd2luZG93X3NlY3NgIGFzIHRoZSB3aW5kb3cgbGVuZ3RoIHJhdGhlciB0aGFuIGFkZGluZwp5ZXQgYW5vdGhlciBgaW5pdGlhbGl6ZWAgcGFyYW1ldGVyIGZvciBhIHNlY29uZCBkZWFkbGluZSBzZXJ2aW5nCnRoZSBzYW1lICJnaXZlIHRoZSBidXllciBhIGZhaXIgY2hhbmNlIHRvIGFjdCIgcHVycG9zZS4AAAAAAAAVZnhfc2hvcnRmYWxsX2RlYWRsaW5lAAAAAAAABgAAAAAAAAATZnhfc2hvcnRmYWxsX2Z1bmRlZAAAAAABAAAAXFNldCBieSBgY29uZmlybV9kZWxpdmVyeWAg4oCUIHdoaWNoIGBncmFkZV9wcmljZV9icHNgIGVudHJ5IHRoZQp3YXJlaG91c2Ugb3BlcmF0b3IgYXR0ZXN0ZWQuAAAAC2dyYWRlX2luZGV4AAAAAAQAAAHJUHJlLWFncmVlZCBncmFkZSAtPiBwcmljZS1tdWx0aXBsaWVyIHRhYmxlLCBpbiBiYXNpcyBwb2ludHMgb2YKYHRvdGFsX2Ftb3VudGAncyBpbXBsaWVkIHVuaXQgcHJpY2UgKDEwXzAwMCA9IGZ1bGwgcHJpY2UpLCBzZXQgYXQKYGluaXRpYWxpemVgIGFuZCBuZXZlciBjaGFuZ2VkIOKAlCAicHJlLWFncmVlZCwiIHBlciB0aGUgUFJELCBtZWFucwphZ3JlZWQgYmVmb3JlIGRlbGl2ZXJ5LCBub3QgZGVjaWRlZCBhdCBzZXR0bGVtZW50IHRpbWUuIE9yZGVyZWQgYnkKdGhlIGNhbGxlcjsgYGNvbmZpcm1fZGVsaXZlcnlgJ3MgYGdyYWRlX2luZGV4YCBzZWxlY3RzIGFuIGVudHJ5LgpFYWNoIGVudHJ5IG11c3QgYmUgPD0gMTBfMDAwIChhIGdyYWRlIGNhbid0IGJlIHdvcnRoIG1vcmUgdGhhbiB0aGUKZnVsbCBhZ3JlZWQgdW5pdCBwcmljZSkgYW5kIHRoZXJlIG11c3QgYmUgYXQgbGVhc3Qgb25lIGVudHJ5LgAAAAAAAA9ncmFkZV9wcmljZV9icHMAAAAD6gAAAAQAAAGaU2V0IGJ5IGByZWFkeV9mb3JfZGVsaXZlcnlgIHdoZW4gdGhlIHJlbWFpbmRlciBpcyBub24temVybyBhbmQgYnkKYGZ1bmRfcmVtYWluZGVyYCBvbmNlIHRoZSBidXllcidzIHNlY29uZCBkZXBvc2l0IGxhbmRzLiAqKkFsc28gc2V0IGJ5CmByZWFkeV9mb3JfZGVsaXZlcnlgIGl0c2VsZiB3aGVuIHRoZSByZW1haW5kZXIgaXMgemVybyoqICgjMjc1KTogYQpkZXBvc2l0IGNvdmVyaW5nIGFsbCBvZiBgdG90YWxfYW1vdW50YCAoYWR2YW5jZSBicHMgc3VtbWluZyB0byAxMCAwMDApCmxlYXZlcyBub3RoaW5nIHRvIGZ1bmQsIHNvIHRoZSB3aW5kb3cgb3BlbnMgYWxyZWFkeSBzYXRpc2ZpZWQgYW5kIHRoZQpidXllciBjYW4ndCBiZSBkZWZhdWx0ZWQgZm9yIHNraXBwaW5nIGEgY2FsbCB0aGF0IG1vdmVzIG5vIHRva2Vucy4AAAAAABJyZW1haW5kZXJfZGVhZGxpbmUAAAAAAAYAAAAAAAAAEHJlbWFpbmRlcl9mdW5kZWQAAAABAAAArEhvdyBsb25nIHRoZSBidXllciBoYXMgdG8gZnVuZCB0aGUgcmVtYWluZGVyIG9uY2UKYHJlYWR5X2Zvcl9kZWxpdmVyeWAgb3BlbnMgdGhlIHdpbmRvdy4gSW5kZXBlbmRlbnQgb2YKYGNsYWltX3dpbmRvd19zZWNzYCDigJQgdGhpcyBpcyBhIHBheW1lbnQgZGVhZGxpbmUsIG5vdCBhIGNsYWltIG9uZS4AAAAVcmVtYWluZGVyX3dpbmRvd19zZWNzAAAAAAAABgAAAV5TZXQgYnkgYGNvbmZpcm1fZGVsaXZlcnlgOiB0aGUgY29tYmluZWQgcXVhbnRpdHkgeCBncmFkZQptdWx0aXBsaWVyLCBpbiBiYXNpcyBwb2ludHMgb2YgYHRvdGFsX2Ftb3VudGAsIHRoYXQgYHNldHRsZWAgcGF5cwpvdXQgYWdhaW5zdC4gYG1pbihkZWxpdmVyZWRfcXVhbnRpdHksIGNvbnRyYWN0ZWRfcXVhbnRpdHkpIC8KY29udHJhY3RlZF9xdWFudGl0eSAqIGdyYWRlX3ByaWNlX2Jwc1tncmFkZV9pbmRleF1gIOKAlCBjYXBwZWQgYXQKMTBfMDAwIGJ5IGNvbnN0cnVjdGlvbiAob3Zlci1kZWxpdmVyeSBpc24ndCBwYWlkIGV4dHJhIGluIHYxOyBzZWUKYGNvbmZpcm1fZGVsaXZlcnlgJ3MgZG9jIGNvbW1lbnQpLgAAAAAADnNldHRsZW1lbnRfYnBzAAAAAAAEAAAAAAAAAAZzdGF0dXMAAAAAB9AAAAAGU3RhdHVzAAAAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAMdG90YWxfYW1vdW50AAAACwAAAAAAAAASd2FyZWhvdXNlX29wZXJhdG9yAAAAAAAT",
        "AAAAAQAAAQxBIGxpdmUgcmF0ZSByZWFkIGZyb20gdGhlIGNvbmZpZ3VyZWQgUmVmbGVjdG9yIG9yYWNsZSwgcmV0dXJuZWQgYnkKYG9yYWNsZV9yYXRlYC4gTWlycm9ycyBSZWZsZWN0b3IncyBvd24gYFByaWNlRGF0YWAgc2hhcGUgKHNlZQpgcmVmbGVjdG9yLnJzYCkgcmF0aGVyIHRoYW4gcmV1c2luZyB0aGF0IHR5cGUgZGlyZWN0bHksIHNpbmNlIHRoaXMgaXMKdGhpcyBjb250cmFjdCdzIG93biBwdWJsaWMgaW50ZXJmYWNlLCBub3QgYSByZS1leHBvcnQgb2YgUmVmbGVjdG9yJ3MuAAAAAAAAAApPcmFjbGVSYXRlAAAAAAACAAABXmBwcmljZSAvIDEwXmRlY2ltYWxzYCBpcyB0aGUgYWN0dWFsIHJhdGUsIHdoZXJlIGBkZWNpbWFsc2AgY29tZXMKZnJvbSB0aGUgb3JhY2xlIGNvbnRyYWN0J3Mgb3duIGBkZWNpbWFscygpYCAoMTQsIGZvciBSZWZsZWN0b3IncwpmaWF0IGV4Y2hhbmdlIHJhdGUgb3JhY2xlIGFzIG9mIHRoaXMgd3JpdGluZykg4oCUIG5vdCBkdXBsaWNhdGVkCmludG8gYE9yYWNsZUNvbmZpZ2AsIHNpbmNlIGl0J3MgYSBwcm9wZXJ0eSBvZiB0aGUgb3JhY2xlLCBub3Qgb2YKdGhpcyBjb21taXRtZW50LCBhbmQgY2FsbGVycyBjYW4gcmVhZCBpdCBkaXJlY3RseSBvZmYgdGhlIG9yYWNsZQpjb250cmFjdCBpZiB0aGV5IG5lZWQgaXQuAAAAAAAFcHJpY2UAAAAAAAALAAAAvlRoZSBsZWRnZXIgdGltZXN0YW1wIHRoZSBvcmFjbGUgcmVjb3JkZWQgdGhpcyBxdW90ZSBhdCDigJQgYWxyZWFkeQpjaGVja2VkIGFnYWluc3QgYG1heF9hZ2Vfc2Vjc2AgYnkgdGhlIHRpbWUgdGhpcyBpcyByZXR1cm5lZCwgYnV0CnN1cmZhY2VkIGFueXdheSBzbyBhIGNhbGxlciBjYW4gZGlzcGxheSAiYXMgb2YiIGZyZXNobmVzcy4AAAAAAAl0aW1lc3RhbXAAAAAAAAAG",
        "AAAABQAAAJRFbWl0dGVkIGJ5IGByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAgYWZ0ZXIgdGhlIEZYIHNob3J0ZmFsbCBhbW91bnQgaXMgY29tcHV0ZWQgKCMyMTApLgpgZGVhZGxpbmVgIGlzIDAgd2hlbiBgc2hvcnRmYWxsYCBpcyAwIChubyB0b3AtdXAgd2luZG93IG5lZWRlZCkuAAAAAAAAAApGeFJlc29sdmVkAAAAAAABAAAAC2Z4X3Jlc29sdmVkAAAAAAUAAAAAAAAABHJhdGUAAAALAAAAAAAAAAAAAAAOcmF0ZV90aW1lc3RhbXAAAAAAAAYAAAAAAAAAAAAAABFmeF9hZGp1c3RlZF90b3RhbAAAAAAAAAsAAAAAAAAAAAAAAAlzaG9ydGZhbGwAAAAAAAALAAAAAAAAAAAAAAAIZGVhZGxpbmUAAAAGAAAAAAAAAAI=",
        "AAAAAQAAAEhPbmUgYWR2YW5jZSB0cmFuY2hlJ3MgY2xhaW0gc3RhdGUuIGBkZWFkbGluZSA9PSAwYCBtZWFucyBub3Qgb3BlbmVkIHlldC4AAAAAAAAAC1RyYW5jaGVWaWV3AAAAAAMAAAAAAAAAB2NsYWltZWQAAAAAAQAAAAAAAAAIZGVhZGxpbmUAAAAGAAAAAAAAAAdleHBpcmVkAAAAAAE=",
        "AAAAAQAAAL5QUkQgwqc0LjIgb3B0aW9uIChiKSdzIEZYLXJlc29sdXRpb24gc3RhdGUuIEFsbCBmaWVsZHMgYXJlIHplcm8vZmFsc2UgZm9yIGEKY29tbWl0bWVudCBpbml0aWFsaXplZCB3aXRob3V0IGFuIGBvcmFjbGVfY29uZmlnYDsgYGNvbmZpZ3VyZWRgIHRlbGxzIHRoZQp0d28gYXBhcnQgd2l0aG91dCBhIHNlY29uZCBzdG9yYWdlIHJlYWQuAAAAAAAAAAAADENvbW1pdG1lbnRGeAAAAAcAAAAAAAAADmFkanVzdGVkX3RvdGFsAAAAAAALAAAAAAAAAApjb25maWd1cmVkAAAAAAABAAAAAAAAAAtxdW90ZV9zdGFsZQAAAAABAAAAAAAAAAhyZXNvbHZlZAAAAAEAAAAAAAAAEHNob3J0ZmFsbF9hbW91bnQAAAALAAAAAAAAABJzaG9ydGZhbGxfZGVhZGxpbmUAAAAAAAYAAAAAAAAAEHNob3J0ZmFsbF9mdW5kZWQAAAAB",
        "AAAAAQAAANVXaGljaCBSZWZsZWN0b3Igb3JhY2xlIGluc3RhbmNlIGFuZCBhc3NldCBzeW1ib2wgYG9yYWNsZV9yYXRlYCBhbmQKYHJlc29sdmVfZnhfc2hvcnRmYWxsYCByZWFkIGZyb20sIGhvdyBvbGQgYSBxdW90ZSB0aGV5J2xsIGFjY2VwdCwgYW5kCndoYXQgdGhlIGRlYWwgaXMgYWN0dWFsbHkgd29ydGggaW4gdGhhdCBjdXJyZW5jeS4gU2V0IG9uY2UsIGF0CmBpbml0aWFsaXplYC4AAAAAAAAAAAAADE9yYWNsZUNvbmZpZwAAAAQAAAQAVGhlIGRlYWwncyB0cnVlIHZhbHVlIGluIGBwcmljZV9hc3NldGAsIGluIHRoZSBzYW1lICJzbWFsbGVzdAp1bml0LCA3IGRlY2ltYWwgcGxhY2VzIiBjb252ZW50aW9uIGB0b3RhbF9hbW91bnRgIGFscmVhZHkgdXNlcwpmb3IgdGhlIHNldHRsZW1lbnQgdG9rZW4gKHNvIGUuZy4gMSBOR04gPT0gMTBfMDAwXzAwMCB1bml0cyBoZXJlLAptYXRjaGluZyBob3cgMSBYTE0gPT0gMTBfMDAwXzAwMCBzdHJvb3BzKSDigJQgKipub3QqKiB0aGUgc2FtZQpudW1iZXIgYXMgYHRvdGFsX2Ftb3VudGAsIHdoaWNoIGlzIGBkZW5vbWluYXRlZF9hbW91bnRgJ3MgdmFsdWUKKmF0IHdoYXRldmVyIHJhdGUgaGFwcGVuZWQgdG8gaG9sZCB3aGVuIHRoZSBkZWFsIHdhcyBmdW5kZWQqLgpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIHJlcHJpY2VzIHRoaXMgYXQgYSBmcmVzaCByYXRlOyB0aGUgZ2FwCmJldHdlZW4gdGhhdCBhbmQgd2hhdCdzIGFjdHVhbGx5IGVzY3Jvd2VkIGlzIFBSRCDCpzQuMidzIEZYIHJpc2ssCm1ha2UgY29uY3JldGUuIFRoaXMgZmllbGQgYWx3YXlzIHVzZXMgdGhlIDctZGVjaW1hbCBjb252ZW50aW9uOwpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIGNvbnZlcnRzIHRoZSByZXN1bHQgdG8gdGhlIHNldHRsZW1lbnQgdG9rZW4ncwpvd24gYGRlY2ltYWxzKClgICh3aGljaCBpcyBub3QgbmVjZXNzYXJpbHkgNyDigJQgc2VlICMyNTgpIGJlZm9yZQpjb21wYXJpbmcgaXQgd2l0aCB0aGUgZXNjcm93ZWQgYmFsYW5jZS4gU2VlIHRoYXQgZnVuY3Rpb24ncyBkb2MKY29tbWVudCBmb3IgdGhlIHdvcmtlZCBleGFtcGxlLgoKKipOb3QqKiB0aGUgb3JhY2xlJ3MgYGRlY2ltYWxzKClgIOKAlCB0aGF0IGlzIGEgcHJvcGVydHkgb2YgdGhlCm9yYWNsZSwgcmVhZCBsaXZlIGF0IHNldHRsZW1lbnQgYW5kIGFzc3VtZWQgaW1tdXRhYmxlIChzZWUKYGRvY3MvcmVmbGVjdG9yLWRlY2ltYWxzLWFzc3VtcHRpb24ubWRgLCBpc3N1ZSAjNjMpOyB0aGlzIGZpZWxkIGlzCnRoZSBkZWFsJ3MgdmFsdWUgaW4gdGhlIHNhbWUgNwAAABJkZW5vbWluYXRlZF9hbW91bnQAAAAAAAsAAAB1TWF4aW11bSBhZ2UsIGluIHNlY29uZHMsIGEgcXVvdGUgbWF5IGhhdmUgYW5kIHN0aWxsIGJlIGFjY2VwdGVkIOKAlApQUkQgwqcxNi4zJ3Mgb3JhY2xlIHN0YWxlbmVzcyBib3VuZC4gTXVzdCBiZSA+IDAuAAAAAAAADG1heF9hZ2Vfc2VjcwAAAAYAAACrVGhlIFJlZmxlY3RvciBvcmFjbGUgY29udHJhY3QgdG8gY2FsbCDigJQgZS5nLiB0aGUgdGVzdG5ldAoiRmlhdCBleGNoYW5nZSByYXRlcyIgaW5zdGFuY2UsIGBDQ1NTT0hUQkwzTEVXVUNCQkVCNU5KRkMyT0tGUkM3NE9XRUlKSVpMUkpCR0FBVTRWTVU1TlY0V2AKKHNlZSBgcmVmbGVjdG9yLnJzYCkuAAAAAA9vcmFjbGVfY29udHJhY3QAAAAAEwAAAMFUaGUgYXNzZXQgc3ltYm9sIHRvIHF1b3RlIGFnYWluc3QgdGhhdCBvcmFjbGUncyBgYmFzZSgpYCDigJQgZS5nLgpgTkdOYC4gQWx3YXlzIHF1ZXJpZWQgYXMgYFJlZmxlY3RvckFzc2V0OjpPdGhlcmAsIHNpbmNlIHRoaXMKY29udHJhY3QncyB1c2UgY2FzZSBpcyBmaWF0L0ZYIHN5bWJvbHMsIG5vdCBTdGVsbGFyLW5hdGl2ZSBhc3NldHMuAAAAAAAAC3ByaWNlX2Fzc2V0AAAAABE=",
        "AAAABQAAAFRFbWl0dGVkIGJ5IGByZXNvbHZlX2Rpc3B1dGVgIGFuZCBgZXhwaXJlX2Rpc3B1dGVfd2luZG93YCB3aGVuIGEgZGlzcHV0ZSBlbmRzICgjMjA4KS4AAAAAAAAADERpc3B1dGVFbmRlZAAAAAEAAAANZGlzcHV0ZV9lbmRlZAAAAAAAAAIAAAAAAAAAB291dGNvbWUAAAAH0AAAAA5EaXNwdXRlT3V0Y29tZQAAAAAAAQAAAAAAAAAIcmVzdG9yZWQAAAfQAAAABlN0YXR1cwAAAAAAAAAAAAI=",
        "AAAABQAABABQdWJsaXNoZWQgb24gZXZlcnkgcmVhbCBzdGF0dXMgdHJhbnNpdGlvbiDigJQgYGxvY2tgLCBib3RoIGFkdmFuY2UKcmVsZWFzZXMsIGNoZWNrcG9pbnQsIGRlbGl2ZXJ5IGNvbmZpcm1hdGlvbiwgc2V0dGxlbWVudCwgY2FuY2VsbGF0aW9uLApib3RoIGRlZmF1bHQgcGF0aHMsIGFuZCB0aGUgZGlzcHV0ZSBmcmVlemUvcmVzdW1lIHBhaXIuIEV2ZXJ5IGVtaXNzaW9uCnBvaW50IGlzIGBzZXRfc3RhdHVzYCAodGhlIG9ubHkgcGxhY2UgYHN0YXR1c2AgaXMgYXNzaWduZWQgYWZ0ZXIKYGluaXRpYWxpemVgKSwgc28gYSBmdXR1cmUgdHJhbnNpdGlvbiBnZXRzIGFuIGV2ZW50IGJ5IGNvbnN0cnVjdGlvbgpyYXRoZXIgdGhhbiBieSByZW1lbWJlcmluZyB0byBhZGQgb25lLgoKQm90aCBzdGF0dXNlcyByaWRlIGFzIHRvcGljcywgaW4gdGhlIHNoYXBlIFNFUC00MSdzIGB0cmFuc2ZlcmAgdXNlcwooaW5kZXhlZCBkaW1lbnNpb25zIGZpcnN0LCBwYXlsb2FkIHNlY29uZCk6IGFuIG9mZi1jaGFpbiBjb25zdW1lciBjYW4KdG9waWMtZmlsdGVyIGZvciBhIHNwZWNpZmljIHRyYW5zaXRpb24g4oCUIGUuZy4gZXZlcnl0aGluZyB0aGF0IGp1c3QKYmVjYW1lIGBEZWZhdWx0ZWRgIOKAlCBpbnN0ZWFkIG9mIGZldGNoaW5nIGV2ZXJ5IGV2ZW50IGFuZCBkZWNvZGluZyBkYXRhLgpgaW5pdGlhbGl6ZWAgZGVsaWJlcmF0ZWx5IGRvZXNuJ3QgZW1pdDogYSBjb21taXRtZW50IGJlaW5nIGNyZWF0ZWQgaW4KYERyYWZ0YCBpcyBhbiBpbml0aWFsIHN0YXRlLCBub3QgYSB0cmFuc2l0aW9uIGludG8gb25lLgoKQWRkZWQgZm9yICM2OSwgd2hpY2ggYXNrZWQgd2hldGhlciBhbiBpbmRleGVyIHdvdWxkIGJlbmVmaXQ6IG5vbmUKZXhpc3RzIHlldCwgYnV0IHRoZSBBUEkncyBjYWNoZSBpcyByZWFkLXRyaWdnZXJlZCBwcmVjaXNlbHkgYmVjYXVzZQp0aGVyZSBpcyBubyBjaGVhcGVyIHdheSB0byBsZWFybiBhYm91dCBhIHN0YXRlIGNoYW5nZSAoc2VlCmBhcGkvSEFORE9GRi5tZGApLCBhbmQgdGhpcyBpcyB0aGUgcHJpbWl0AAAAAAAAAA1TdGF0dXNDaGFuZ2VkAAAAAAAAAQAAAA5zdGF0dXNfY2hhbmdlZAAAAAAAAgAAAAAAAAAEZnJvbQAAB9AAAAAGU3RhdHVzAAAAAAABAAAAAAAAAAJ0bwAAAAAH0AAAAAZTdGF0dXMAAAAAAAEAAAAA",
        "AAAAAgAAAPBUaGUgdGltZXN0YW1wLCBpZiBhbnksIHRoYXQgYm91bmRzIGEgY2FsbGFibGUgYWN0aW9uLiBUaGUgdmFyaWFudCBzYXlzCndoaWNoIHNpZGUgb2YgdGhlIGJvdW5kYXJ5IHRoZSBhY3Rpb24gbGl2ZXMgb24sIHNvIGEgY2xpZW50IGNhbiB0ZWxsCiJjbGFpbSBjbG9zZXMgYXQgVCIgZnJvbSAiZXhwaXJ5IG9wZW5zIGp1c3QgYWZ0ZXIgVCIgd2l0aG91dCBrbm93aW5nCmVhY2ggZnVuY3Rpb24ncyBndWFyZCBieSBoZWFydC4AAAAAAAAADkFjdGlvbkRlYWRsaW5lAAAAAAADAAAAAAAAADNObyBkZWFkbGluZSBnYXRlcyB0aGlzIGFjdGlvbiBpbiB0aGUgY3VycmVudCBzdGF0ZS4AAAAABE5vbmUAAAABAAAALExlZ2FsIHdoaWxlIGBlbnYubGVkZ2VyKCkudGltZXN0YW1wKCkgPD0gVGAuAAAACUluY2x1c2l2ZQAAAAAAAAEAAAAGAAAAAQAAACpMZWdhbCBvbmNlIGBlbnYubGVkZ2VyKCkudGltZXN0YW1wKCkgPiBUYC4AAAAAAAlFeGNsdXNpdmUAAAAAAAABAAAABg==",
        "AAAAAQAABABBIHN0YWJsZSwgdmVyc2lvbmVkIHJlYWQgbW9kZWwgZm9yIGEgY29tbWl0bWVudCAoIzI1NikuCgpbYENvbW1pdG1lbnRgXSBpcyB0aGUgc3RvcmFnZSBzdHJ1Y3Q6IGV2ZXJ5IGZpZWxkIGV4aXN0cyBiZWNhdXNlIHNvbWUgZ3VhcmQKb3IgcGF5b3V0IG5lZWRzIGl0IG9uIGNoYWluLCBhbmQgaXRzIHNoYXBlIGlzIGZyZWUgdG8gY2hhbmdlIGZvciBzdG9yYWdlCnJlYXNvbnMuIE9mZi1jaGFpbiBjbGllbnRzIHJlYWQgdGhpcyB2aWV3IGluc3RlYWQsIHNvIGEgc3RvcmFnZS1sYXlvdXQKY2hhbmdlIGRvZXMgbm90IHJld3JpdGUgdGhlaXIgQUJJLgoKKipTdGFiaWxpdHkgcHJvbWlzZS4qKiBGaWVsZHMgYXJlIG9ubHkgYWRkZWQsIG9yIHRoZWlyIG1lYW5pbmcgY2hhbmdlZCwgaW4KYSByZWxlYXNlIHRoYXQgaW5jcmVtZW50cyBbYENPTU1JVE1FTlRfVklFV19TQ0hFTUFfVkVSU0lPTmBdOyByZWFkZXJzCnNob3VsZCByZWZ1c2UgYSBgc2NoZW1hX3ZlcnNpb25gIG5ld2VyIHRoYW4gdGhleSB1bmRlcnN0YW5kIGFuZCBtYXkgYXNzdW1lCnRoYXQgYSBgc2NoZW1hX3ZlcnNpb25gIHRoZXkgZG8gdW5kZXJzdGFuZCBrZWVwcyB0aGUgc2FtZSBmaWVsZCBtZWFuaW5ncy4KVGhlIHN0b3JhZ2Ugc3RydWN0IGNhbiBjaGFuZ2Ugd2l0aG91dCBhIHZlcnNpb24gYnVtcCDigJQgb25seSB0aGlzIHZpZXcncwpzaGFwZSBpcyBjb250cmFjdHVhbC4KClRoZSBncm91cGluZyBpcyBzZW1hbnRpYywgbm90IHN0b3JhZ2Ugb3JkZXI6IGBwYXJ0aWVzYCAod2hvKSwgYHRlcm1zYAooYWdyZWVkIGJlZm9yZSBmdW5kaW5nKSwgYHRyYW5jaGVzYCAoYWR2YW5jZSBjbGFpbSBzdGF0ZSksIGBmdW5kaW5nYAoodHdvLXBoYXNlIHBheW1lbnQpLCBgZGVsaXZlcnlgICh0aGUgb3BlcmF0b3IncyBhdHRlc3RhdGlvbiksIGBmeGAgKG9yYWNsZQpjb252ZXJzaW9uKSwgYW5kIGBkaXNwdXRlYCAodGhlIGJvdW5kZWQgZnJlZXplKS4gYHN0YXR1c2AgYW5kIGBjcmVhdGVkX2F0YApzdGF5IHRvcC1sZXZlbCBiZWNhdXNlIHRoZXkgZGVzAAAAAAAAAA5Db21taXRtZW50VmlldwAAAAAACgAAAAAAAAAKY3JlYXRlZF9hdAAAAAAABgAAAAAAAAAIZGVsaXZlcnkAAAfQAAAAEkNvbW1pdG1lbnREZWxpdmVyeQAAAAAAAAAAAAdkaXNwdXRlAAAAB9AAAAARQ29tbWl0bWVudERpc3B1dGUAAAAAAAAAAAAAB2Z1bmRpbmcAAAAH0AAAABFDb21taXRtZW50RnVuZGluZwAAAAAAAAAAAAACZngAAAAAB9AAAAAMQ29tbWl0bWVudEZ4AAAAAAAAAAdwYXJ0aWVzAAAAB9AAAAARQ29tbWl0bWVudFBhcnRpZXMAAAAAAAAAAAAADnNjaGVtYV92ZXJzaW9uAAAAAAAEAAAAAAAAAAZzdGF0dXMAAAAAB9AAAAAGU3RhdHVzAAAAAAAAAAAABXRlcm1zAAAAAAAH0AAAAA9Db21taXRtZW50VGVybXMAAAAAAAAAAAh0cmFuY2hlcwAAB9AAAAASQ29tbWl0bWVudFRyYW5jaGVzAAA=",
        "AAAAAgAAAE5XaGV0aGVyIGEgZGlzcHV0ZSBlbmRlZCBieSB1bmFuaW1vdXMgY29uc2VudCBvciBieSB0aGUgd2luZG93IGV4cGlyaW5nICgjMjA4KS4AAAAAAAAAAAAORGlzcHV0ZU91dGNvbWUAAAAAAAIAAAAAAAAAAAAAAAhSZXNvbHZlZAAAAAAAAAAAAAAAB0V4cGlyZWQA",
        "AAAABQAAAEtFbWl0dGVkIGJ5IGBmbGFnX2Rpc3B1dGVgIHdoZW4gYSBuYW1lZCBwYXJ0eSBmcmVlemVzIHRoZSBjb21taXRtZW50ICgjMjA4KS4AAAAAAAAAAA5EaXNwdXRlRmxhZ2dlZAAAAAAAAQAAAA9kaXNwdXRlX2ZsYWdnZWQAAAAAAwAAAAAAAAAHZmxhZ2dlcgAAAAATAAAAAQAAAAAAAAAKcHJlX3N0YXR1cwAAAAAH0AAAAAZTdGF0dXMAAAAAAAAAAAAAAAAACGRlYWRsaW5lAAAABgAAAAAAAAAC",
        "AAAABQAAAH9FbWl0dGVkIGJ5IGBjbGFpbV9hZHZhbmNlXzFgIC8gYGNsYWltX2FkdmFuY2VfMmAgYWZ0ZXIgYSB0cmFuY2hlIGlzCnRyYW5zZmVycmVkIHRvIHRoZSBjb29wZXJhdGl2ZS4gYHRyYW5jaGVgIGlzIDEgb3IgMiAoIzIwMikuAAAAAAAAAAAOVHJhbmNoZUNsYWltZWQAAAAAAAEAAAAPdHJhbmNoZV9jbGFpbWVkAAAAAAMAAAAAAAAAB3RyYW5jaGUAAAAABAAAAAEAAAAAAAAAAnRvAAAAAAATAAAAAAAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAAI=",
        "AAAAAQAABABUaGUgZGVhbCdzIGFncmVlZCB0ZXJtcyBhbmQgd2luZG93cywgZml4ZWQgYXQgYGluaXRpYWxpemVgLgoKKipUaGUgb25lIGRlZmluaXRpb24gb2YgInRoZSB0ZXJtcyIgKCMyNzApLioqIGBpbml0aWFsaXplYCB0YWtlcyB0aGlzCnN0cnVjdCBpbnN0ZWFkIG9mIGEgcnVuIG9mIGVpZ2h0IHBvc2l0aW9uYWwgYXJndW1lbnRzLCBhbmQKYGdldF9jb21taXRtZW50X3ZpZXdgIHJldHVybnMgdGhlIHNhbWUgc3RydWN0IGFzIGB0ZXJtc2Ag4oCUIHNvIHRoZQpnZW5lcmF0ZWQgYmluZGluZ3MsIHRoZSByZWFkIG1vZGVsLCBhbmQgYGRvY3MvMDAwMi1pbnRlcmZhY2Utc3BlYy5tZGAKYWxsIG5hbWUgdGhlIHNhbWUgZmllbGRzLiBUaGF0IGlzIHRoZSB3aG9sZSBwb2ludDogYGFkdmFuY2UxX2Jwc2AgYW5kCmBhZHZhbmNlMl9icHNgIGFyZSBhZGphY2VudCBgdTMyYHMsIGFuZCBgY2xhaW1fd2luZG93X3NlY3NgLApgcmVtYWluZGVyX3dpbmRvd19zZWNzYCBhbmQgYGRlbGl2ZXJ5X3dpbmRvd19zZWNzYCBhcmUgYWRqYWNlbnQgYHU2NGBzLApzbyBhIHBvc2l0aW9uYWwgc3dhcCB0eXBlLWNoZWNrZWQgYW5kIHNpbGVudGx5IHByb2R1Y2VkIGEgZGlmZmVyZW50CmRlYWwuIEEgbmFtZWQgZmllbGQgY2Fubm90IGJlIHN3YXBwZWQgc2lsZW50bHksIGFuZCBnZW5lcmF0ZWQgY2xpZW50cwpjYXJyeSB0aGUgbmFtZXMgdGhyb3VnaC4KCmBkZWxpdmVyeV93aW5kb3dfc2Vjc2AgaXMgdGhlICp3aW5kb3cgbGVuZ3RoKiwgbm90IHRoZSBhYnNvbHV0ZQpkZWFkbGluZTogYGluaXRpYWxpemVgIGNvbXB1dGVzIGBkZWxpdmVyeV9kZWFkbGluZSA9IGNyZWF0ZWRfYXQgKwpkZWxpdmVyeV93aW5kb3dfc2Vjc2Agb25jZSBhbmQgc3RvcmVzIHRoYXQgYWJzb2x1dGUgdmFsdWUgaW4KW2BDb21taXRtZW50YF0uIFRoZSB2aWV3IHJlY29uc3RydWN0cyB0aGUgd2luZG93IGFzCmBkZWxpdmVyeV9kZWFkbGluZSAtIGNyZWF0ZWRfYXRgLCB3aGljaCBpcyBleGFjdGx5IHRoZSB2YWx1ZSBwYXNzZWQKaGVyZSwgc28gdGhlIHJlYWQgAAAAAAAAAA9Db21taXRtZW50VGVybXMAAAAACAAAAGpCYXNpcyBwb2ludHMgb2YgYHRvdGFsX2Ftb3VudGAgcmVsZWFzZWQgYXQgbG9jay1pbi4gMC0xMF8wMDAsIGFuZApgYWR2YW5jZTFfYnBzICsgYWR2YW5jZTJfYnBzIDw9IDEwXzAwMGAuAAAAAAAMYWR2YW5jZTFfYnBzAAAABAAAAHlCYXNpcyBwb2ludHMgcmVsZWFzZWQgYXQgdGhlIG1pZC1zZWFzb24gY2hlY2twb2ludC4gVG9nZXRoZXIgd2l0aApgYWR2YW5jZTFfYnBzYCB0aGlzIGRlZmluZXMgdGhlIGRlcG9zaXQgYGxvY2tgIGVzY3Jvd3MuAAAAAAAADGFkdmFuY2UyX2JwcwAAAAQAAAA/SG93IGxvbmcgdGhlIGNvb3BlcmF0aXZlIGhhcyB0byBjbGFpbSBhbiBhZHZhbmNlIG9uY2UgaXQgb3BlbnMuAAAAABFjbGFpbV93aW5kb3dfc2VjcwAAAAAAAAYAAABDVGhlIHF1YW50aXR5IHRoZSBidXllciBpcyBjb250cmFjdGluZyBmb3IsIGluIGNhbGxlci1kZWZpbmVkIHVuaXRzLgAAAAATY29udHJhY3RlZF9xdWFudGl0eQAAAAAEAAAAUkhvdyBsb25nIGFmdGVyIGBjcmVhdGVkX2F0YCB0aGUgY29vcGVyYXRpdmUgaGFzIHRvIHJlYWNoCmBEZWxpdmVyZWRgLiBNdXN0IGJlID4gMC4AAAAAABRkZWxpdmVyeV93aW5kb3dfc2VjcwAAAAYAAACZUHJlLWFncmVlZCBncmFkZSAtPiBwcmljZSBtdWx0aXBsaWVyIHRhYmxlLCBpbiBiYXNpcyBwb2ludHMKKDEwXzAwMCA9IGZ1bGwgcHJpY2UpLiBOb24tZW1wdHksIGF0IG1vc3QgYE1BWF9HUkFERV9FTlRSSUVTYAplbnRyaWVzLCBldmVyeSBlbnRyeSA8PSAxMF8wMDAuAAAAAAAAD2dyYWRlX3ByaWNlX2JwcwAAAAPqAAAABAAAAFhIb3cgbG9uZyB0aGUgYnV5ZXIgaGFzIHRvIGZ1bmQgdGhlIHJlbWFpbmRlciBvbmNlCmByZWFkeV9mb3JfZGVsaXZlcnlgIG9wZW5zIHRoZSB3aW5kb3cuAAAAFXJlbWFpbmRlcl93aW5kb3dfc2VjcwAAAAAAAAYAAAAAAAAADHRvdGFsX2Ftb3VudAAAAAs=",
        "AAAABQAAAFZFbWl0dGVkIGJ5IGByZWFzc2lnbl9idXllcmAgYWZ0ZXIgdGhlIGJ1eWVyIHBvc2l0aW9uIGlzIG5vdmF0ZWQgdG8gYG5ld19idXllcmAgKCMyMDYpLgAAAAAAAAAAAA9CdXllclJlYXNzaWduZWQAAAAAAQAAABBidXllcl9yZWFzc2lnbmVkAAAAAgAAAAAAAAAIcHJldmlvdXMAAAATAAAAAQAAAAAAAAAJbmV3X2J1eWVyAAAAAAAAEwAAAAEAAAAC",
        "AAAABQAAAEpFbWl0dGVkIGJ5IGBmdW5kX3JlbWFpbmRlcmAgYWZ0ZXIgdGhlIGJ1eWVyJ3Mgc2Vjb25kIGRlcG9zaXQgbGFuZHMgKCMyMDIpLgAAAAAAAAAAAA9SZW1haW5kZXJGdW5kZWQAAAAAAQAAABByZW1haW5kZXJfZnVuZGVkAAAAAgAAAAAAAAAEZnJvbQAAABMAAAAAAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAAg==",
        "AAAAAQAAAmNPbmUgZmFybWVyIG1lbWJlcidzIHJlY29yZGVkIHNoYXJlIG9mIGEgY29tbWl0bWVudCwgY2FwdHVyZWQgYnkKYHNldF9hbGxvY2F0aW9uYC4gYG1lbWJlcl9oYXNoYCBpcyBhIHBlci1tZW1iZXIgc2FsdGVkIGhhc2gg4oCUIG5ldmVyIGEKYmFyZSBwaG9uZSBudW1iZXIgb3Igb3RoZXIgUElJIOKAlCBjb21wdXRlZCBvZmYtY2hhaW4gKEFQSSkgYXMKYEhNQUMtU0hBMjU2KHNhbHQsIHBob25lX251bWJlcilgIHdpdGggYSBmcmVzaCByYW5kb20gc2FsdCBwZXIKbWVtYmVyLiBUaGUgc2FsdCBhbmQgdGhlIHBob25lLW51bWJlciBtYXBwaW5nIGxpdmUgb25seSBpbiB0aGUgQVBJJ3MKUG9zdGdyZXMsIHdoaWNoIGlzIHdoYXQgbWFrZXMgdGhpcyBnZW51aW5lbHkgZXJhc2FibGUgcGVyIE5EUEEgcy4zNDoKZGVsZXRlIHRoZSBvZmYtY2hhaW4gcm93IGFuZCB0aGlzIGhhc2ggYmVjb21lcyBwZXJtYW5lbnRseQp1bmxpbmthYmxlIHRvIGEgcmVhbCBwZXJzb24sIHNpbmNlIHRoZSBzYWx0IGlzIGdvbmUgYW5kIGJydXRlLQpmb3JjaW5nIGEgcmFuZG9tIHNhbHQgaXMgaW5mZWFzaWJsZSByZWdhcmRsZXNzIG9mIGhvdyBzbWFsbCB0aGUKcGhvbmUtbnVtYmVyIGtleXNwYWNlIGlzLgAAAAAAAAAAEEFsbG9jYXRpb25NZW1iZXIAAAACAAAAAAAAAAttZW1iZXJfaGFzaAAAAAPuAAAAIAAAAL1CYXNpcyBwb2ludHMgb2YgdGhlIGNvbW1pdG1lbnQncyB0b3RhbCBwYXlvdXQgdGhpcyBtZW1iZXIgaXMKZW50aXRsZWQgdG8sIG9mZi1jaGFpbi4gUmVjb3JkLW9ubHkgaW4gdjEg4oCUIHNlZSBgc2V0X2FsbG9jYXRpb25gJ3MKZG9jIGNvbW1lbnQgZm9yIHdoeSBgc2V0dGxlYCBkb2Vzbid0IHByby1yYXRlIGFnYWluc3QgdGhpcy4AAAAAAAAJc2hhcmVfYnBzAAAAAAAABA==",
        "AAAABQAAAGxFbWl0dGVkIGJ5IGByZWNsYWltX2FkdmFuY2VfMWAgLyBgcmVjbGFpbV9hZHZhbmNlXzJgIGFmdGVyIGEgbGFwc2VkCnRyYW5jaGUgaXMgcmV0dXJuZWQgdG8gdGhlIGJ1eWVyICgjMjAyKS4AAAAAAAAAEFRyYW5jaGVSZWNsYWltZWQAAAABAAAAEXRyYW5jaGVfcmVjbGFpbWVkAAAAAAAAAwAAAAAAAAAHdHJhbmNoZQAAAAAEAAAAAQAAAAAAAAACdG8AAAAAABMAAAAAAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAAg==",
        "AAAAAQAAAEBEaXNwdXRlIGZyZWV6ZSBzdGF0ZTsgYHByZV9zdGF0dXNgIGlzIGBEcmFmdGAgd2hlbiBub3QgZGlzcHV0ZWQuAAAAAAAAABFDb21taXRtZW50RGlzcHV0ZQAAAAAAAAIAAAAAAAAACGRlYWRsaW5lAAAABgAAAAAAAAAKcHJlX3N0YXR1cwAAAAAH0AAAAAZTdGF0dXMAAA==",
        "AAAAAQAAAIlUd28tcGhhc2UgZnVuZGluZyBzdGF0ZTogdGhlIGRlcG9zaXQgaXMgZXNjcm93ZWQgYnkgYGxvY2tgLCB0aGUgcmVtYWluZGVyCmJ5IGBmdW5kX3JlbWFpbmRlcmAgb25jZSBgcmVhZHlfZm9yX2RlbGl2ZXJ5YCBvcGVucyBpdHMgd2luZG93LgAAAAAAAAAAAAARQ29tbWl0bWVudEZ1bmRpbmcAAAAAAAACAAAAAAAAABJyZW1haW5kZXJfZGVhZGxpbmUAAAAAAAYAAAAAAAAAEHJlbWFpbmRlcl9mdW5kZWQAAAAB",
        "AAAAAQAAADJUaGUgdGhyZWUgbmFtZWQgcGFydGllcyBwbHVzIHRoZSBzZXR0bGVtZW50IHRva2VuLgAAAAAAAAAAABFDb21taXRtZW50UGFydGllcwAAAAAAAAQAAAAAAAAABWJ1eWVyAAAAAAAAEwAAAAAAAAALY29vcGVyYXRpdmUAAAAAEwAAAAAAAAAFdG9rZW4AAAAAAAATAAAAAAAAABJ3YXJlaG91c2Vfb3BlcmF0b3IAAAAAABM=",
        "AAAABQAAAbdFbWl0dGVkIGJ5IGBjb25maXJtX2RlbGl2ZXJ5YCBhZnRlciB0aGUgb3BlcmF0b3IncyBhdHRlc3RhdGlvbiBpcyByZWNvcmRlZCAoIzIxMikuClRoZSB0aHJlZSBmaWVsZHMgdG9nZXRoZXIgYXJlIHRoZSB0cnVzdCBhbmNob3IgZm9yIGV2ZXJ5IGRvd25zdHJlYW0KY2FsY3VsYXRpb246IGBzZXR0bGVtZW50X2Jwc2AgaXMgZGVyaXZlZCBmcm9tIGBkZWxpdmVyZWRfcXVhbnRpdHlgIMOXCmBncmFkZV9wcmljZV9icHNbZ3JhZGVfaW5kZXhdYCAvIGBjb250cmFjdGVkX3F1YW50aXR5YCwgY29tcHV0ZWQgb25jZSBhbmQKbmV2ZXIgcmUtZGVyaXZlZC4gUGVyLW1lbWJlciBoYXNoZXMgc3RheSBvdXQgb2YgdGhpcyBldmVudCDigJQgc2VlCmBkb2NzL2FsbG9jYXRpb24tc2FsdC1hbmFseXNpcy5tZGAgZm9yIHdoeSBtZW1iZXIgaWRlbnRpdHkgYmVsb25ncwpvZmYtY2hhaW4uAAAAAAAAAAARRGVsaXZlcnlDb25maXJtZWQAAAAAAAABAAAAEmRlbGl2ZXJ5X2NvbmZpcm1lZAAAAAAAAwAAAAAAAAALZ3JhZGVfaW5kZXgAAAAABAAAAAEAAAAAAAAAEmRlbGl2ZXJlZF9xdWFudGl0eQAAAAAABAAAAAAAAAAAAAAADnNldHRsZW1lbnRfYnBzAAAAAAAEAAAAAAAAAAI=",
        "AAAABQAAAEhFbWl0dGVkIGJ5IGBmdW5kX2Z4X3Nob3J0ZmFsbGAgYWZ0ZXIgdGhlIGJ1eWVyJ3MgRlggdG9wLXVwIGxhbmRzICgjMjAyKS4AAAAAAAAAEUZ4U2hvcnRmYWxsRnVuZGVkAAAAAAAAAQAAABNmeF9zaG9ydGZhbGxfZnVuZGVkAAAAAAIAAAAAAAAABGZyb20AAAATAAAAAAAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAAI=",
        "AAAAAQAAAERUaGUgd2FyZWhvdXNlIG9wZXJhdG9yJ3MgZGVsaXZlcnkgYXR0ZXN0YXRpb24sIGNvbnN1bWVkIGJ5IGBzZXR0bGVgLgAAAAAAAAASQ29tbWl0bWVudERlbGl2ZXJ5AAAAAAADAAAAAAAAABJkZWxpdmVyZWRfcXVhbnRpdHkAAAAAAAQAAAAAAAAAC2dyYWRlX2luZGV4AAAAAAQAAAAAAAAADnNldHRsZW1lbnRfYnBzAAAAAAAE",
        "AAAAAQAAAAAAAAAAAAAAEkNvbW1pdG1lbnRUcmFuY2hlcwAAAAAAAgAAAAAAAAAIYWR2YW5jZTEAAAfQAAAAC1RyYW5jaGVWaWV3AAAAAAAAAAAIYWR2YW5jZTIAAAfQAAAAC1RyYW5jaGVWaWV3AA==",
        "AAAABQAAARBFbWl0dGVkIGJ5IGBzZXRfYWxsb2NhdGlvbmAgYWZ0ZXIgdGhlIG1lbWJlciBlbnRpdGxlbWVudCBsZWRnZXIgaXMKcmVjb3JkZWQgKCMyMTIpLiBSZXBvcnRzIHRoZSBhZ2dyZWdhdGUgc2hhcGUgKG1lbWJlciBjb3VudCBhbmQgdG90YWwKc2hhcmUgYmFzaXMgcG9pbnRzKSB3aXRob3V0IGV4cG9zaW5nIGluZGl2aWR1YWwgaGFzaGVzLCBzbyBhbiBpbmRleGVyCmNhbiB2ZXJpZnkgY29tcGxldGVuZXNzIHdpdGhvdXQgZGVjb2RpbmcgdGhlIGZ1bGwgYWxsb2NhdGlvbiBsaXN0LgAAAAAAAAASQWxsb2NhdGlvblJlY29yZGVkAAAAAAABAAAAE2FsbG9jYXRpb25fcmVjb3JkZWQAAAAAAgAAAAAAAAAMbWVtYmVyX2NvdW50AAAABAAAAAAAAAAAAAAAD3RvdGFsX3NoYXJlX2JwcwAAAAAEAAAAAAAAAAI=",
        "AAAABQAAAOtFbWl0dGVkIGJ5IGByZWZyZXNoX2Z4X3F1b3RlX3N0YXR1c2Agd2hlbiBgZnhfcXVvdGVfc3RhbGVgIGNoYW5nZXMgKCMyMTApLgpgc2luZ2xlLXZhbHVlYCBpcyB0aGUgbWVhc3VyZWQtY2hlYXBlc3QgZW5jb2RpbmcgZm9yIGEgb25lLWZpZWxkIGV2ZW50CihubyBmaWVsZC1uYW1lIGtleXMgdG8gY2FycnkpOyBzZWUgYGRvY3MvZ2FzLWNvc3RzLm1kYCDCpyJFdmVudCBlbWlzc2lvbiBieQpkYXRhX2Zvcm1hdCIuAAAAAAAAAAATRnhRdW90ZVN0YWxlQ2hhbmdlZAAAAAABAAAAFmZ4X3F1b3RlX3N0YWxlX2NoYW5nZWQAAAAAAAEAAAAAAAAABXN0YWxlAAAAAAAAAQAAAAAAAAAA",
        "AAAABQAAAQZFbWl0dGVkIGJ5IGByZXBsYWNlX29yYWNsZV9jb25maWdgIHdoZW4gYm90aCBwYXJ0aWVzIGNvbnNlbnQgdG8gc3dhcCB0aGUKb3JhY2xlIGFkZHJlc3MgKCMyMTUpLiBUaGUgb2xkIGNvbmZpZyBpcyBub3Qgc3RvcmVkIGFmdGVyIHJlcGxhY2VtZW50OwpzdWJzY3JpYmVycyB0aGF0IG5lZWQgaXQgY2FuIHJlYWQgaXQgZnJvbSB0aGlzIGV2ZW50J3MgY29udGV4dCwgdmlhIHRoZQpwcmV2aW91cyBgaW5pdGlhbGl6ZWAgdHJhbnNhY3Rpb24ncyBhcmd1bWVudHMuAAAAAAAAAAAAFE9yYWNsZUNvbmZpZ1JlcGxhY2VkAAAAAQAAABZvcmFjbGVfY29uZmlnX3JlcGxhY2VkAAAAAAABAAAAAAAAABNuZXdfb3JhY2xlX2NvbnRyYWN0AAAAABMAAAAAAAAAAg==",
        "AAAAAAAABABEcmFmdCAtPiBMb2NrZWQuIFB1bGxzIHRoZSBidXllcidzICoqZGVwb3NpdCoqIGludG8gdGhlIGNvbnRyYWN0IOKAlApgYWR2YW5jZTFfYnBzICsgYWR2YW5jZTJfYnBzYCBvZiBgdG90YWxfYW1vdW50YCwgbm90IHRoZSBmdWxsCmFtb3VudC4gU2VlIG1vZHVsZSBkb2NzIGZvciB3aHkgdGhpcyBjaGFuZ2VkIGZyb20gZWFybGllciB2ZXJzaW9ucwpvZiB0aGlzIGNvbnRyYWN0LCB3aGljaCBlc2Nyb3dlZCBldmVyeXRoaW5nIGhlcmUuCgpSZXF1aXJlcyBidXllciBhdXRoLgoKKipSZWZ1c2VkIG9uY2UgYGRlbGl2ZXJ5X2RlYWRsaW5lYCBoYXMgc3RyaWN0bHkgcGFzc2VkICgjMjc3KSoqOiB0aGUKZGVsaXZlcnkgd2luZG93IGNvdW50cyBmcm9tIGBpbml0aWFsaXplYCwgbm90IGZyb20gdGhpcyBjYWxsIChzZWUKYENvbW1pdG1lbnQ6OmRlbGl2ZXJ5X2RlYWRsaW5lYCBmb3Igd2h5KSwgc28gbG9ja2luZyBhZnRlciBpdCB3b3VsZApidXkgdGhlIGJ1eWVyIGEgc2FtZS1zZWNvbmQgYHJlY2xhaW1fb25fbm9uZGVsaXZlcnlgIGFuZCBhIGZyZWUKYEZvcmZlaXRlZGAgYWdhaW5zdCBhIGNvb3BlcmF0aXZlIHRoYXQgbmV2ZXIgaGFkIHRpbWUgdG8gZGVsaXZlci4KVGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgaXMgc3RpbGwgaW4gdGltZSDigJQgYG5vdyA+IGRlbGl2ZXJ5X2RlYWRsaW5lYAppcyB0aGUgb25seSByZWplY3Rpb24gKGBSZW1haW5kZXJXaW5kb3dQYXNzZWRgL2BDbGFpbVdpbmRvd1Bhc3NlZGAKdXNlIHRoZSBzYW1lIGluY2x1c2l2ZSBjdXQtb2ZmKSwgc28KYGxvY2tfZXhhY3RseV9hdF90aGVfZGVsaXZlcnlfZGVhZGxpbmVfc3RpbGxfc3VjY2VlZHNgIHBpbnMgdGhlCmJvdW5kYXJ5IGZyb20gYm90aCBzaWRlcy4gUmVqZWN0aW5nIGhlcmUgaXMgdGhlIGd1YXJkIGhhbGYgb2YgdGhlCmZpeDsgdGhlIGFsdGVybmF0aXZlIGNvbnNpZGVyZWQgYW5kIG5vdCB0YWtlbiB3YXMgcmUtYW5jaG9yaW5nCmBkZWxpdmVyeV9kZWFkbGluZWAgdG8gYG1heChkZWxpdmVyeV9kZWFkbGluZSwgbG9jAAAABGxvY2sAAAAAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAABABNdXR1YWwgY2FuY2VsbGF0aW9uIOKAlCBQUkQgwqc3OiAiRGVmaW5lZCB1bndpbmQ6IGFkdmFuY2Ugc2V0dGxlZCBwZXIKYWdyZWVkIHNjaGVkdWxlLCByZW1haW5pbmcgZXNjcm93IHJldHVybmVkLCBubyBwZW5hbHR5LCBsb2dnZWQuIgpBbGxvd2VkIGZyb20gYW55IHByZS1kZWxpdmVyeSBzdGF0ZSAoYERyYWZ0YCB0aHJvdWdoCmBSZWFkeUZvckRlbGl2ZXJ5YCkuIE5vdCBmcm9tIGBEZWxpdmVyZWRgIG9ud2FyZCDigJQgYXQgdGhhdCBwb2ludApgc2V0dGxlYCBpcyB0aGUgY29ycmVjdCBwYXRoLCBhbiB1bndpbmQgZG9lc24ndCBhcHBseSBhbnltb3JlLgoKUmVxdWlyZXMgKipib3RoKiogdGhlIGJ1eWVyJ3MgYW5kIHRoZSBjb29wZXJhdGl2ZSdzIGF1dGggaW4gdGhlIHNhbWUKY2FsbCwgc2luY2UgdGhpcyBpcyBtdXR1YWwsIG5vdCB1bmlsYXRlcmFsIOKAlCB1bmxpa2UgYHJlY2xhaW1fKmAsCndoaWNoIGlzIHRoZSBidXllcidzIHVuaWxhdGVyYWwgcmlnaHQgb25jZSBhIGNsYWltIHdpbmRvdyBsYXBzZXMgb24KaXRzIG93bi4KCiJBZHZhbmNlIHNldHRsZWQgcGVyIGFncmVlZCBzY2hlZHVsZSwgbm8gcGVuYWx0eSI6IHdoYXRldmVyJ3MKYWxyZWFkeSBiZWVuIGNsYWltZWQgc3RheXMgd2l0aCB0aGUgY29vcGVyYXRpdmUg4oCUIHRoaXMgZG9lc24ndCBjbGF3CmFueXRoaW5nIGJhY2suICJSZW1haW5pbmcgZXNjcm93IHJldHVybmVkIjogd2hhdGV2ZXIncyBzdGlsbCBpbiB0aGUKY29udHJhY3QgKHplcm8sIGlmIG5vdGhpbmcgd2FzIGV2ZXIgbG9ja2VkKSBnb2VzIGJhY2sgdG8gdGhlIGJ1eWVyLAp2aWEgdGhlIHNhbWUgYmFsYW5jZS1iYXNlZCB0cmFuc2ZlciBgc2V0dGxlYCB1c2VzLCBzbyBpdCdzIGNvcnJlY3QKcmVnYXJkbGVzcyBvZiBjbGFpbS9yZWNsYWltIGhpc3Rvcnkgb3IgaG93IG11Y2ggb2YgdGhlIGRlcG9zaXQgdnMuCnJlbWFpbmRlciBoYWQgYmVlbiBmdW5kZWQuICJMb2dnZWQiOiB0aGUgc3RhdGUgdHJhbnNpdGlvbiBhbmQgdGhlCnRyYW5zZmVyIGJvdGggbGFuZCBvbiB0AAAABmNhbmNlbAAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAABABEZWxpdmVyZWQgLT4gU2V0dGxlZC4gU3BsaXRzIHRoZSBjb250cmFjdCdzIHJlbWFpbmluZyB0b2tlbiBiYWxhbmNlCih0aGUgZnVuZGVkIHJlbWFpbmRlciDigJQgYWR2YW5jZXMgYWxyZWFkeSBsZWZ0IHRoZSBjb250cmFjdCB2aWEKYGNsYWltX3RyYW5jaGVgL2ByZWNsYWltX3RyYW5jaGVgIGJ5IHRoaXMgcG9pbnQpIGJldHdlZW4gdGhlCmNvb3BlcmF0aXZlIGFuZCBhIHNob3J0ZmFsbCByZWZ1bmQgdG8gdGhlIGJ1eWVyLCBwZXIKYGNvbmZpcm1fZGVsaXZlcnlgJ3MgYHNldHRsZW1lbnRfYnBzYC4KCiJBZHZhbmNlIG5vdCBjbGF3ZWQgYmFjayIgKFBSRCDCpzcsIHBhcnRpYWwgZGVsaXZlcnkpOiB3aGF0ZXZlciB0aGUKY29vcGVyYXRpdmUgYWxyZWFkeSBjbGFpbWVkIHZpYSBgY2xhaW1fYWR2YW5jZV8xYC9gY2xhaW1fYWR2YW5jZV8yYApzdGF5cyB0aGVpcnMgcmVnYXJkbGVzcyBvZiB0aGUgZmluYWwgYHNldHRsZW1lbnRfYnBzYCDigJQgdGhpcyBvbmx5CmFkanVzdHMgdGhlICpyZW1haW5kZXIqIHBheW1lbnQuIGBhZGp1c3RlZF90b3RhbCA9IHRvdGFsX2Ftb3VudCAqCnNldHRsZW1lbnRfYnBzIC8gMTBfMDAwYCBpcyB0aGUgZnVsbCBjb250cmFjdCB2YWx1ZSB0aGUgZGVsaXZlcnkKYWN0dWFsbHkgZWFybmVkOyBzdWJ0cmFjdGluZyB3aGF0IHRoZSBjb29wZXJhdGl2ZSBhbHJlYWR5IGNsYWltZWQKZ2l2ZXMgd2hhdCdzIHN0aWxsIG93ZWQgdG8gdGhlbSBmcm9tIHRoZSByZW1haW5kZXIgKGZsb29yZWQgYXQKemVybywgc2luY2UgY2xhaW1lZCBhZHZhbmNlcyBhcmUgbmV2ZXIgY2xhd2VkIGJhY2sgZXZlbiBpZiBhIHNldmVyZQpzaG9ydGZhbGwgbWVhbnMgdGhleSB0ZWNobmljYWxseSBvdmVyLWVhcm5lZCBhbHJlYWR5KS4gV2hhdGV2ZXIgb2YKdGhlIHJlbWFpbmRlciBiYWxhbmNlIGlzbid0IG93ZWQgZ29lcyBiYWNrIHRvIHRoZSBidXllciBhcyBhCnNob3J0ZmFsbCByZWZ1bmQuCgoqKlJlcXVpcmVzIGJvdGggYWR2YW5jZSB0cmFuY2hlcyBhbHJlYWR5IHJlc29sdmVkKiogKGVhY2ggAAAABnNldHRsZQAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAADhObyBhdXRoIHJlcXVpcmVkIOKAlCByZWFkLW9ubHkgdmlldywgY2FsbGFibGUgYnkgYW55b25lLgAAAApnZXRfc3RhdHVzAAAAAAAAAAAAAQAAA+kAAAfQAAAABlN0YXR1cwAAAAAAAw==",
        "AAAAAAAABABDcmVhdGVzIHRoZSBjb21taXRtZW50IGluIGBEcmFmdGAuIE5vIGZ1bmRzIG1vdmUgeWV0LgoKUmVxdWlyZXMgdGhlIGJ1eWVyJ3MgYXV0aCBiZWNhdXNlIHRoZXkncmUgdGhlIG9uZSBjb21taXR0aW5nIHRvIGEKZnV0dXJlIGRlcG9zaXQg4oCUIHRoZSBjb29wZXJhdGl2ZSBhbmQgd2FyZWhvdXNlIG9wZXJhdG9yIGRvbid0IG5lZWQKdG8gY28tc2lnbiBjb250cmFjdCBjcmVhdGlvbiBpdHNlbGYsIG9ubHkgdGhlIHN0YXRlIHRyYW5zaXRpb25zCnRoYXQgbWF0dGVyIHRvIHRoZW0gKGBtYXJrX2NoZWNrcG9pbnRgLCBgY29uZmlybV9kZWxpdmVyeWApLgoKVGhlIHRocmVlIHBhcnR5IGFkZHJlc3NlcyBhcmUgKm5vdCogcmVxdWlyZWQgdG8gYmUgZGlzdGluY3QsIG9uCnB1cnBvc2UuIFJvbGUgc2VwYXJhdGlvbiBpcyBlbmZvcmNlZCB3aGVyZSBpdCBtYXR0ZXJzIOKAlCBhdCBlYWNoCnN0YXRlIHRyYW5zaXRpb24sIGJ5IHRoYXQgcm9sZSdzIG93biBgcmVxdWlyZV9hdXRoKClgIOKAlCByYXRoZXIgdGhhbgpieSBhIGRpc3RpbmN0bmVzcyBjaGVjayBoZXJlLiBDb2xsYXBzaW5nIGJ1eWVyLCBjb29wZXJhdGl2ZSBhbmQKd2FyZWhvdXNlIG9udG8gb25lIHByaW5jaXBhbCBpcyBhIGxlZ2l0aW1hdGUgZGVwbG95bWVudCBjaG9pY2UgKGEKY29vcGVyYXRpdmUgdGhhdCBhbHNvIHJ1bnMgdGhlIHdhcmVob3VzZSwgb3IgYSBtaW5pbWFsIHNpbmdsZS1zaWduZXIKdmFsaWRhdGlvbiBkZXBsb3ltZW50KTsgYGluaXRpYWxpemVgIHJlY29yZHMgdGhlIHBhcnRpZXMgaXQgaXMgaGFuZGVkCmFuZCBkb2VzIG5vdCBzZWNvbmQtZ3Vlc3MgdGhlbS4gVGhlIGNvbnNlcXVlbmNlIGlzIGFjY2VwdGVkLCBub3QKaGlkZGVuOiB3aXRoIG9uZSBwcmluY2lwYWwgaW4gYWxsIHRocmVlIHJvbGVzIGV2ZXJ5IG11bHRpLXBhcnR5CmNvbnNlbnQgZ2F0ZSByZWR1Y2VzIHRvIHRoYXQgc2luZ2xlIHNpZ25lci4gU2VlIHRoZSB0ZXN0cwpgaW5pdGlhbGl6ZV9hbGxvd3NfYnV5ZXJfY29vcGVyYXRpdmVfYW5kX3dhcmVob3VzZV90AAAACmluaXRpYWxpemUAAAAAAAYAAAAAAAAABWJ1eWVyAAAAAAAAEwAAAAAAAAALY29vcGVyYXRpdmUAAAAAEwAAAAAAAAASd2FyZWhvdXNlX29wZXJhdG9yAAAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAABXRlcm1zAAAAAAAH0AAAAA9Db21taXRtZW50VGVybXMAAAAAAAAAAA1vcmFjbGVfY29uZmlnAAAAAAAD6AAAB9AAAAAMT3JhY2xlQ29uZmlnAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAAARFSZWFkcyB0aGUgY3VycmVudCByYXRlIGZvciB0aGlzIGNvbW1pdG1lbnQncyBjb25maWd1cmVkIG9yYWNsZQphc3NldCwgZW5mb3JjaW5nIHRoZSBzdGFsZW5lc3MgYm91bmQuIEEgcHVyZSByZWFkIGFnYWluc3QKUmVmbGVjdG9yJ3Mgb3duIHN0b3JlZCBoaXN0b3J5LCBub3QgYSBzdGF0ZSB0cmFuc2l0aW9uIOKAlCBjYWxsYWJsZQppbiBhbnkgc3RhdHVzLCBpbmNsdWRpbmcgYWZ0ZXIgYFNldHRsZWRgLgoKTm8gYXV0aCByZXF1aXJlZCDigJQgcGVybWlzc2lvbmxlc3MgcmVhZC4AAAAAAAALb3JhY2xlX3JhdGUAAAAAAAAAAAEAAAPpAAAH0AAAAApPcmFjbGVSYXRlAAAAAAAD",
        "AAAAAAAABABEaXNwdXRlIGZsYWdnaW5nIChQUkQncyBtdXN0LWhhdmUgImRpc3B1dGUgZmxhZ2dpbmcgd2l0aCBkZWZpbmVkCmVzY2FsYXRpb24iKS4gQW55ICoqb25lKiogb2YgdGhlIHRocmVlIG5hbWVkIHBhcnRpZXMg4oCUIGBmbGFnZ2VyYAptdXN0IGVxdWFsIGBidXllcmAsIGBjb29wZXJhdGl2ZWAsIG9yIGB3YXJlaG91c2Vfb3BlcmF0b3JgIOKAlCBjYW4KZnJlZXplIHRoZSBjb21taXRtZW50IGJ5IG1vdmluZyBpdCB0byBgU3RhdHVzOjpEaXNwdXRlZGAsIHdoaWNoCmJsb2NrcyBldmVyeSBvdGhlciBzdGF0ZS1jaGFuZ2luZyBjYWxsIGJ5IGNvbnN0cnVjdGlvbjogZXZlcnkgb25lCm9mIHRoZW0gbWF0Y2hlcyBhIHNwZWNpZmljIHJlcXVpcmVkIHN0YXR1cyAob3Igc2V0IG9mIHN0YXR1c2VzKQphbmQgYERpc3B1dGVkYCBpcyBuZXZlciBhbW9uZyB0aGVtLgoKVGhpcyBjb250cmFjdCBzdGlsbCBkb2Vzbid0IGFyYml0cmF0ZSAqd2hhdCogdGhlIGRpc3B1dGUgaXMgYWJvdXQK4oCUIHNlZSB0aGUgbW9kdWxlIGRvYydzIGV4cGxhbmF0aW9uIG9mIHdoeSB0aGF0J3MgYSBkZWxpYmVyYXRlCm5vbi1nb2FsLCBub3QgYW4gb3ZlcnNpZ2h0LiBXaGF0IHRoaXMgYnV5cyBpcyB0aGUgImZsYWdnaW5nIiBoYWxmCm9ubHk6IGEgdW5pbGF0ZXJhbCBwYXVzZSwgc28gYSBjb250ZXN0ZWQgc2l0dWF0aW9uIGRvZXNuJ3QgbGV0CnNvbWUgb3RoZXIgZGVhZGxpbmUtdHJpZ2dlcmVkIGZ1bmN0aW9uIChlLmcuIGBzZXR0bGVgLApgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgKSBydW4gdG8gYSBjb25jbHVzaW9uIHdoaWxlIGl0J3MgYmVpbmcKc29ydGVkIG91dCBvZmYtY2hhaW4uIFJlYWNoYWJsZSBmcm9tIGFueSBzdGF0ZSB3aXRoIGZ1bmRzIGFscmVhZHkKYXQgc3Rha2Ugb3IgYSBjbGFpbSBvdXRzdGFuZGluZyAoYExvY2tlZGAgdGhyb3VnaCBgRGVsaXZlcmVkYCkg4oCUCm5vdCBgRHJhZnRgIChub3RoaW5nIGVzY3Jvd2VkIHlldCB0byBmcmVlemUpIGFuZCBub3QgYW55IHRlcm1pbmFsCnN0YXR1cyBvciBgRGlzcHV0AAAADGZsYWdfZGlzcHV0ZQAAAAEAAAAAAAAAB2ZsYWdnZXIAAAAAEwAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAABABCdXllciBlc2Nyb3dzIGB0b3RhbF9hbW91bnQgLSBkZXBvc2l0YCDigJQgdGhlIHNlY29uZCBoYWxmIG9mCnR3by1waGFzZSBmdW5kaW5nLiBNdXN0IGhhcHBlbiB3aXRoaW4gdGhlIHdpbmRvdwpgcmVhZHlfZm9yX2RlbGl2ZXJ5YCBvcGVuZWQsIG9yIGl0J3MgYSBkZWZhdWx0IChzZWUKYGV4cGlyZV9yZW1haW5kZXJfd2luZG93YCkuIFJlcXVpcmVzIGJ1eWVyIGF1dGgg4oCUIHRoaXMgaXMgbmV3Cm1vbmV5IHRoZSBidXllciBzcGVjaWZpY2FsbHkgb3dlcy4KClRoZSB3aW5kb3cncyBlbmQgaXMgKippbmNsdXNpdmUgb2YgYHJlbWFpbmRlcl9kZWFkbGluZWAgaXRzZWxmKio6CmBub3cgPiByZW1haW5kZXJfZGVhZGxpbmVgIGlzIHRoZSBvbmx5IHJlamVjdGlvbiwgc28gYSBwYXltZW50CmxhbmRpbmcgaW4gdGhlIGRlYWRsaW5lIHNlY29uZCBzdGlsbCBjb3VudHMuCmBleHBpcmVfcmVtYWluZGVyX3dpbmRvd2AgbmVlZHMgc3RyaWN0bHkgbW9yZSB0aGFuIHRoZSBkZWFkbGluZSwKd2hpY2ggaXMgd2hhdCBrZWVwcyB0aGUgYm91bmRhcnkgZnJvbSBiZWluZyBhbWJpZ3VvdXMg4oCUIHRoZSB0d28KY2FsbHMgYXJlIG5ldmVyIGJvdGggbGVnYWwgYXQgdGhlIHNhbWUgdGltZXN0YW1wLgoKKipBIHplcm8gcmVtYWluZGVyIGlzIGFscmVhZHkgZnVuZGVkICgjMjc1KSoqOiB3aGVuCmBhZHZhbmNlMV9icHMgKyBhZHZhbmNlMl9icHMgPT0gMTBfMDAwYCwgYGxvY2tgIGhhcyBlc2Nyb3dlZCB0aGUKd2hvbGUgYHRvdGFsX2Ftb3VudGAsIGByZWFkeV9mb3JfZGVsaXZlcnlgIG1hcmtlZCB0aGUgcmVtYWluZGVyCmZ1bmRlZCBvbiB0aGUgYnV5ZXIncyBiZWhhbGYsIGFuZCB0aGlzIGNhbGwgcmVmdXNlcyB3aXRoCmBSZW1haW5kZXJBbHJlYWR5RnVuZGVkYCBpbnN0ZWFkIG9mIHRyYW5zZmVycmluZyB6ZXJvIHRva2Vucy4gVGhlCmRlYWwgY2FuIGdvIHN0cmFpZ2h0IHRvIGBjb25maXJtX2RlbGl2ZXJ5YCDigJQgbm8gbm8tb3AgY2FsbCBpcwpyZXF1aXJlZCwgYW5kIG5vbmUgY2FuIGJlIGhlbGQgYWdhaW5zAAAADmZ1bmRfcmVtYWluZGVyAAAAAAAAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAAAHZUaGUgcmVjb3JkZWQgYWxsb2NhdGlvbiBsZWRnZXIsIGlmIGBzZXRfYWxsb2NhdGlvbmAgaGFzIHJ1bi4KCk5vIGF1dGggcmVxdWlyZWQg4oCUIHJlYWQtb25seSB2aWV3LCBjYWxsYWJsZSBieSBhbnlvbmUuAAAAAAAOZ2V0X2FsbG9jYXRpb24AAAAAAAAAAAABAAAD6QAAA+oAAAfQAAAAEEFsbG9jYXRpb25NZW1iZXIAAAAD",
        "AAAAAAAAAUJObyBhdXRoIHJlcXVpcmVkIOKAlCByZWFkLW9ubHkgdmlldywgcmV0dXJucyB0aGUgZnVsbCBjb21taXRtZW50IHN0YXRlLgoKS2VwdCBmb3IgYmFja3dhcmQgY29tcGF0aWJpbGl0eTogZXhpc3RpbmcgY2xpZW50cyByZWFkIHRoaXMgc3RvcmFnZQpzdHJ1Y3QgZGlyZWN0bHksIGFuZCBpdCByZW1haW5zIHRoZSBjYW5vbmljYWwgb24tY2hhaW4gcmVjb3JkLiBOZXcKY2xpZW50cyBzaG91bGQgcHJlZmVyIFtgU2VsZjo6Z2V0X2NvbW1pdG1lbnRfdmlld2BdLCB3aG9zZSBzaGFwZSBpcwp2ZXJzaW9uZWQgaW5kZXBlbmRlbnRseSBvZiB0aGUgc3RvcmFnZSBsYXlvdXQuAAAAAAAOZ2V0X2NvbW1pdG1lbnQAAAAAAAAAAAABAAAD6QAAB9AAAAAKQ29tbWl0bWVudAAAAAAAAw==",
        "AAAAAAAABABCdXllci1wb3NpdGlvbiBhc3NpZ25hYmlsaXR5IChQUkQgwqc0LjgpOiB0cmFuc2ZlcnMgYGJ1eWVyYCB0bwpgbmV3X2J1eWVyYC4gRGVsaWJlcmF0ZWx5ICoqbm90KiogYSBtYXJrZXQg4oCUIHRoZXJlJ3Mgbm8gb3JkZXIgYm9vaywKbm8gbGlzdGluZywgbm8gb24tY2hhaW4gcHJpY2UgZGlzY292ZXJ5IGhlcmUsIGp1c3QgYSBub3ZhdGlvbiBvZgp3aG8gaG9sZHMgdGhlIHBvc2l0aW9uLiBObyBmdW5kcyBtb3ZlOyB0aGlzIG9ubHkgZXZlciByZXdyaXRlcyB3aG8KYGJ1eWVyYCByZWZlcnMgdG8gZm9yIGV2ZXJ5IGZ1dHVyZSBgcmVjbGFpbV8qYC9gY2FuY2VsYC8KYGZ1bmRfcmVtYWluZGVyYCBhdXRoIGNoZWNrLgoKUmVxdWlyZXMgKip0aHJlZSoqIHNpZ25hdHVyZXMgaW4gdGhlIHNhbWUgY2FsbCwgbm90IHR3bzogdGhlCmN1cnJlbnQgYnV5ZXIncyAodGhleSdyZSBnaXZpbmcgdXAgdGhlIHBvc2l0aW9uKSwgdGhlCmNvb3BlcmF0aXZlJ3MgKFBSRCdzIGV4cGxpY2l0ICJ3aXRoIGNvb3BlcmF0aXZlIGNvbnNlbnQiKSwgYW5kIHRoZQpuZXcgYnV5ZXIncy4gVGhhdCB0aGlyZCBvbmUgaXNuJ3QgbmFtZWQgaW4gdGhlIFBSRCBsaW5lIHRoaXMKaW1wbGVtZW50cywgYnV0IGxlYXZpbmcgaXQgb3V0IHdvdWxkIGxldCB0aGUgY3VycmVudCBidXllciBhbmQKY29vcGVyYXRpdmUgc2FkZGxlIGEgdGhpcmQgcGFydHkgd2l0aCBhIHBvc2l0aW9uIOKAlCBpbmNsdWRpbmcgaXRzCm9ibGlnYXRpb25zIOKAlCB3aXRob3V0IHRoYXQgcGFydHkgZXZlciBhZ3JlZWluZyB0byB0YWtlIGl0IG9uLiBUd28KY29uc2VudHMgd2hlcmUgdGhlIFBSRCBhc2tlZCBmb3IgdHdvIHdvdWxkIGJlIHRoZSBzbWFsbGVyIGNoYW5nZTsKdGhyZWUgaXMgdGhlIHNhZmVyIG9uZSwgYW5kIHNhZmV0eSB3aW5zIGhlcmUuCgpSZWFjaGFibGUgZnJvbSB0aGUgc2FtZSBzdGF0ZXMgYXMgYGNhbmNlbGAgKGBEcmFmdGAgdGhyb3VnaApgUmVhZHlGb3JEZWxpdmVyeWApIGFuZCBmb3IgdGhlIHNhbWUgcmVhc29uOiBwYXN0IGBEZWxpdmVyAAAADnJlYXNzaWduX2J1eWVyAAAAAAABAAAAAAAAAAluZXdfYnV5ZXIAAAAAAAATAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAABABDb29wZXJhdGl2ZS1hdXRob3JlZCByZWNvcmQgb2YgbWVtYmVyIGZhcm1lcnMnIGVudGl0bGVtZW50IHNoYXJlcwrigJQgUFJEJ3MgIm1lbWJlciBhbGxvY2F0aW9uIGxlZGdlciBjYXB0dXJlIGF0IGxvY2staW4uIiAqKlJlY29yZC0Kb25seSBpbiB2MSoqOiBgc2V0dGxlYCBzdGlsbCBwYXlzIHRoZSBjb29wZXJhdGl2ZSB3YWxsZXQgYSBsdW1wCnN1bTsgdGhpcyBkb2Vzbid0IHByby1yYXRlIGFuIG9uLWNoYWluIHBheW91dCBhY3Jvc3MgbWVtYmVycy4gVGhhdAptYXRjaGVzIFBSRCDCpzQuOSdzIG93biBzdGF0ZWQgdjEgZGVmYXVsdCAoVHJhbnNwYXJlbmN5IExhZGRlcgpSdW5nIDEg4oCUICJQYXltZW50IHNldHRsZXMgdG8gdGhlIGNvb3BlcmF0aXZlIHdhbGxldC4gRWFjaAptZW1iZXIncyBlbnRpdGxlbWVudCBpcyBvbiBjaGFpbiBhbmQgcmVhZGFibGU7IG1lbWJlcnMgcmVjZWl2ZSBhbgpTTVMgc3RhdGluZyB0aGVpciBzaGFyZSIpIOKAlCBub3QgYW4gb3BlbiBxdWVzdGlvbiB0aGlzIGNvbnRyYWN0CmRlZmVycmVkLCBidXQgdGhlIGRvY3VtZW50ZWQgZGVmYXVsdC4gUHJvLXJhdGVkIG9uLWNoYWluIHBheW91dAooUnVuZyAyKykgd291bGQgbmVlZCByZWFsIGRlc2lnbiB3b3JrIChOIHRva2VuIHRyYW5zZmVycyBpbnN0ZWFkCm9mIG9uZSwgcm91bmRpbmctcmVtYWluZGVyIGhhbmRsaW5nKSBhbmQgaXNuJ3QgYnVpbHQuCgpPbmUtdGltZSBhbmQgaW1tdXRhYmxlIG9uY2Ugc2V0IOKAlCBubyBhbWVuZCBmdW5jdGlvbiBleGlzdHMuCmBtZW1iZXJzYCBtdXN0IGJlIG5vbi1lbXB0eSwgbWF5IGhvbGQgYXQgbW9zdApgTUFYX0FMTE9DQVRJT05fTUVNQkVSU2AgZW50cmllcywgYW5kIHRoZSBzaGFyZXMgbXVzdCBzdW0gdG8gYXQKbW9zdCAxMF8wMDAgYnBzLgpEZWxpYmVyYXRlbHkgKipub3QqKiByZXF1aXJlZCBiZWZvcmUgYGxvY2tgOiBhIHNvbG8tZmFybWVyCmNvbW1pdG1lbnQgd2l0aCBubyBjb29wZXJhdGl2ZSBwb29saW5nIHNob3VsZG4ndCBiZSBmb3JjZWQgdGhyb3VnaAphIGxlZGdlAAAADnNldF9hbGxvY2F0aW9uAAAAAAABAAAAAAAAAAdtZW1iZXJzAAAAA+oAAAfQAAAAEEFsbG9jYXRpb25NZW1iZXIAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAVNDb29wZXJhdGl2ZSBjbGFpbXMgdHJhbmNoZSAxLCBpZiB3aXRoaW4gdGhlIHdpbmRvdy4gUmVxdWlyZXMKY29vcGVyYXRpdmUgYXV0aC4KCkJvdW5kYXJ5ICgjMTExKTogIndpdGhpbiIgaXMgaW5jbHVzaXZlIG9mIHRoZSBkZWFkbGluZSBpbnN0YW50IOKAlApgY2xhaW1fdHJhbmNoZWAgcmVqZWN0cyBvbmx5IHdoZW4gYHRpbWVzdGFtcCA+IGFkdmFuY2UxX2RlYWRsaW5lYCwgc28KYGFkdmFuY2UxX2RlYWRsaW5lYCBpdHNlbGYgaXMgc3RpbGwgY2xhaW1hYmxlLCBhbmQgaXQgaXMgdGhlIGxhc3QKaW5zdGFudCB0aGF0IGlzLiBQaW5uZWQgYnkgYHRlc3RzL2RlYWRsaW5lX2JvdW5kYXJ5LnJzYC4AAAAAD2NsYWltX2FkdmFuY2VfMQAAAAAAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAAALNDb29wZXJhdGl2ZSBjbGFpbXMgdHJhbmNoZSAyLCBpZiB3aXRoaW4gdGhlIHdpbmRvdy4gUmVxdWlyZXMKY29vcGVyYXRpdmUgYXV0aCDigJQgc2FtZSBtZWNoYW5pY3MgYXMgYGNsYWltX2FkdmFuY2VfMWAsIGluY2x1ZGluZyBpdHMKaW5jbHVzaXZlIGBhZHZhbmNlMl9kZWFkbGluZWAgYm91bmRhcnkgKCMxMTEpLgAAAAAPY2xhaW1fYWR2YW5jZV8yAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAATVBZHZhbmNlMVJlbGVhc2VkIC0+IENoZWNrcG9pbnRQYXNzZWQuIFJlcXVpcmVzIHRoZSB3YXJlaG91c2UKb3BlcmF0b3IncyBhdHRlc3RhdGlvbiDigJQgdGhpcyBpcyBhIGp1ZGdtZW50IGNhbGwsIG5vdCBhIG1lY2hhbmljYWwKc3RhdGUgYWR2YW5jZSwgc28gdW5saWtlIG9wZW5pbmcgYW4gYWR2YW5jZSB0cmFuY2hlIGl0ICppcyoKYXV0aC1nYXRlZC4gSW5kZXBlbmRlbnQgb2Ygd2hldGhlciB0cmFuY2hlIDEgaGFzIGFjdHVhbGx5IGJlZW4KY2xhaW1lZCB5ZXQg4oCUIGNyb3AgcHJvZ3Jlc3MgZG9lc24ndCB3YWl0IG9uIHBhcGVyd29yay4AAAAAAAAPbWFya19jaGVja3BvaW50AAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAA71SZXNvbHZlcyBhIGRpc3B1dGUgYnkgdW5hbmltb3VzIGNvbnNlbnQg4oCUICoqYWxsIHRocmVlKiogbmFtZWQKcGFydGllcycgYXV0aCwgbm90IGEgbWFqb3JpdHkgb3IgYW55IHNpbmdsZSBvbmUsIHNpbmNlIHJlc3VtaW5nCmlzIHRoZSBtaXJyb3Igb2YgYGZsYWdfZGlzcHV0ZWAncyB1bmlsYXRlcmFsIGZyZWV6ZSBhbmQgc2hvdWxkbid0Cml0c2VsZiBiZSB1bmlsYXRlcmFsLiBSZXN0b3JlcyBgc3RhdHVzYCB0byBleGFjdGx5IHdoYXRldmVyIGl0CndhcyB0aGUgbW9tZW50IGBmbGFnX2Rpc3B1dGVgIHJhbiDigJQgdGhpcyBjb250cmFjdCBkb2Vzbid0IGRlY2lkZSwKb3IgbGV0IHRoZSBwYXJ0aWVzIHJlZGlyZWN0IGl0IHRvLCBhICpkaWZmZXJlbnQqIG91dGNvbWUgKGUuZy4Kc3RyYWlnaHQgdG8gYENhbmNlbGxlZGApIGZyb20gaW5zaWRlIHRoaXMgY2FsbDsgdGhhdCB3b3VsZCBiZQphcmJpdHJhdGlvbiBieSBhbm90aGVyIG5hbWUuIElmIHRoZSBwYXJ0aWVzJyBvZmYtY2hhaW4gcmVzb2x1dGlvbgppcyAidW53aW5kIHRoZSBkZWFsLCIgdGhleSBjYWxsIHRoZSBleGlzdGluZyBgY2FuY2VsKClgIG5leHQsCnNhbWUgYXMgdGhleSB3b3VsZCBoYXZlIHdpdGhvdXQgZXZlciBkaXNwdXRpbmcuCgoqKkRlYWRsaW5lLWZyZWUgb24gcHVycG9zZSoqOiB1bmxpa2UgYGV4cGlyZV9kaXNwdXRlX3dpbmRvd2AsIHRoaXMKY2FycmllcyBubyBgZGlzcHV0ZV9kZWFkbGluZWAgY2hlY2sgYXQgYWxsLCBzbyBpdCBzdGF5cyBjYWxsYWJsZSBpbgp0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBhbmQgYWZ0ZXIgaXQuIFVuYW5pbW91cyBjb25zZW50IGlzIHRoZQpvbmx5IGdhdGUg4oCUIHRoZSB0aW1lb3V0IGV4aXN0cyB0byBicmVhayBhIGRlYWRsb2NrLCBuZXZlciB0byB0YWtlCnRoZSBkZWNpc2lvbiBhd2F5IGZyb20gdGhyZWUgcGFydGllcyB3aG8gaGF2ZSBhbHJlYWR5IGFncmVlZC4AAAAAAAAPcmVzb2x2ZV9kaXNwdXRlAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABSZWFkeUZvckRlbGl2ZXJ5IC0+IERlbGl2ZXJlZC4gV2FyZWhvdXNlLW9wZXJhdG9yLWF0dGVzdGVkLCBzYW1lCnJlYXNvbmluZyBhcyBgbWFya19jaGVja3BvaW50YC4gUmVxdWlyZXMgdGhlIHJlbWFpbmRlciB0byBhbHJlYWR5CmJlIGZ1bmRlZCDigJQgZGVsaXZlcnkgc2hvdWxkbid0IGJlIGNvbmZpcm1hYmxlIHdoaWxlIHRoZSBidXllcgpzdGlsbCBvd2VzIG1vbmV5IG9uIHRoZSBkZWFsLgoKQXBwbGllcyB0aGUgUFJEIMKnNyBzaG9ydGZhbGwvZ3JhZGUgYWRqdXN0bWVudCBzY2hlZHVsZTogYGRlbGl2ZXJlZF9xdWFudGl0eWAKaXMgY29tcGFyZWQgYWdhaW5zdCBgY29udHJhY3RlZF9xdWFudGl0eWAgKHByb3BvcnRpb25hbCwgY2FwcGVkIGF0CjEwMCUg4oCUIG92ZXItZGVsaXZlcnkgaXNuJ3QgcGFpZCBleHRyYSBpbiB2MSwgbWF0Y2hpbmcgImV4Y2VzcwpvdGhlcndpc2UgcmVsZWFzZWQgdG8gY29vcGVyYXRpdmUiIG9mZi1jaGFpbiksIGBncmFkZV9pbmRleGAgc2VsZWN0cwphbiBlbnRyeSBmcm9tIHRoZSBwcmUtYWdyZWVkIGBncmFkZV9wcmljZV9icHNgIHRhYmxlLCBhbmQgdGhlIHR3bwptdWx0aXBseSB0b2dldGhlciBpbnRvIGBzZXR0bGVtZW50X2Jwc2AsIHdoaWNoIGBzZXR0bGVgIHBheXMgb3V0CmFnYWluc3QuIFRoZSB3YXJlaG91c2Ugb3BlcmF0b3IncyBhdHRlc3RhdGlvbiBpcyBhdXRob3JpdGF0aXZlIGhlcmUK4oCUIGEgZ3JhZGUgb3IgcXVhbnRpdHkgZGlzcHV0ZSBpcyB0aGUgb3BlcmF0b3IncyBvd24gYXBwZWFscwpwcm9jZXNzLCBub3QgYSBIYXJ2ZXN0TG9jayBkaXNwdXRlIChQUkQncyBlZGdlLWNhc2UgdGFibGUpLgoKKipTdG9yYWdlIGFjY2VzcyBwYXR0ZXJuKio6ClJlYWRzIHRoZSBgQ29tbWl0bWVudGAgc3RydWN0IGV4YWN0bHkgb25jZSBmcm9tIGluc3RhbmNlIHN0b3JhZ2UgdXBvbgplbnRyeSB2aWEgYFNlbGY6OmxvYWRgLiBBbGwgdmFsaWRhdGlvbiBjaGVja3MsIHNob3J0ZmFsbCBzY2FsaW5nLCBhbmQKYHNldHRsZW1lbnRfYnBzYCBtdWx0aXBsaWVyIGNhAAAAEGNvbmZpcm1fZGVsaXZlcnkAAAACAAAAAAAAABJkZWxpdmVyZWRfcXVhbnRpdHkAAAAAAAQAAAAAAAAAC2dyYWRlX2luZGV4AAAAAAQAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABObyBhdXRoIHJlcXVpcmVkIOKAlCByZWFkLW9ubHkgdmlldywgY2FsbGFibGUgYnkgYW55b25lLgoKUmV0dXJucyBldmVyeSAqc3RhdGUtY2hhbmdpbmcqIGNhbGwgdGhlIGN1cnJlbnQgY29tbWl0bWVudCBwZXJtaXRzLAplYWNoIHdpdGggdGhlIGF1dGggaXQgbmVlZHMgYW5kIHRoZSB0aW1lc3RhbXAgdGhhdCBib3VuZHMgaXQgKGlmCmFueSkuIFRoaXMgaXMgdGhlIGNvbnRyYWN0J3MgYW5zd2VyIHRvIHRoZSBoYW5kLXdyaXR0ZW4gIndobyBjYW4KY2FsbCB3aGF0LCB3aGVuIiB0YWJsZSBpbiBgZG9jcy9ncmllZmluZy1hbmQtaW5hY3Rpb24ubWRgIMKnMTogYQpjbGllbnQgY2FuIHJlbmRlciBleGFjdGx5IHRoZSBsZWdhbCBidXR0b25zIGluc3RlYWQgb2YgcmVidWlsZGluZwp0aGUgc3RhdGUgbWFjaGluZS4KCkV2ZXJ5IGVudHJ5IGlzIHByb2R1Y2VkIGJ5IHRoZSBzYW1lIHByaXZhdGUgcHJlZGljYXRlIHRoZSByZWFsCmd1YXJkIGNhbGxzIChgY2FuX2xvY2tgLCBgY2FuX2NsYWltX3RyYW5jaGVgLCBgY2FuX3NldHRsZWAsIC4uLiksIHNvCnRoZSB2aWV3IGNhbm5vdCBkcmlmdCBmcm9tIHRoZSBjaGVja3MgdGhhdCBhY3R1YWxseSBhdXRob3JpemUgdGhlCmNhbGwuIEEgcHJlZGljYXRlIGlzIGBPa2Agb25seSB3aGVuIGV2ZXJ5IHN0YXRlLWRlcml2ZWQgZ3VhcmQgd291bGQKcGFzczsgYXJndW1lbnQtc3BlY2lmaWMgdmFsaWRhdGlvbiAoYSB2YWxpZCBgZ3JhZGVfaW5kZXhgLCBhCm5vbi1lbXB0eSBhbGxvY2F0aW9uIGxpc3QsIGEgYG5ld19idXllcmAgYWRkcmVzcywgYSBgZmxhZ2dlcmAgdGhhdAppcyBvbmUgb2YgdGhlIG5hbWVkIHBhcnRpZXMpIHJlbWFpbnMgYXQgdGhlIGNhbGwgc2l0ZSBiZWNhdXNlIGl0Cmlzbid0IGtub3dhYmxlIGZyb20gdGhlIGNvbW1pdG1lbnQuCgpUd28gZGVsaWJlcmF0ZSBleGNsdXNpb25zOiBgaW5pdGlhbGl6ZWAgKHRoZXJlIGlzIG5vIGNvbW1pdG1lbnQgdG8KZGVyaXZlIGFjdGlvbnMgZnJvbSBiZWZvcmUgaXQgcnVucyDigJQgdGhlIHZpZXcgcmV0dXJuAAAAEWF2YWlsYWJsZV9hY3Rpb25zAAAAAAAAAAAAAAEAAAPqAAAH0AAAAAZBY3Rpb24AAA==",
        "AAAAAAAAAXpCdXllciBwYXlzIGluIHRoZSBzaG9ydGZhbGwgYHJlc29sdmVfZnhfc2hvcnRmYWxsYCBjb21wdXRlZC4KQnV5ZXItZ2F0ZWQsIG1hdGNoaW5nIGBmdW5kX3JlbWFpbmRlcmAncyBvd24gcmVhc29uaW5nOiB0aGlzIGlzCm5ldyBtb25leSB0aGUgYnV5ZXIgc3BlY2lmaWNhbGx5IG93ZXMsIG5vdCBhIG1lY2hhbmljYWwgc3RlcAphbnlvbmUgY291bGQgdHJpZ2dlci4KCkRlYWRsaW5lLSoqaW5jbHVzaXZlKiosIGV4YWN0bHkgbGlrZSBgZnVuZF9yZW1haW5kZXJgOgpgbm93ID4gZnhfc2hvcnRmYWxsX2RlYWRsaW5lYCBpcyB0aGUgb25seSByZWplY3Rpb24sIHNvIHRoZQpkZWFkbGluZSBzZWNvbmQgaXRzZWxmIGlzIHN0aWxsIHRoZSBidXllcidzIHRvIHBheSBpbi4AAAAAABFmdW5kX2Z4X3Nob3J0ZmFsbAAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAGlUaGUgY29uZmlndXJlZCBvcmFjbGUsIGlmIGBpbml0aWFsaXplYCBzZXQgb25lLgoKTm8gYXV0aCByZXF1aXJlZCDigJQgcmVhZC1vbmx5IHZpZXcsIGNhbGxhYmxlIGJ5IGFueW9uZS4AAAAAAAARZ2V0X29yYWNsZV9jb25maWcAAAAAAAAAAAAAAQAAA+kAAAPoAAAH0AAAAAxPcmFjbGVDb25maWcAAAAD",
        "AAAAAAAAAZJCdXllciByZWNsYWltcyB0cmFuY2hlIDEsIGlmIHRoZSB3aW5kb3cgaGFzIHBhc3NlZCB1bmNsYWltZWQuClJlcXVpcmVzIGJ1eWVyIGF1dGguCgpCb3VuZGFyeSAoIzExMSk6IHRoZSBleGFjdCBjb21wbGVtZW50IG9mIGBjbGFpbV9hZHZhbmNlXzFgIOKAlApgcmVjbGFpbV90cmFuY2hlYCByZWplY3RzIHdoaWxlIGB0aW1lc3RhbXAgPD0gYWR2YW5jZTFfZGVhZGxpbmVgLCBzbwp0aGUgZGVhZGxpbmUgaW5zdGFudCBiZWxvbmdzIHRvIHRoZSBjb29wZXJhdGl2ZSdzIGNsYWltIGFuZCB0aGUgYnV5ZXIncwpyZWNsYWltIGZpcnN0IGJlY29tZXMgcG9zc2libGUgYXQgYGFkdmFuY2UxX2RlYWRsaW5lICsgMWAuIE5vIHRpbWVzdGFtcApldmVyIGFkbWl0cyBib3RoLCBhbmQgbm9uZSBhZG1pdHMgbmVpdGhlci4AAAAAABFyZWNsYWltX2FkdmFuY2VfMQAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAALlCdXllciByZWNsYWltcyB0cmFuY2hlIDIsIGlmIHRoZSB3aW5kb3cgaGFzIHBhc3NlZCB1bmNsYWltZWQuClJlcXVpcmVzIGJ1eWVyIGF1dGgg4oCUIHNhbWUgbWVjaGFuaWNzIGFzIGByZWNsYWltX2FkdmFuY2VfMWAsIGluY2x1ZGluZwp0aGUgZXhjbHVzaXZlIGBhZHZhbmNlMl9kZWFkbGluZWAgYm91bmRhcnkgKCMxMTEpLgAAAAAAABFyZWNsYWltX2FkdmFuY2VfMgAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABMb2NrZWQgLT4gQWR2YW5jZTFSZWxlYXNlZC4gT3BlbnMgdHJhbmNoZSAxJ3MgY2xhaW0gd2luZG93IOKAlCBkb2VzCioqbm90KiogbW92ZSBmdW5kcy4gVGhlIGNvb3BlcmF0aXZlIGNsYWltcyB2aWEgYGNsYWltX2FkdmFuY2VfMWAsCm9yIHRoZSBidXllciByZWNsYWltcyB2aWEgYHJlY2xhaW1fYWR2YW5jZV8xYCBhZnRlciB0aGUgd2luZG93CnBhc3NlcyB1bmNsYWltZWQuCgpEZWxpYmVyYXRlbHkgbm90IGF1dGgtZ2F0ZWQ6IGl0IHN0YXJ0cyBhIGNsb2NrLCBpdCBkb2Vzbid0IG1vdmUKbW9uZXkgb3IgZ3JhbnQgYW55b25lIGFueXRoaW5nIHRoZXkgd2VyZW4ndCBhbHJlYWR5IGVudGl0bGVkIHRvCnVuZGVyIHRoZSBhZ3JlZWQgdGVybXMuCgpTZWN1cml0eS1yZXZpZXdlZCBvbiB0aGUgcmVjb3JkICgjMik6IHRoYXQgcmVhc29uaW5nIGhvbGRzIHVuZGVyCmFkdmVyc2FyaWFsIHRpbWluZyDigJQgYSB0aGlyZCBwYXJ0eSBjYW4gcGljayB0aGUgaW5zdGFudCB0aGUgd2luZG93Cm9wZW5zIGJ1dCBnYWlucyBub3RoaW5nIGJ5IGl0IChubyBmdW5kcyBtb3ZlLCB0aGUgZGVhZGxpbmUgaXMKd3JpdHRlbiBleGFjdGx5IG9uY2Ugc28gdGhlIGNsb2NrIGNhbiBuZXZlciBiZSByZXN0YXJ0ZWQgb3IKZXh0ZW5kZWQsIHRoZSBsYXBzZSBiZW5lZmljaWFyeSBzdGF5cyB0aGUgYnV5ZXIsIGFuZCB0aGUKY29vcGVyYXRpdmUgYWx3YXlzIGdldHMgdGhlIGZ1bGwgYGNsYWltX3dpbmRvd19zZWNzYCBmcm9tIHdoZW5ldmVyCnRoZSB3aW5kb3cgb3BlbmVkKS4gVGhlIGZ1bGwgcmVhZGVyIGF1ZGl0IGFuZCB0aGUgcGlubmVkIHNjZW5hcmlvcwpsaXZlIGluIGBkb2NzL3Blcm1pc3Npb25sZXNzLXJlbGVhc2UtcmV2aWV3Lm1kYCBhbmQKYGNvbnRyYWN0cy9lc2Nyb3cvdGVzdHMvcGVybWlzc2lvbmxlc3NfcmVsZWFzZV9yZXZpZXcucnNgLgpBdXRoLWdhdGluZyB3b3VsZCBhZGQgYSB0aW1pbmcgdmV0byB0aGUgbm9uLWN1c3RvZGlhbCBkZXNpZ24KZGVsaWJlcmF0ZWx5IGdpdmVzIG5vYm9keSAoUFJEIMKnNC41AAAAEXJlbGVhc2VfYWR2YW5jZV8xAAAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAghDaGVja3BvaW50UGFzc2VkIC0+IEFkdmFuY2UyUmVsZWFzZWQuIFNhbWUgbWVjaGFuaWNzIGFzCmByZWxlYXNlX2FkdmFuY2VfMWAsIGZvciB0cmFuY2hlIDIuIFBlcm1pc3Npb25sZXNzIGxpa2UKYHJlbGVhc2VfYWR2YW5jZV8xYCDigJQgZGVsaWJlcmF0ZWx5IG5vdCBhdXRoLWdhdGVkLCBpdCBvbmx5IHN0YXJ0cwphIGNsb2NrLiBUaGUgIzIgc2VjdXJpdHkgcmV2aWV3IGNvdmVycyBib3RoIGNhbGxzIHRvZ2V0aGVyICh0aGV5CnNoYXJlIGBvcGVuX3RyYW5jaGVgKTsgc2VlIGByZWxlYXNlX2FkdmFuY2VfMWAncyBkb2MgY29tbWVudCBhbmQKYGRvY3MvcGVybWlzc2lvbmxlc3MtcmVsZWFzZS1yZXZpZXcubWRgLiBOb3RlIHRoYXQgdHJhbmNoZSAyJ3MKY2xvY2sgYWRkaXRpb25hbGx5IGNhbm5vdCBiZSBwdWxsZWQgZWFybHkgYXQgYWxsOiByZWFjaGluZwpgQ2hlY2twb2ludFBhc3NlZGAgcmVxdWlyZXMgdGhlIHdhcmVob3VzZSBvcGVyYXRvcidzIGF1dGgtZ2F0ZWQKYG1hcmtfY2hlY2twb2ludGAgZmlyc3QuAAAAEXJlbGVhc2VfYWR2YW5jZV8yAAAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAABABBZHZhbmNlMlJlbGVhc2VkIC0+IFJlYWR5Rm9yRGVsaXZlcnkuIFRoZSBjb29wZXJhdGl2ZSBzaWduYWxpbmcKInNldHRpbmcgb3V0IGZvciBkZWxpdmVyeSIg4oCUIG9wZW5zIHRoZSByZW1haW5kZXItcGF5bWVudCB3aW5kb3cKKGByZW1haW5kZXJfd2luZG93X3NlY3NgIGZyb20gbm93KS4gQXV0aC1nYXRlZCwgdW5saWtlCmByZWxlYXNlX2FkdmFuY2VfKmA6IHRoaXMgb25lIGhhcyByZWFsIHN0YWtlcyBmb3IgdGhlIGJ1eWVyIChpdApzdGFydHMgdGhlIGNsb2NrIHRoYXQgY2FuIGVuZCBpbiBgRGVmYXVsdGVkYCksIHNvIGl0IHNob3VsZG4ndCBiZQp0cmlnZ2VyYWJsZSBieSBqdXN0IGFueW9uZS4KCioqQSB6ZXJvIHJlbWFpbmRlciBpcyBmdW5kZWQgaGVyZSwgbm90IGJ5IGEgdG9rZW4gdHJhbnNmZXIgKCMyNzUpLioqCmBsb2NrYCBlc2Nyb3dzIGBhZHZhbmNlMV9icHMgKyBhZHZhbmNlMl9icHNgIG9mIGB0b3RhbF9hbW91bnRgIGFuZApgZnVuZF9yZW1haW5kZXJgIGVzY3Jvd3MgdGhlIHJlc3Q7IHdoZW4gYW4gYWR2YW5jZSBzcGxpdCBzdW1zIHRvCmV4YWN0bHkgMTAgMDAwIGJwcyB0aGUgZGVwb3NpdCAqaXMqIGB0b3RhbF9hbW91bnRgLCBzbyB0aGVyZSBpcyBubwpyZW1haW5kZXIgdG8gc2VuZC4gUmVxdWlyaW5nIGBmdW5kX3JlbWFpbmRlcmAgYW55d2F5IHdvdWxkIGxldCBhCmZ1bGx5LXByZXBhaWQgYnV5ZXIgYmUgbWFya2VkIGBEZWZhdWx0ZWRgIOKAlCBhbmQgbG9zZSB0aGUgd2hvbGUKZGVwb3NpdCB0byB0aGUgY29vcGVyYXRpdmUg4oCUIGJ5IG5vdCBtYWtpbmcgYSBjYWxsIHRoYXQgbW92ZXMgemVybwp0b2tlbnMgYmVmb3JlIGByZW1haW5kZXJfZGVhZGxpbmVgLiBTbyB3aGVuIHRoZSByZW1haW5kZXIgaXMgemVybywKdGhpcyBjYWxsIHNldHMgYHJlbWFpbmRlcl9mdW5kZWRgIGl0c2VsZiwgd2hpY2gga2VlcHMKYGV4cGlyZV9yZW1haW5kZXJfd2luZG93YCdzIGJ1eWVyLWRlZmF1bHQgKGBSZW1haW5kZXJBbHJlYWR5RnVuZGVkYCkKYW5kIGBjb25maXJtX2RlbGl2ZXJ5YCdzAAAAEnJlYWR5X2Zvcl9kZWxpdmVyeQAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAc5ObyBhdXRoIHJlcXVpcmVkIOKAlCB0aGUgdmVyc2lvbmVkIHJlYWQgbW9kZWwgKCMyNTYpLgoKU2FtZSBzdGF0ZSBhcyBgZ2V0X2NvbW1pdG1lbnRgLCByZXNoYXBlZCBpbnRvIFtgQ29tbWl0bWVudFZpZXdgXSB3aXRoCmFuIGV4cGxpY2l0IGBzY2hlbWFfdmVyc2lvbmAgYW5kIHNlbWFudGljIHN1Yi1zdHJ1Y3RzLCBzbyBhIGZpZWxkIGFkZGVkCnRvIGBDb21taXRtZW50YCBmb3Igc3RvcmFnZSByZWFzb25zIGRvZXMgbm90IGNoYW5nZSB0aGUgY2xpZW50LWZhY2luZwpBQkkuIGBmeC5jb25maWd1cmVkYCBpcyByZWFkIGZyb20gdGhlIHNhbWUgaW5zdGFuY2Utc3RvcmFnZSBlbnRyeSB0aGF0CmdhdGVzIHNldHRsZW1lbnQncyBGWCBwYXRoLCBzbyBhIGNhbGxlciBjYW4gZGlzdGluZ3Vpc2ggIm5vIG9yYWNsZSIKZnJvbSAib3JhY2xlIHJlc29sdmVkIHdpdGggemVyb3MiIHdpdGhvdXQgYSBzZWNvbmQgY2FsbC4AAAAAABNnZXRfY29tbWl0bWVudF92aWV3AAAAAAAAAAABAAAD6QAAB9AAAAAOQ29tbWl0bWVudFZpZXcAAAAAAAM=",
        "AAAAAAAABABQUkQgwqc0LjIgb3B0aW9uIChiKSwgdGhlIGFjdHVhbCBjb252ZXJzaW9uIHN0ZXA6IHJlYWRzIGEgZnJlc2gKb3JhY2xlIHJhdGUsIHJlcHJpY2VzIGBvcmFjbGVfY29uZmlnLmRlbm9taW5hdGVkX2Ftb3VudGAgKGFkanVzdGVkCmJ5IGBzZXR0bGVtZW50X2Jwc2AsIHNhbWUgc2hvcnRmYWxsL2dyYWRlIG1hdGggYHNldHRsZWAgaXRzZWxmCnVzZXMpIGludG8gdGhlIHNldHRsZW1lbnQgdG9rZW4sIGFuZCBjb21wYXJlcyB0aGF0IGFnYWluc3Qgd2hhdCdzCmFjdHVhbGx5IGVzY3Jvd2VkLiBXaGF0ZXZlciBnYXAgcmVtYWlucyDigJQgdGhlIGJ1eWVyJ3MgRlggcmlzaywKbWFkZSBjb25jcmV0ZSDigJQgYmVjb21lcyBgZnhfc2hvcnRmYWxsX2Ftb3VudGAgZm9yIGBmdW5kX2Z4X3Nob3J0ZmFsbGAKdG8gY29sbGVjdC4KCldvcmtlZCBleGFtcGxlLCB1c2luZyB0aGlzIHNlc3Npb24ncyBsaXZlLXRlc3RlZCBudW1iZXJzOiBhIGRlYWwKd29ydGggMSwwMDAsMDAwIE5HTiAoYGRlbm9taW5hdGVkX2Ftb3VudCA9IDEwXzAwMF8wMDBfMDAwXzAwMGAgYXQKdGhlIDctZGVjaW1hbCBjb252ZW50aW9uKSwgZnVsbHkgZGVsaXZlcmVkIGF0IGZ1bGwgZ3JhZGUKKGBzZXR0bGVtZW50X2JwcyA9IDEwXzAwMGApLCBSZWZsZWN0b3IgcXVvdGluZyBgcHJpY2UgPQo2Nl83MDBfMDAwXzAwMGAgYXQgYGRlY2ltYWxzID0gMTRgICjiiYgxIE5HTiA9IDAuMDAwNjY3IFVTRCwgaS5lLgriiYgxLDUwMCBOR04vVVNEKTogYG5nbl9vd2VkID0gMTBfMDAwXzAwMF8wMDBfMDAwYCAodW5jaGFuZ2VkLCBmdWxsCmJwcyksIGBmeF9hZGp1c3RlZF90b3RhbCA9IDEwXzAwMF8wMDBfMDAwXzAwMCAqIDY2XzcwMF8wMDBfMDAwIC8KMTBeMTQgPSA2XzY3MF8wMDBfMDAwYCDigJQgNjY3IFVTREMsIG1hdGNoaW5nIOKCpjEsMDAwLDAwMCBhdAriiYgxLDUwMCBOR04vVVNEIGJ5IGhhbmQuIElmIGB0b3RhbF9hbW91bnRgIGhhZCBiZWVuIGZ1bmRlZAphc3N1bWluZyBhIHNsaWdodGx5IGJldHRlciByYXRlLCBlc2Nyb3cgbWlnaHQgaG9sAAAAFHJlc29sdmVfZnhfc2hvcnRmYWxsAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAA85UaGUgZGlzcHV0ZS1zaWRlIGVzY2FwZSBoYXRjaCwgc2FtZSBzaGFwZSBhcwpgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgL2BleHBpcmVfZnhfc2hvcnRmYWxsX3dpbmRvd2A6CnBlcm1pc3Npb25sZXNzLCBkZWFkbGluZS1nYXRlZC4gSWYgdGhlIHRocmVlIHBhcnRpZXMgY2FuJ3QgcmVhY2gKdGhlIHVuYW5pbW91cyBjb25zZW50IGByZXNvbHZlX2Rpc3B1dGVgIHJlcXVpcmVzIGJlZm9yZQpgZGlzcHV0ZV9kZWFkbGluZWAsIHRoaXMgZG9lcyAqKm5vdCoqIGd1ZXNzIHdobyB3YXMgYXQgZmF1bHQg4oCUCmNvbnNpc3RlbnQgd2l0aCBgZmxhZ19kaXNwdXRlYCdzIGRvYyBjb21tZW50LCB0aGlzIGNvbnRyYWN0IHN0aWxsCmlzbid0IGFyYml0cmF0aW5nIGFueXRoaW5nLiBJdCBqdXN0IHJlc3RvcmVzIHRoZSBwcmUtZGlzcHV0ZQpzdGF0dXMsIGV4YWN0bHkgbGlrZSBgcmVzb2x2ZV9kaXNwdXRlYCB3b3VsZCwgc28gdGhlIGZyZWV6ZSBjYW4ndApiZWNvbWUgcGVybWFuZW50IGJ5IG9uZSBwYXJ0eSBzaW1wbHkgcmVmdXNpbmcgdG8gZXZlciBjb25zZW50LgpXaGF0ZXZlciBub3JtYWwgZGVhZGxpbmUtdHJpZ2dlcmVkIG1lY2hhbmlzbSB3b3VsZCBvdGhlcndpc2UgaGF2ZQphcHBsaWVkIChgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgLCBgcmVjbGFpbV9vbl9ub25kZWxpdmVyeWAsCmV0Yy4pIGlzIGZyZWUgdG8gcnVuIGFnYWluIGZyb20gdGhlcmUuCgpEZWFkbGluZS0qKmV4Y2x1c2l2ZSoqOiBgbm93ID4gZGlzcHV0ZV9kZWFkbGluZWAgaXMgcmVxdWlyZWQsIHNvIGluCnRoZSBkZWFkbGluZSBzZWNvbmQgaXRzZWxmIHRoaXMgc3RpbGwgcmVmdXNlcyB3aXRoCmBEaXNwdXRlV2luZG93Tm90UGFzc2VkYCB3aGlsZSBgcmVzb2x2ZV9kaXNwdXRlYCByZW1haW5zIGF2YWlsYWJsZSDigJQKdGhlIGJvdW5kYXJ5IHNlY29uZCBpcyBuZXZlciBhIGRlYWQgem9uZSB3aGVyZSBuZWl0aGVyIHJlc3VtZSBwYXRoCndvcmtzLgAAAAAAFWV4cGlyZV9kaXNwdXRlX3dpbmRvdwAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAaJSZXBsYWNlcyBhIGJyb2tlbiBvcmFjbGUgY29uZmlndXJhdGlvbiB3aXRoIGEgbmV3IG9uZSwgd2l0aCB0d28tcGFydHkKY29uc2VudCAoaXNzdWUgIzIxNSkuIEFkZHJlc3NlcyBGMSAodW5yZWFjaGFibGUgb3JhY2xlKSBieSBsZXR0aW5nIHRoZQpidXllciBhbmQgY29vcGVyYXRpdmUgc3Vic3RpdHV0ZSBhIHdvcmtpbmcgYG9yYWNsZV9jb25maWdgIGJlZm9yZQpgZnhfcmVzb2x2ZWRgIGlzIHNldC4KCk9ubHkgbGVnYWwgaW4gYFN0YXR1czo6RGVsaXZlcmVkYCB3aGlsZSBgZnhfcmVzb2x2ZWRgIGlzIGZhbHNlLgpUaGUgbmV3IGNvbmZpZyBpcyB2YWxpZGF0ZWQgdGhlIHNhbWUgd2F5IGBpbml0aWFsaXplYCBkb2VzCihgbWF4X2FnZV9zZWNzICE9IDBgKS4gUmVxdWlyZXMgYnV5ZXIgYW5kIGNvb3BlcmF0aXZlIHRvIGNvLXNpZ24uAAAAAAAVcmVwbGFjZV9vcmFjbGVfY29uZmlnAAAAAAAAAQAAAAAAAAAKbmV3X2NvbmZpZwAAAAAH0AAAAAxPcmFjbGVDb25maWcAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAUVQZXJtaXNzaW9ubGVzcyBmYWxsYmFjayB0aGF0IGZvcmZlaXRzIHRoZSBlc2Nyb3dlZCBmdW5kcyB0byB0aGUgYnV5ZXIKd2hlbiB0aGUgY29vcGVyYXRpdmUgbmV2ZXIgY2FsbGVkIGByZWNsYWltX29uX25vbmRlbGl2ZXJ5YCBhbmQgdGhlCmJ1eWVyJ3Mga2V5IGlzIGxvc3Qgb3IgaW5hY2Nlc3NpYmxlLiBDYWxsYWJsZSBieSBhbnlvbmUgYWZ0ZXIKYGRlbGl2ZXJ5X2RlYWRsaW5lICsgcmVtYWluZGVyX3dpbmRvd19zZWNzYCAodGhlIGdyYWNlIHBlcmlvZCkuCldpdGhvdXQgdGhpcyBoYXRjaCwgYSBsb3N0IGJ1eWVyIGtleSB3b3VsZCBsb2NrIGZ1bmRzIGZvcmV2ZXIuAAAAAAAAFmV4cGlyZV9kZWxpdmVyeV93aW5kb3cAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAA3dUaGUgc2VsbGVyLW5vbi1kZWxpdmVyeSBwYXRoOiBgZGVsaXZlcnlfZGVhZGxpbmVgIGhhcyAqc3RyaWN0bHkqCnBhc3NlZCAoYG5vdyA+IGRlbGl2ZXJ5X2RlYWRsaW5lYDsgdGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgc3RpbGwKcmVmdXNlcyB3aXRoIGBEZWxpdmVyeURlYWRsaW5lTm90UGFzc2VkYCkgd2l0aG91dCBgY29uZmlybV9kZWxpdmVyeWAKZXZlciBoYXZpbmcgcnVuLiBSZXR1cm5zIHdoYXRldmVyJ3MgY3VycmVudGx5IGVzY3Jvd2VkIHRvIHRoZSBidXllcgphbmQgc2V0cyBgRm9yZmVpdGVkYC4KCkJ1eWVyLWdhdGVkLCBub3QgcGVybWlzc2lvbmxlc3Mg4oCUIHVubGlrZSBgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgLAp0aGUgYmVuZWZpY2lhcnkgaGVyZSBpcyBhIHNwZWNpZmljIHBhcnR5ICh0aGUgYnV5ZXIpIHJlY2xhaW1pbmcKd2hhdCdzIHJpZ2h0ZnVsbHkgdGhlaXJzLCB0aGUgc2FtZSByZWFzb25pbmcgYHJlY2xhaW1fdHJhbmNoZWAgdXNlcy4KCk5vdCByZWFjaGFibGUgZnJvbSBgRHJhZnRgOiBub3RoaW5nJ3MgZXZlciBiZWVuIGVzY3Jvd2VkIGF0IHRoYXQKcG9pbnQsIHNvIHRoZXJlJ3Mgbm90aGluZyB0byByZWNsYWltIGFuZCBubyByZWFsIGV2ZW50IGhhcwpvY2N1cnJlZCB3b3J0aCByZWNvcmRpbmcgYXMgYSBmb3JmZWl0dXJlLiBTaW5jZSBgbG9ja2AgcmVmdXNlcyBvbmNlCmBkZWxpdmVyeV9kZWFkbGluZWAgaGFzIHBhc3NlZCAoIzI3NyksIHRoZXJlIGlzIGFsc28gbm8gd2F5IHRvIHJlYWNoCmBMb2NrZWRgIHdpdGggdGhlIGRlbGl2ZXJ5IHdpbmRvdyBhbHJlYWR5IGNsb3NlZCwgc28gdGhpcyBzd2VlcCBjYW4Kb25seSBldmVyIGZpcmUgYWdhaW5zdCBhIHdpbmRvdyB0aGUgY29vcGVyYXRpdmUgYWN0dWFsbHkgaGFkLgAAAAAWcmVjbGFpbV9vbl9ub25kZWxpdmVyeQAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAABABUaGUgYnV5ZXItZGVmYXVsdCBwYXRoOiBgcmVtYWluZGVyX2RlYWRsaW5lYCBoYXMgKnN0cmljdGx5KiBwYXNzZWQKKGBub3cgPiByZW1haW5kZXJfZGVhZGxpbmVgKSB3aXRoIHRoZSByZW1haW5kZXIgbmV2ZXIgZnVuZGVkIOKAlCBpbgp0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiB0aGlzIHJlZnVzZXMgd2l0aApgUmVtYWluZGVyV2luZG93Tm90UGFzc2VkYCwgc28gdGhlIGJ1eWVyJ3Mgb3duIGZ1bmRpbmcgd2luZG93IGlzCm5ldmVyIGN1dCBzaG9ydCBieSBhIHNlY29uZC4gU3dlZXBzIHdoYXRldmVyJ3MgY3VycmVudGx5IGVzY3Jvd2VkCih0aGUgZGVwb3NpdCwgb3Igd2hhdGV2ZXIgb2YgaXQgdGhlIGNvb3BlcmF0aXZlIGhhc24ndCBhbHJlYWR5CmNsYWltZWQpIHRvIHRoZSBjb29wZXJhdGl2ZSBhbmQgc2V0cyBgRGVmYXVsdGVkYCDigJQgdW5jb250ZXN0ZWQgYnkKY29uc3RydWN0aW9uLCBzbyB1bmxpa2UgYGNhbmNlbGAvYHJlYXNzaWduX2J1eWVyYCB0aGlzIG5lZWRzIG5vCmNvbnNlbnQgZnJvbSBhbnlvbmUuCgpQZXJtaXNzaW9ubGVzcywgZGVsaWJlcmF0ZWx5OiB0aGUgb3V0Y29tZSAoc3dlZXAgdG8gY29vcGVyYXRpdmUpCmRvZXNuJ3QgZGVwZW5kIG9uIHdobyBjYWxscyBpdCwgb25seSBvbiB3aGV0aGVyIHRoZSBkZWFkbGluZSBoYXMKcGFzc2VkLCB0aGUgc2FtZSByZWFzb25pbmcgYHJlY2xhaW1fdHJhbmNoZWAgd291bGQgdXNlIGlmIGl0CndlcmVuJ3QgYWxyZWFkeSBzY29wZWQgdG8gYmVuZWZpdCB0aGUgYnV5ZXIgc3BlY2lmaWNhbGx5LiBBbnlvbmUg4oCUCmluY2x1ZGluZyBhbiBvZmYtY2hhaW4gd2F0Y2hlciDigJQgY2FuIHRyaWdnZXIgaXQgb25jZSB0aGUgZmFjdCBvZgp0aGUgbWF0dGVyIChkZWFkbGluZSBwYXNzZWQsIHVuZnVuZGVkKSBpcyB0cnVlLgoKKipVbnJlYWNoYWJsZSBmb3IgYSBmdWxseS1wcmVwYWlkIGNvbW1pdG1lbnQgKCMyNzUpKio6IHdoZW4gdGhlCmFkdmFuY2UgYnBzIHN1bSB0byAxMCAwMDAgdGhlcmUgaXMgbm8gcmVtYWluZGVyLCBgcmVhZHlfAAAAF2V4cGlyZV9yZW1haW5kZXJfd2luZG93AAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABSZWNvcmRzIHdoZXRoZXIgc2V0dGxlbWVudCdzIEZYIHJlc29sdXRpb24gaXMgY3VycmVudGx5IGJsb2NrZWQKb24gYSBzdGFsZSBvcmFjbGUgcXVvdGUgKCMxKSDigJQgdGhlICJ3YWl0aW5nIG9uIGEgZnJlc2ggcXVvdGUiCnNpZ25hbCwgbWFkZSByZWFkYWJsZSBpbnN0ZWFkIG9mIGluZmVyYWJsZSBvbmx5IGZyb20gYSBmYWlsZWQKYHJlc29sdmVfZnhfc2hvcnRmYWxsYCBjYWxsLgoKV2h5IHRoaXMgZXhpc3RzIGFzIGl0cyBvd24gY2FsbDogU29yb2JhbiByb2xscyBiYWNrIGV2ZXJ5IHN0YXRlCndyaXRlIGFuIGludm9jYXRpb24gbWFrZXMgd2hlbiBpdCByZXR1cm5zIGBFcnJgLCBzbwpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgJ3MgYE9yYWNsZVN0YWxlYCByZWZ1c2FsIGNhbm5vdCBwZXJzaXN0IGFueQptYXJrZXIgb2YgaXRzIG93biDigJQgYnkgdGhlIHRpbWUgYSBjYWxsZXIgc2VlcyB0aGF0IGVycm9yLCBub3RoaW5nCndhcyByZWNvcmRlZC4gVGhpcyBwcm9iZSBpcyB0aGUgd3JpdGUgdGhhdCByZWZ1c2FsIGNhbid0IGJlOiBpdApydW5zIHRoZSBzYW1lIHN0YWxlbmVzcyBjaGVjayBhbmQgcmVjb3JkcyB0aGUgYW5zd2VyIGluCmBDb21taXRtZW50OjpmeF9xdW90ZV9zdGFsZWAsIHdoaWNoIGBnZXRfY29tbWl0bWVudGAgc3VyZmFjZXMgdG8KdGhlIEFQSS9pbmRleGVycy9VSS4KClNldHMgdGhlIGZsYWcgd2hlbiB0aGUgcXVvdGUgaXMgc3RhbGU7IGNsZWFycyBpdCB3aGVuIGEgZnJlc2gKcXVvdGUgaXMgcmVhZCDigJQgc28gdGhlIGZsYWcgc2VsZi1jb3JyZWN0cyB0aGUgbW9tZW50IFJlZmxlY3RvcgpwdWJsaXNoZXMgYSBmcmVzaCBvbmUsIHdpdGggbm8gcmV2ZXJzZSB0cmFuc2l0aW9uIHRvIGxlYXJuLiBBCm1pc3NpbmcgcXVvdGUgKGBPcmFjbGVQcmljZVVuYXZhaWxhYmxlYCkgcHJvcGFnYXRlcyBhcyBhbiBlcnJvcgppbnN0ZWFkIG9mIHNldHRpbmcgYW55dGhpbmc6ICJ0aGUgb3JhY2xlIGRvZXNuJ3QgcXVvdGUgdGhpcyBhc3NldAphdCBhbGwiIGlzIGEgcGVybWFuZW50IGNvbmZpAAAAF3JlZnJlc2hfZnhfcXVvdGVfc3RhdHVzAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAP9FeHRlbmRzIHRoZSBkZWxpdmVyeSB3aW5kb3cgYnkgbXV0dWFsIGNvbnNlbnQgd2hpbGUgdGhlIGNvbW1pdG1lbnQgaXMKc3RpbGwgcHJlLWRlbGl2ZXJ5IGFuZCB0aGUgb3JpZ2luYWwgZGVhZGxpbmUgaGFzIG5vdCBwYXNzZWQuIFRoZQpkZWFkbGluZSBpcyBtb25vdG9uaWM6IGEgY29uc2VudCBjYWxsIGNhbiBvbmx5IGFkZCB0aW1lLCBuZXZlciByZXZva2UKdGltZSB0aGF0IHdhcyBhbHJlYWR5IHByb21pc2VkIHRvIHRoZSBjb29wZXJhdGl2ZS4AAAAAGGV4dGVuZF9kZWxpdmVyeV9kZWFkbGluZQAAAAEAAAAAAAAADG5ld19kZWFkbGluZQAAAAYAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAsNUaGUgRlgtc2hvcnRmYWxsIGJ1eWVyLWRlZmF1bHQgcGF0aDogYGZ4X3Nob3J0ZmFsbF9kZWFkbGluZWAgaGFzCipzdHJpY3RseSogcGFzc2VkIChgbm93ID4gZnhfc2hvcnRmYWxsX2RlYWRsaW5lYDsgdGhlIGRlYWRsaW5lCnNlY29uZCBpdHNlbGYgc3RpbGwgcmVmdXNlcyB3aXRoIGBGeFNob3J0ZmFsbFdpbmRvd05vdFBhc3NlZGApIHdpdGgKdGhlIHRvcC11cCBuZXZlciBmdW5kZWQuIFN3ZWVwcyB3aGF0ZXZlcidzIGN1cnJlbnRseQplc2Nyb3dlZCB0byB0aGUgY29vcGVyYXRpdmUgYW5kIHNldHMgYFN0YXR1czo6RGVmYXVsdGVkYCDigJQgdGhlCipzYW1lKiBzdGF0dXMgYGV4cGlyZV9yZW1haW5kZXJfd2luZG93YCdzIGJ1eWVyLWRlZmF1bHQgYWxyZWFkeQp1c2VzLCBieSBleHBsaWNpdCBwcm9kdWN0IGRlY2lzaW9uIChzZWUgbW9kdWxlIGRvYyksIHdoaWNoIG1lYW5zCnRoZSBBUEkncyBleGlzdGluZyByZXB1dGF0aW9uIGNvbnNlcXVlbmNlIChpbW1lZGlhdGUgcGVybWFuZW50CmJhcikgYXBwbGllcyBoZXJlIHdpdGggbm8gY2hhbmdlcyBuZWVkZWQgdGhlcmUgYXQgYWxsLgoKUGVybWlzc2lvbmxlc3MsIHNhbWUgcmVhc29uaW5nIGFzIGBleHBpcmVfcmVtYWluZGVyX3dpbmRvd2A6IHRoZQpvdXRjb21lIGRvZXNuJ3QgZGVwZW5kIG9uIHdobyBjYWxscyBpdCwgb25seSBvbiB3aGV0aGVyIHRoZQpkZWFkbGluZSBoYXMgcGFzc2VkLgAAAAAaZXhwaXJlX2Z4X3Nob3J0ZmFsbF93aW5kb3cAAAAAAAAAAAABAAAD6QAAAAIAAAAD" ]),
      options
    )
  }
  public readonly fromJSON = {
    lock: this.txFromJSON<Result<void>>,
        cancel: this.txFromJSON<Result<void>>,
        settle: this.txFromJSON<Result<void>>,
        get_status: this.txFromJSON<Result<Status>>,
        initialize: this.txFromJSON<Result<void>>,
        oracle_rate: this.txFromJSON<Result<OracleRate>>,
        flag_dispute: this.txFromJSON<Result<void>>,
        fund_remainder: this.txFromJSON<Result<void>>,
        get_allocation: this.txFromJSON<Result<Array<AllocationMember>>>,
        get_commitment: this.txFromJSON<Result<Commitment>>,
        reassign_buyer: this.txFromJSON<Result<void>>,
        set_allocation: this.txFromJSON<Result<void>>,
        claim_advance_1: this.txFromJSON<Result<void>>,
        claim_advance_2: this.txFromJSON<Result<void>>,
        mark_checkpoint: this.txFromJSON<Result<void>>,
        resolve_dispute: this.txFromJSON<Result<void>>,
        confirm_delivery: this.txFromJSON<Result<void>>,
        available_actions: this.txFromJSON<Array<Action>>,
        fund_fx_shortfall: this.txFromJSON<Result<void>>,
        get_oracle_config: this.txFromJSON<Result<Option<OracleConfig>>>,
        reclaim_advance_1: this.txFromJSON<Result<void>>,
        reclaim_advance_2: this.txFromJSON<Result<void>>,
        release_advance_1: this.txFromJSON<Result<void>>,
        release_advance_2: this.txFromJSON<Result<void>>,
        ready_for_delivery: this.txFromJSON<Result<void>>,
        get_commitment_view: this.txFromJSON<Result<CommitmentView>>,
        resolve_fx_shortfall: this.txFromJSON<Result<void>>,
        expire_dispute_window: this.txFromJSON<Result<void>>,
        replace_oracle_config: this.txFromJSON<Result<void>>,
        expire_delivery_window: this.txFromJSON<Result<void>>,
        reclaim_on_nondelivery: this.txFromJSON<Result<void>>,
        expire_remainder_window: this.txFromJSON<Result<void>>,
        refresh_fx_quote_status: this.txFromJSON<Result<void>>,
        extend_delivery_deadline: this.txFromJSON<Result<void>>,
        expire_fx_shortfall_window: this.txFromJSON<Result<void>>
  }
}