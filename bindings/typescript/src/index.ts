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
   * `initialize`'s `oracle_config` was `Some` with `max_age_secs == 0`
   * — a zero staleness bound would reject every real quote, which is
   * never what a caller actually wants (that's what leaving
   * `oracle_config` as `None` is for).
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
  38: {message:"AllocationTooLarge"}
}

export type Status = {tag: "Draft", values: void} | {tag: "Locked", values: void} | {tag: "Advance1Released", values: void} | {tag: "CheckpointPassed", values: void} | {tag: "Advance2Released", values: void} | {tag: "ReadyForDelivery", values: void} | {tag: "Delivered", values: void} | {tag: "Settled", values: void} | {tag: "Cancelled", values: void} | {tag: "Defaulted", values: void} | {tag: "Forfeited", values: void} | {tag: "Disputed", values: void};


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
 */
delivery_deadline: u64;
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
 * 0 = not yet opened. Set by `ready_for_delivery`.
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
 * make concrete. Using the same 7-decimal convention on both sides
 * of the multiplication is what lets `resolve_fx_shortfall`'s
 * conversion formula skip a separate unit-normalization step — see
 * its doc comment for the worked example.
 * 
 * **Not** the oracle's `decimals()` — that is a property of the
 * oracle, read live at settlement and assumed immutable (see
 * `docs/reflector-decimals-assumption.md`, issue #63); this field is
 * the deal's value in the same 7-decimal units as `total_amount`.
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

export interface Client {
  /**
   * Construct and simulate a lock transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Draft -> Locked. Pulls the buyer's **deposit** into the contract —
   * `advance1_bps + advance2_bps` of `total_amount`, not the full
   * amount. See module docs for why this changed from earlier versions
   * of this contract, which escrowed everything here.
   * 
   * No prior SEP-41 `approve()` needed — verified live against real
   * testnet, not assumed. Soroban's single-transaction nested-
   * authorization model means the buyer's signature on the top-level
   * `lock` invocation already covers the nested `token::Client::transfer`
   * call's own `require_auth()` below; this differs from the classic
   * ERC-20-style approve/transferFrom pattern a reader coming from EVM
   * might expect. (An earlier version of this comment claimed approve
   * was required — that was never checked against real behavior; this
   * replaces it with what was actually verified.)
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
  initialize: ({buyer, cooperative, warehouse_operator, token, total_amount, advance1_bps, advance2_bps, claim_window_secs, remainder_window_secs, delivery_window_secs, contracted_quantity, grade_price_bps, oracle_config}: {buyer: string, cooperative: string, warehouse_operator: string, token: string, total_amount: i128, advance1_bps: u32, advance2_bps: u32, claim_window_secs: u64, remainder_window_secs: u64, delivery_window_secs: u64, contracted_quantity: u32, grade_price_bps: Array<u32>, oracle_config: Option<OracleConfig>}, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

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
   */
  ready_for_delivery: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>

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
   * occurred worth recording as a forfeiture.
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
      new ContractSpec([ "AAAABAAAAAAAAAAAAAAABUVycm9yAAAAAAAAJgAAAAAAAAASQWxyZWFkeUluaXRpYWxpemVkAAAAAAABAAAAAAAAAA5Ob3RJbml0aWFsaXplZAAAAAAAAgAAAAAAAAAMSW52YWxpZFN0YXRlAAAAAwAAAAAAAAAKSW52YWxpZEJwcwAAAAAABAAAAAAAAAAKWmVyb0Ftb3VudAAAAAAABQAAAAAAAAANSW52YWxpZFdpbmRvdwAAAAAAAAYAAAAAAAAADE5vdFlldE9wZW5lZAAAAAcAAAAAAAAADkFscmVhZHlDbGFpbWVkAAAAAAAIAAAAAAAAAA5BbHJlYWR5RXhwaXJlZAAAAAAACQAAAAAAAAARQ2xhaW1XaW5kb3dQYXNzZWQAAAAAAAAKAAAAAAAAABRDbGFpbVdpbmRvd05vdFBhc3NlZAAAAAsAAADiQSB0cmFuY2hlIGlzIG5laXRoZXIgY2xhaW1lZCBub3IgZXhwaXJlZCB5ZXQg4oCUIGBzZXR0bGVgIHJlZnVzZXMgdG8KcnVuIHVudGlsIHNvbWVvbmUgcmVzb2x2ZXMgaXQgdmlhIGBjbGFpbV9hZHZhbmNlXypgIG9yCmByZWNsYWltX2FkdmFuY2VfKmAuIFNlZSBgc2V0dGxlYCdzIGRvYyBjb21tZW50IGZvciB3aHkgdGhpcyBpcyBhCmhhcmQgcmVxdWlyZW1lbnQsIG5vdCBqdXN0IGEgbmljZXR5LgAAAAAAEVRyYW5jaGVVbnJlc29sdmVkAAAAAAAADAAAAAAAAAAWUmVtYWluZGVyQWxyZWFkeUZ1bmRlZAAAAAAADQAAAHlgZnVuZF9yZW1haW5kZXJgIGNhbGxlZCBhZnRlciBgcmVtYWluZGVyX2RlYWRsaW5lYCBoYXMgKnN0cmljdGx5KgpwYXNzZWQg4oCUIHRoZSBkZWFkbGluZSBzZWNvbmQgaXRzZWxmIGlzIHN0aWxsIGluIHRpbWUuAAAAAAAAFVJlbWFpbmRlcldpbmRvd1Bhc3NlZAAAAAAAAA4AAADYYGV4cGlyZV9yZW1haW5kZXJfd2luZG93YCBjYWxsZWQgYXQgb3IgYmVmb3JlCmByZW1haW5kZXJfZGVhZGxpbmVgIOKAlCB0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBzdGlsbCBiZWxvbmdzIHRvCnRoZSBidXllcidzIGZ1bmRpbmcgd2luZG93LCBzbyBleHBpcnkgbmVlZHMgc3RyaWN0bHkgbW9yZSB0aGFuIHRoZQpkZWFkbGluZSwgbm90IG1lcmVseSB0aGUgZGVhZGxpbmUuAAAAGFJlbWFpbmRlcldpbmRvd05vdFBhc3NlZAAAAA8AAABhYGNvbmZpcm1fZGVsaXZlcnlgIGNhbGxlZCB3aGlsZSBgUmVhZHlGb3JEZWxpdmVyeWAgYnV0IHRoZSBidXllcgpoYXNuJ3QgZnVuZGVkIHRoZSByZW1haW5kZXIgeWV0LgAAAAAAABJSZW1haW5kZXJOb3RGdW5kZWQAAAAAABAAAACBYHJlY2xhaW1fb25fbm9uZGVsaXZlcnlgIGNhbGxlZCBhdCBvciBiZWZvcmUgYGRlbGl2ZXJ5X2RlYWRsaW5lYCDigJQKdGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgc3RpbGwgYmVsb25ncyB0byB0aGUgY29vcGVyYXRpdmUuAAAAAAAAGURlbGl2ZXJ5RGVhZGxpbmVOb3RQYXNzZWQAAAAAAAARAAAAL2Bjb250cmFjdGVkX3F1YW50aXR5YCB3YXMgemVybyBhdCBgaW5pdGlhbGl6ZWAuAAAAAA9JbnZhbGlkUXVhbnRpdHkAAAAAEgAAAIhgZ3JhZGVfcHJpY2VfYnBzYCB3YXMgZW1wdHksIG9yIGNvbnRhaW5lZCBhbiBlbnRyeSBvdmVyIDEwXzAwMAooYSBncmFkZSB3b3J0aCBtb3JlIHRoYW4gdGhlIHByZS1hZ3JlZWQgZnVsbCB1bml0IHByaWNlKSBhdApgaW5pdGlhbGl6ZWAuAAAAFEludmFsaWRHcmFkZVNjaGVkdWxlAAAAEwAAAEhgY29uZmlybV9kZWxpdmVyeWAncyBgZ3JhZGVfaW5kZXhgIGRpZG4ndCBsYW5kIGluc2lkZQpgZ3JhZGVfcHJpY2VfYnBzYC4AAAARSW52YWxpZEdyYWRlSW5kZXgAAAAAAAAUAAAAaGBzZXRfYWxsb2NhdGlvbmAgY2FsbGVkIGEgc2Vjb25kIHRpbWUg4oCUIHRoZSBhbGxvY2F0aW9uIGxlZGdlciBpcwpvbmUtdGltZSBhbmQgaW1tdXRhYmxlIG9uY2UgcmVjb3JkZWQuAAAAFEFsbG9jYXRpb25BbHJlYWR5U2V0AAAAFQAAAGBgc2V0X2FsbG9jYXRpb25gJ3MgYG1lbWJlcnNgIHdhcyBlbXB0eSwgb3IgdGhlIHN1bSBvZiBldmVyeQplbnRyeSdzIGBzaGFyZV9icHNgIGV4Y2VlZGVkIDEwXzAwMC4AAAARSW52YWxpZEFsbG9jYXRpb24AAAAAAAAWAAAAOWBnZXRfYWxsb2NhdGlvbmAgY2FsbGVkIGJlZm9yZSBgc2V0X2FsbG9jYXRpb25gIGV2ZXIgcmFuLgAAAAAAABBBbGxvY2F0aW9uTm90U2V0AAAAFwAAAOBgaW5pdGlhbGl6ZWAncyBgb3JhY2xlX2NvbmZpZ2Agd2FzIGBTb21lYCB3aXRoIGBtYXhfYWdlX3NlY3MgPT0gMGAK4oCUIGEgemVybyBzdGFsZW5lc3MgYm91bmQgd291bGQgcmVqZWN0IGV2ZXJ5IHJlYWwgcXVvdGUsIHdoaWNoIGlzCm5ldmVyIHdoYXQgYSBjYWxsZXIgYWN0dWFsbHkgd2FudHMgKHRoYXQncyB3aGF0IGxlYXZpbmcKYG9yYWNsZV9jb25maWdgIGFzIGBOb25lYCBpcyBmb3IpLgAAABNJbnZhbGlkT3JhY2xlQ29uZmlnAAAAABgAAACvYG9yYWNsZV9yYXRlYCBvciBgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIGNhbGxlZCBidXQgYGluaXRpYWxpemVgCm5ldmVyIHNldCBhbiBgb3JhY2xlX2NvbmZpZ2AgZm9yIHRoaXMgY29tbWl0bWVudCDigJQgdGhlIG9ubHkgdHdvCmZ1bmN0aW9ucyB0aGF0IHJlYWQgdGhlIG9yYWNsZSBhdCBhbGwgKCMxMjQpLgAAAAATT3JhY2xlTm90Q29uZmlndXJlZAAAAAAZAAAAtlRoZSBjb25maWd1cmVkIFJlZmxlY3RvciBvcmFjbGUgcmV0dXJuZWQgYE5vbmVgIGZvcgpgb3JhY2xlX2NvbmZpZy5wcmljZV9hc3NldGAg4oCUIGl0IGRvZXNuJ3QgcXVvdGUgdGhhdCBhc3NldCBhdCBhbGwKKGNhbGwgdGhlIG9yYWNsZSdzIG93biBgYXNzZXRzKClgIHRvIGNoZWNrIHdoYXQgaXQgZG9lcyBxdW90ZSkuAAAAAAAWT3JhY2xlUHJpY2VVbmF2YWlsYWJsZQAAAAAAGgAAAN1UaGUgbW9zdCByZWNlbnQgcXVvdGUgaXMgb2xkZXIgdGhhbiBgb3JhY2xlX2NvbmZpZy5tYXhfYWdlX3NlY3NgCmFsbG93cyAoUFJEIMKnMTYuMydzIHN0YWxlbmVzcyBib3VuZCkg4oCUIHJlZnVzZWQgcmF0aGVyIHRoYW4KcmV0dXJuZWQsIHNpbmNlIGEgY2FsbGVyIHNpbGVudGx5IGdldHRpbmcgYSB0b28tb2xkIHJhdGUgaXMgd29yc2UKdGhhbiBnZXR0aW5nIG5vIHJhdGUgYXQgYWxsLgAAAAAAAAtPcmFjbGVTdGFsZQAAAAAbAAAA2mByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAgY2FsbGVkIGEgc2Vjb25kIHRpbWUg4oCUIG9uZS10aW1lIGFuZAppbW11dGFibGUsIHNhbWUgcmVhc29uaW5nIGFzIGBzZXRfYWxsb2NhdGlvbmA6IHJlLXJlc29sdmluZyBsYXRlcgphZ2FpbnN0IGEgZGlmZmVyZW50IHJhdGUgd291bGQgbGV0IGVpdGhlciBzaWRlIGdhbWUgdGhlIG51bWJlcgpieSBjaG9vc2luZyB3aGVuIHRvIGNhbGwgaXQuAAAAAAARRnhBbHJlYWR5UmVzb2x2ZWQAAAAAAAAcAAAAi2BmdW5kX2Z4X3Nob3J0ZmFsbGAvYGV4cGlyZV9meF9zaG9ydGZhbGxfd2luZG93YC9gc2V0dGxlYCBjYWxsZWQKYmVmb3JlIGByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAgaGFzIHJ1biBmb3IgYW4gb3JhY2xlLWNvbmZpZ3VyZWQKY29tbWl0bWVudC4AAAAADUZ4Tm90UmVzb2x2ZWQAAAAAAAAdAAAAQGBzZXR0bGVgIGNhbGxlZCB3aGlsZSBhIHJlc29sdmVkIEZYIHNob3J0ZmFsbCBpcyBzdGlsbCB1bmZ1bmRlZC4AAAATRnhTaG9ydGZhbGxVbmZ1bmRlZAAAAAAeAAAA3WBmdW5kX2Z4X3Nob3J0ZmFsbGAvYGV4cGlyZV9meF9zaG9ydGZhbGxfd2luZG93YCBjYWxsZWQgYnV0CmByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAgZm91bmQgbm8gc2hvcnRmYWxsIGF0IGFsbCAodGhlIGVzY3Jvd2VkCmFtb3VudCBhbHJlYWR5IGNvdmVyZWQgdGhlIGZyZXNoLXJhdGUtY29udmVydGVkIG9ibGlnYXRpb24pIOKAlAp0aGVyZSdzIG5vdGhpbmcgdG8gZnVuZCBvciBleHBpcmUuAAAAAAAADU5vRnhTaG9ydGZhbGwAAAAAAAAfAAAAKWBmdW5kX2Z4X3Nob3J0ZmFsbGAgY2FsbGVkIGEgc2Vjb25kIHRpbWUuAAAAAAAAGEZ4U2hvcnRmYWxsQWxyZWFkeUZ1bmRlZAAAACAAAADWYGZ1bmRfZnhfc2hvcnRmYWxsYCBjYWxsZWQgYWZ0ZXIgYGZ4X3Nob3J0ZmFsbF9kZWFkbGluZWAgaGFzCipzdHJpY3RseSogcGFzc2VkIOKAlCB0b28gbGF0ZSB0byBjdXJlLCB0aG91Z2ggdGhlIGRlYWRsaW5lIHNlY29uZAppdHNlbGYgaXMgc3RpbGwgaW4gdGltZS4gYGV4cGlyZV9meF9zaG9ydGZhbGxfd2luZG93YCBpcyB0aGUgb25seQpwYXRoIGxlZnQgZnJvbSBoZXJlLgAAAAAAF0Z4U2hvcnRmYWxsV2luZG93UGFzc2VkAAAAACEAAADdYGV4cGlyZV9meF9zaG9ydGZhbGxfd2luZG93YCBjYWxsZWQgYXQgb3IgYmVmb3JlCmBmeF9zaG9ydGZhbGxfZGVhZGxpbmVgIOKAlCB0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBzdGlsbCBiZWxvbmdzCnRvIHRoZSBidXllcidzIGZ1bmRpbmcgd2luZG93LCBtYXRjaGluZyBgZnVuZF9meF9zaG9ydGZhbGxgJ3Mgb3duCmluY2x1c2l2ZSBjdXQtb2ZmIChgbm93ID4gZGVhZGxpbmVgKS4AAAAAAAAaRnhTaG9ydGZhbGxXaW5kb3dOb3RQYXNzZWQAAAAAACIAAACpYGZsYWdfZGlzcHV0ZWAncyBgZmxhZ2dlcmAgYXJndW1lbnQgd2Fzbid0IHRoZSBidXllciwgY29vcGVyYXRpdmUsCm9yIHdhcmVob3VzZSBvcGVyYXRvciDigJQgb25seSBhIG5hbWVkIHBhcnR5IHRvIHRoZSBkZWFsIGNhbiBmbGFnCm9uZSwgbm90IGFuIGFyYml0cmFyeSB0aGlyZCBhZGRyZXNzLgAAAAAAAAlOb3RBUGFydHkAAAAAAAAjAAABVmBleHBpcmVfZGlzcHV0ZV93aW5kb3dgIGNhbGxlZCBhdCBvciBiZWZvcmUgYGRpc3B1dGVfZGVhZGxpbmVgIOKAlAp0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBpcyBzdGlsbCBpbnNpZGUgdGhlIGZyZWV6ZS4gT25seSB0aGF0CnRpbWVvdXQgcGF0aCBjYW4gcHJvZHVjZSB0aGlzOiBgcmVzb2x2ZV9kaXNwdXRlYCBoYXMgbm8gZGVhZGxpbmUKZ2F0ZSBhdCBhbGwuIChFaXRoZXIgY2FsbCBtYWRlIHdoaWxlIGBzdGF0dXNgIGlzbid0IGBEaXNwdXRlZGAgaXMKY292ZXJlZCBieSB0aGUgc2FtZSBgSW52YWxpZFN0YXRlYCBldmVyeSBvdGhlciBzdGF0dXMtZ2F0ZWQgY2FsbAphbHJlYWR5IHVzZXMuKQAAAAAAFkRpc3B1dGVXaW5kb3dOb3RQYXNzZWQAAAAAACQAAACuYGluaXRpYWxpemVgJ3MgYGdyYWRlX3ByaWNlX2Jwc2AgaGVsZCBtb3JlIHRoYW4KYE1BWF9HUkFERV9FTlRSSUVTYCBlbnRyaWVzIOKAlCBsb25nZXIgdGhhbiBhbnkgcmVhbCBncmFkaW5nCnNjaGVkdWxlIG5lZWRzLCBhbmQgdW5ib3VuZGVkIHdvcmsgdGhlIGNvbnRyYWN0IHNob3VsZG4ndCBhY2NlcHQuAAAAAAAUR3JhZGVTY2hlZHVsZVRvb0xvbmcAAAAlAAAAs2BzZXRfYWxsb2NhdGlvbmAncyBgbWVtYmVyc2AgaGVsZCBtb3JlIHRoYW4KYE1BWF9BTExPQ0FUSU9OX01FTUJFUlNgIGVudHJpZXMg4oCUIGJleW9uZCB0aGUgUFJEJ3Mgc3BlY2lmaWVkCmNvb3BlcmF0aXZlIHNjYWxlLCBhbmQgdW5ib3VuZGVkIHN0b3JhZ2UgdGhlIGNvbnRyYWN0IHNob3VsZG4ndAphY2NlcHQuAAAAABJBbGxvY2F0aW9uVG9vTGFyZ2UAAAAAACY=",
        "AAAAAgAAAAAAAAAAAAAABlN0YXR1cwAAAAAADAAAAAAAAAAAAAAABURyYWZ0AAAAAAAAAAAAAAAAAAAGTG9ja2VkAAAAAAAAAAAAAAAAABBBZHZhbmNlMVJlbGVhc2VkAAAAAAAAAAAAAAAQQ2hlY2twb2ludFBhc3NlZAAAAAAAAAAAAAAAEEFkdmFuY2UyUmVsZWFzZWQAAAAAAAAAZ1RoZSBjb29wZXJhdGl2ZSBoYXMgc2lnbmFsZWQgaW50ZW50IHRvIGRlbGl2ZXIg4oCUIG9wZW5zIHRoZQpyZW1haW5kZXItcGF5bWVudCB3aW5kb3cuIFNlZSBtb2R1bGUgZG9jcy4AAAAAEFJlYWR5Rm9yRGVsaXZlcnkAAAAAAAAAAAAAAAlEZWxpdmVyZWQAAAAAAAAAAAAAAAAAAAdTZXR0bGVkAAAAAAAAAAAAAAAACUNhbmNlbGxlZAAAAAAAAAAAAACAQnV5ZXIgZmFpbGVkIHRvIGZ1bmQgdGhlIHJlbWFpbmRlciBiZWZvcmUgaXRzIGRlYWRsaW5lLiBVbmNvbnRlc3RlZApieSBjb25zdHJ1Y3Rpb24g4oCUIHRoZSBkZWFkbGluZSBlaXRoZXIgcGFzc2VkIG9yIGl0IGRpZG4ndC4AAAAJRGVmYXVsdGVkAAAAAAAAAAAAAWlDb29wZXJhdGl2ZSBuZXZlciByZWFjaGVkIGBEZWxpdmVyZWRgIGJlZm9yZSB0aGUgb3ZlcmFsbCBkZWxpdmVyeQpkZWFkbGluZS4gQWxzbyB1bmNvbnRlc3RlZCBieSBjb25zdHJ1Y3Rpb24uIERlbGliZXJhdGVseSBhCnNlcGFyYXRlIHZhcmlhbnQgZnJvbSBgRGVmYXVsdGVkYCwgbm90IGEgcmV1c2Ugb2YgaXQg4oCUIHRoZSB0d28KcmVwcmVzZW50IG9wcG9zaXRlIHBhcnRpZXMnIGZhaWx1cmUsIGFuZCBjb2xsYXBzaW5nIHRoZW0gaW50byBvbmUKc3RhdHVzIHdvdWxkIG1ha2UgYW4gYWxyZWFkeS1zZXR0bGVkIGNvbW1pdG1lbnQncyBoaXN0b3J5CmFtYmlndW91cyBhYm91dCB3aG8gYWN0dWFsbHkgZmFpbGVkIHRvIHBlcmZvcm0uAAAAAAAACUZvcmZlaXRlZAAAAAAAAAAAAAAAAAAACERpc3B1dGVk",
        "AAAAAQAABABPbi1jaGFpbiBzdGF0ZSBmb3IgYSBzaW5nbGUgZXNjcm93IGNvbW1pdG1lbnQgaW5zdGFuY2UuCgoqKkZvb3RwcmludCBBdWRpdCAmIFN0b3JhZ2UgTmVjZXNzaXR5IEZpbmRpbmcqKjoKQSBjb21wcmVoZW5zaXZlIHJldmlldyBvZiBldmVyeSBmaWVsZCBpbiB0aGlzIHN0cnVjdCBjb25maXJtZWQgdGhhdCBhbGwgMzMKZmllbGRzIGFyZSBnZW51aW5lbHkgbmVjZXNzYXJ5IGluIG9uLWNoYWluIGluc3RhbmNlIHN0b3JhZ2UgYW5kIGNhbm5vdCBiZQptb3ZlZCBvZmYtY2hhaW4gb3IgZGVyaXZlZCBmcm9tIGV2ZW50czoKMS4gKipBdXRoICYgVG9rZW4gUm91dGluZyoqIChgYnV5ZXJgLCBgY29vcGVyYXRpdmVgLCBgd2FyZWhvdXNlX29wZXJhdG9yYCwgYHRva2VuYCk6CkVuZm9yY2VzIGByZXF1aXJlX2F1dGgoKWAgYW5kIHJvdXRlcyB0b2tlbiB0cmFuc2ZlcnMgYWNyb3NzIGV2ZXJ5IGxpZmVjeWNsZSBjYWxsLgoyLiAqKkZpbmFuY2lhbCBUZXJtcyAmIFNjaGVkdWxlcyoqIChgdG90YWxfYW1vdW50YCwgYGFkdmFuY2UxX2Jwc2AsIGBhZHZhbmNlMl9icHNgLApgY2xhaW1fd2luZG93X3NlY3NgLCBgcmVtYWluZGVyX3dpbmRvd19zZWNzYCwgYGRlbGl2ZXJ5X2RlYWRsaW5lYCwgYGNvbnRyYWN0ZWRfcXVhbnRpdHlgLApgZ3JhZGVfcHJpY2VfYnBzYCk6ClNtYXJ0IGNvbnRyYWN0cyBjYW5ub3QgcXVlcnkgaGlzdG9yaWNhbCBldmVudCBsb2dzOyBldmVyeSBndWFyZCwgZGVhZGxpbmUgY2hlY2ssCmFuZCBzZXR0bGVtZW50IHBheW91dCBlcXVhdGlvbiByZXF1aXJlcyB0aGVzZSBiYXNlbGluZSB0ZXJtcyBvbi1jaGFpbi4KMy4gKipTdGF0ZSAmIExpZmVjeWNsZSBGbGFncyoqIChgc3RhdHVzYCwgYGNyZWF0ZWRfYXRgLCBgYWR2YW5jZTFfZGVhZGxpbmVgLApgYWR2YW5jZTFfY2xhaW1lZGAsIGBhZHZhbmNlMV9leHBpcmVkYCwgYGFkdmFuY2UyX2RlYWRsaW5lYCwgYGFkdmFuY2UyX2NsYWltZWRgLApgYWR2YW5jZTJfZXhwaXJlZGAsIGByZW1haW5kZXJfZGVhZGxpbmVgLCBgcmVtYWluZGVyX2Z1bmRlAAAAAAAAAApDb21taXRtZW50AAAAAAAhAAAAP0Jhc2lzIHBvaW50cyAob2YgYHRvdGFsX2Ftb3VudGApIHJlbGVhc2VkIGF0IGxvY2staW4uIDAtMTBfMDAwLgAAAAAMYWR2YW5jZTFfYnBzAAAABAAAAAAAAAAQYWR2YW5jZTFfY2xhaW1lZAAAAAEAAACxMCA9IG5vdCB5ZXQgb3BlbmVkLiBPbmNlIGByZWxlYXNlX2FkdmFuY2VfMWAgcnVucywgdGhpcyBpcyBzZXQgdG8KdGhlIGxlZGdlciB0aW1lc3RhbXAgYWZ0ZXIgd2hpY2ggdGhlIGNvb3BlcmF0aXZlIGNhbiBubyBsb25nZXIKY2xhaW0gYW5kIHRoZSBidXllciBiZWNvbWVzIGVsaWdpYmxlIHRvIHJlY2xhaW0uAAAAAAAAEWFkdmFuY2UxX2RlYWRsaW5lAAAAAAAABgAAAAAAAAAQYWR2YW5jZTFfZXhwaXJlZAAAAAEAAADyQmFzaXMgcG9pbnRzIHJlbGVhc2VkIGF0IHRoZSBtaWQtc2Vhc29uIGNoZWNrcG9pbnQuIDAtMTBfMDAwLgpgYWR2YW5jZTFfYnBzICsgYWR2YW5jZTJfYnBzYCBtdXN0IG5vdCBleGNlZWQgMTBfMDAwIOKAlCBlbmZvcmNlZCBhdApgaW5pdGlhbGl6ZWAuIFRvZ2V0aGVyIHRoZXNlIHR3byBkZWZpbmUgdGhlICpkZXBvc2l0KiDigJQgdGhlCnBvcnRpb24gYGxvY2tgIGFjdHVhbGx5IGVzY3Jvd3M7IHNlZSBtb2R1bGUgZG9jcy4AAAAAAAxhZHZhbmNlMl9icHMAAAAEAAAAAAAAABBhZHZhbmNlMl9jbGFpbWVkAAAAAQAAAAAAAAARYWR2YW5jZTJfZGVhZGxpbmUAAAAAAAAGAAAAAAAAABBhZHZhbmNlMl9leHBpcmVkAAAAAQAAAAAAAAAFYnV5ZXIAAAAAAAATAAADVEhvdyBsb25nIHRoZSBjb29wZXJhdGl2ZSBoYXMgdG8gY2xhaW0gYW4gYWR2YW5jZSBvbmNlIGl0IG9wZW5zLAppbiBzZWNvbmRzLiBTYW1lIHdpbmRvdyBmb3IgYm90aCB0cmFuY2hlcyDigJQgUFJEIGRvZXNuJ3QgY2FsbCBmb3IKZGlmZmVyZW50IHdpbmRvd3MgcGVyIHRyYW5jaGUsIHNvIG9uZSBzaGFyZWQgdmFsdWUgaXMgdGhlIHNpbXBsZXIKY2hvaWNlIHVudGlsIGEgcmVhc29uIHRvIHNwbGl0IHRoZW0gc2hvd3MgdXAuCgpCb3VuZGFyeSBjb252ZW50aW9uICgjMTExLCBwaW5uZWQgYnkgYHRlc3RzL2RlYWRsaW5lX2JvdW5kYXJ5LnJzYCk6CnRoZSBkZWFkbGluZSBpcyBgcmVsZWFzZSB0aW1lICsgY2xhaW1fd2luZG93X3NlY3NgLCBhbmQgKmNsYWltaW5nKiBpcwphbGxvd2VkIHVwIHRvICoqYW5kIGluY2x1ZGluZyoqIHRoYXQgaW5zdGFudCAoYHRpbWVzdGFtcCA+IGRlYWRsaW5lYAppcyB0aGUgcmVqZWN0aW9uIGd1YXJkKSB3aGlsZSAqcmVjbGFpbWluZyogaXMgYWxsb3dlZCBvbmx5ICoqc3RyaWN0bHkKYWZ0ZXIqKiBpdCAoYHRpbWVzdGFtcCA8PSBkZWFkbGluZWAgaXMgdGhlIHJlamVjdGlvbiBndWFyZCkuIFRoZSB0d28KZ3VhcmRzIGFyZSBleGFjdCBjb21wbGVtZW50cywgc28gZXZlcnkgbGVkZ2VyIHRpbWVzdGFtcCBiZWxvbmdzIHRvCmV4YWN0bHkgb25lIHBhcnR5LiBDb25zZXF1ZW5jZSB3b3J0aCBrbm93aW5nOiB0aGUgY29vcGVyYXRpdmUncwplZmZlY3RpdmUgd2luZG93IHNwYW5zIGBjbGFpbV93aW5kb3dfc2VjcyArIDFgIGRpc3RpbmN0IHRpbWVzdGFtcHMKKHRoZSBvcGVuaW5nIG9uZSBhbmQgdGhlIGRlYWRsaW5lIG9uZSksIG5vdCBgY2xhaW1fd2luZG93X3NlY3NgLgAAABFjbGFpbV93aW5kb3dfc2VjcwAAAAAAAAYAAADDVGhlIHF1YW50aXR5IChjYWxsZXItZGVmaW5lZCB1bml0LCBlLmcuIGtnKSB0aGUgYnV5ZXIgaXMKY29udHJhY3RpbmcgZm9yLiBTZXQgYXQgYGluaXRpYWxpemVgLCBuZXZlciBjaGFuZ2VzLiBUaGUKZGVub21pbmF0b3IgZm9yIGBjb25maXJtX2RlbGl2ZXJ5YCdzIHByb3BvcnRpb25hbCBzaG9ydGZhbGwKYWRqdXN0bWVudCAoUFJEIMKnNykuAAAAABNjb250cmFjdGVkX3F1YW50aXR5AAAAAAQAAAAAAAAAC2Nvb3BlcmF0aXZlAAAAABMAAAAAAAAACmNyZWF0ZWRfYXQAAAAAAAYAAAC9U2V0IGJ5IGBjb25maXJtX2RlbGl2ZXJ5YC4gMCB1bnRpbCB0aGVuIChhbHNvIGEgbGVnaXRpbWF0ZSByZWFsCnZhbHVlIOKAlCB0b3RhbCBjcm9wIGZhaWx1cmUg4oCUIGJ1dCBub3RoaW5nIHJlYWRzIHRoaXMgZmllbGQgYmVmb3JlCmBTdGF0dXM6OkRlbGl2ZXJlZGAsIHNvIG5vIHNlcGFyYXRlIHNlbnRpbmVsIGlzIG5lZWRlZCkuAAAAAAAAEmRlbGl2ZXJlZF9xdWFudGl0eQAAAAAABAAAAOZBYnNvbHV0ZSBkZWFkbGluZSAoYGNyZWF0ZWRfYXQgKyBkZWxpdmVyeV93aW5kb3dfc2Vjc2ApLCBjb21wdXRlZApvbmNlIGF0IGBpbml0aWFsaXplYCDigJQgdW5saWtlIHRoZSB0cmFuY2hlL3JlbWFpbmRlciBkZWFkbGluZXMsCnRoaXMgb25lIGRvZXNuJ3QgZGVwZW5kIG9uIHNvbWUgb3RoZXIgY2FsbCBoYXBwZW5pbmcgZmlyc3QsIHNvCnRoZXJlJ3Mgbm8gIjAgPSB1bnNldCIgc3RhdGUgZm9yIGl0LgAAAAAAEWRlbGl2ZXJ5X2RlYWRsaW5lAAAAAAAABgAAAN4wIHdoZW5ldmVyIGBzdGF0dXNgIGlzbid0IGBEaXNwdXRlZGAuIFJldXNlcyBgcmVtYWluZGVyX3dpbmRvd19zZWNzYAphcyB0aGUgd2luZG93IGxlbmd0aCwgc2FtZSByZWFzb25pbmcgYXMgYGZ4X3Nob3J0ZmFsbF9kZWFkbGluZWAKYWJvdmUg4oCUIHNlZSBgZmxhZ19kaXNwdXRlYCdzIGRvYyBjb21tZW50IGZvciB3aHkgYSBkaXNwdXRlIGZyZWV6ZQpuZWVkcyBhIGJvdW5kIGF0IGFsbC4AAAAAABBkaXNwdXRlX2RlYWRsaW5lAAAABgAAAgFTZXQgYnkgYGZsYWdfZGlzcHV0ZWAsIHRoZSBzdGF0dXMgdG8gcmVzdG9yZSBvbiBgcmVzb2x2ZV9kaXNwdXRlYApvciBgZXhwaXJlX2Rpc3B1dGVfd2luZG93YC4gYFN0YXR1czo6RHJhZnRgIHdoZW5ldmVyIGBzdGF0dXNgCmlzbid0IGBEaXNwdXRlZGAg4oCUIGEgc2FmZSAidW5zZXQiIHNlbnRpbmVsLCBub3QgYSByZWFsIHBvc3NpYmxlCnZhbHVlLCBzaW5jZSBgZmxhZ19kaXNwdXRlYCBuZXZlciBhY2NlcHRzIGBEcmFmdGAgYXMgYSBzdGF0dXMgdG8KZGlzcHV0ZSAqZnJvbSouIChOb3QgYE9wdGlvbjxTdGF0dXM+YDogdGhlIGBjb250cmFjdHR5cGVgIG1hY3JvJ3MKZ2VuZXJhdGVkIGBTY1ZhbGAgY29udmVyc2lvbiBkb2Vzbid0IHNhdGlzZnkgdGhlIGJsYW5rZXQgYE9wdGlvbmAKaW1wbCBmb3IgYSB1bml0LXZhcmlhbnQtb25seSBlbnVtIGxpa2UgYFN0YXR1c2AsIHVubGlrZSBhIHN0cnVjdApzdWNoIGFzIGBPcmFjbGVDb25maWdgIOKAlCBjb25maXJtZWQgYnkgdHJ5aW5nIGl0IGZpcnN0LikAAAAAAAASZGlzcHV0ZV9wcmVfc3RhdHVzAAAAAAfQAAAABlN0YXR1cwAAAAABL2BvcmFjbGVfY29uZmlnLmRlbm9taW5hdGVkX2Ftb3VudGAsIGFkanVzdGVkIGJ5IGBzZXR0bGVtZW50X2Jwc2AKYW5kIGNvbnZlcnRlZCB0byB0aGUgc2V0dGxlbWVudCB0b2tlbiBhdCB0aGUgZnJlc2ggcmF0ZQpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIHJlYWQuIDAgdW50aWwgcmVzb2x2ZWQgKGFsc28gd2hhdCBpdCdkIGJlCmZvciBhIHRvdGFsIHdyaXRlLW9mZiwgd2hpY2ggaXMgd2h5IGBmeF9yZXNvbHZlZGAgaXMgdGhlIGFjdHVhbAoiaGFzIHRoaXMgcnVuIiBzaWduYWwsIG5vdCB0aGlzIGZpZWxkIGJlaW5nIG5vbnplcm8pLgAAAAARZnhfYWRqdXN0ZWRfdG90YWwAAAAAAAALAAACIlNldCBieSBgcmVmcmVzaF9meF9xdW90ZV9zdGF0dXNgICgjMSk6IHRoZSBtb3N0IHJlY2VudCBvcmFjbGUtZnJlc2huZXNzCnByb2JlIGZvdW5kIGEgcXVvdGUgb2xkZXIgdGhhbiBgb3JhY2xlX2NvbmZpZy5tYXhfYWdlX3NlY3NgLCBpLmUuCmByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAgaXMgY3VycmVudGx5IGJsb2NrZWQgb24gUmVmbGVjdG9yIHB1Ymxpc2hpbmcgYQpmcmVzaCBvbmUuIENsZWFyZWQgYnkgdGhlIG5leHQgcHJvYmUgdGhhdCBmaW5kcyBhIGZyZXNoIHF1b3RlIChhbmQgYnkKYSBzdWNjZXNzZnVsIGByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAsIHNvIGl0IGNhbiBuZXZlciBsaW5nZXIgb25jZSB0aGUKb25lLXRpbWUgcmVzb2x1dGlvbiBoYXMgcnVuKS4gKipJbmZvcm1hdGlvbmFsIG9ubHkqKiDigJQgbm8gZ3VhcmQgcmVhZHMKaXQ7IHNlZSBgcmVmcmVzaF9meF9xdW90ZV9zdGF0dXNgJ3MgZG9jIGNvbW1lbnQgZm9yIHdoeSB0aGlzIGlzIGEKZmxhZyBhbG9uZ3NpZGUgYGZ4X3Jlc29sdmVkYCByYXRoZXIgdGhhbiBhIGRpc3RpbmN0IGBTdGF0dXNgLgAAAAAADmZ4X3F1b3RlX3N0YWxlAAAAAAABAAAA21NldCBieSBgcmVzb2x2ZV9meF9zaG9ydGZhbGxgLCBvbmx5IG1lYW5pbmdmdWwgd2hlbiBgb3JhY2xlX2NvbmZpZ2AKaXMgYFNvbWVgLiBgc2V0dGxlYCB1c2VzIHRoaXMgaW5zdGVhZCBvZiByZS1kZXJpdmluZyBmcm9tCmB0b3RhbF9hbW91bnRgIHdoZW5ldmVyIGFuIG9yYWNsZSBpcyBjb25maWd1cmVkIOKAlCBzZWUKYHJlc29sdmVfZnhfc2hvcnRmYWxsYCdzIGRvYyBjb21tZW50LgAAAAALZnhfcmVzb2x2ZWQAAAAAAQAAAMJIb3cgbXVjaCBtb3JlIHRoZSBidXllciBvd2VzIGJleW9uZCB3aGF0J3MgYWxyZWFkeSBlc2Nyb3dlZCwgcGVyCmByZXNvbHZlX2Z4X3Nob3J0ZmFsbGAncyBjb21wYXJpc29uLiAwIG1lYW5zIG5vIHRvcC11cCBuZWVkZWQg4oCUCmBzZXR0bGVgIGNhbiBydW4gd2l0aG91dCB3YWl0aW5nIG9uIGBmdW5kX2Z4X3Nob3J0ZmFsbGAgYXQgYWxsLgAAAAAAE2Z4X3Nob3J0ZmFsbF9hbW91bnQAAAAACwAAAPUwIGlmIGBmeF9zaG9ydGZhbGxfYW1vdW50YCBpcyAwIChubyBkZWFkbGluZSBuZWVkZWQpLiBSZXVzZXMKYHJlbWFpbmRlcl93aW5kb3dfc2Vjc2AgYXMgdGhlIHdpbmRvdyBsZW5ndGggcmF0aGVyIHRoYW4gYWRkaW5nCnlldCBhbm90aGVyIGBpbml0aWFsaXplYCBwYXJhbWV0ZXIgZm9yIGEgc2Vjb25kIGRlYWRsaW5lIHNlcnZpbmcKdGhlIHNhbWUgImdpdmUgdGhlIGJ1eWVyIGEgZmFpciBjaGFuY2UgdG8gYWN0IiBwdXJwb3NlLgAAAAAAABVmeF9zaG9ydGZhbGxfZGVhZGxpbmUAAAAAAAAGAAAAAAAAABNmeF9zaG9ydGZhbGxfZnVuZGVkAAAAAAEAAABcU2V0IGJ5IGBjb25maXJtX2RlbGl2ZXJ5YCDigJQgd2hpY2ggYGdyYWRlX3ByaWNlX2Jwc2AgZW50cnkgdGhlCndhcmVob3VzZSBvcGVyYXRvciBhdHRlc3RlZC4AAAALZ3JhZGVfaW5kZXgAAAAABAAAAclQcmUtYWdyZWVkIGdyYWRlIC0+IHByaWNlLW11bHRpcGxpZXIgdGFibGUsIGluIGJhc2lzIHBvaW50cyBvZgpgdG90YWxfYW1vdW50YCdzIGltcGxpZWQgdW5pdCBwcmljZSAoMTBfMDAwID0gZnVsbCBwcmljZSksIHNldCBhdApgaW5pdGlhbGl6ZWAgYW5kIG5ldmVyIGNoYW5nZWQg4oCUICJwcmUtYWdyZWVkLCIgcGVyIHRoZSBQUkQsIG1lYW5zCmFncmVlZCBiZWZvcmUgZGVsaXZlcnksIG5vdCBkZWNpZGVkIGF0IHNldHRsZW1lbnQgdGltZS4gT3JkZXJlZCBieQp0aGUgY2FsbGVyOyBgY29uZmlybV9kZWxpdmVyeWAncyBgZ3JhZGVfaW5kZXhgIHNlbGVjdHMgYW4gZW50cnkuCkVhY2ggZW50cnkgbXVzdCBiZSA8PSAxMF8wMDAgKGEgZ3JhZGUgY2FuJ3QgYmUgd29ydGggbW9yZSB0aGFuIHRoZQpmdWxsIGFncmVlZCB1bml0IHByaWNlKSBhbmQgdGhlcmUgbXVzdCBiZSBhdCBsZWFzdCBvbmUgZW50cnkuAAAAAAAAD2dyYWRlX3ByaWNlX2JwcwAAAAPqAAAABAAAADAwID0gbm90IHlldCBvcGVuZWQuIFNldCBieSBgcmVhZHlfZm9yX2RlbGl2ZXJ5YC4AAAAScmVtYWluZGVyX2RlYWRsaW5lAAAAAAAGAAAAAAAAABByZW1haW5kZXJfZnVuZGVkAAAAAQAAAKxIb3cgbG9uZyB0aGUgYnV5ZXIgaGFzIHRvIGZ1bmQgdGhlIHJlbWFpbmRlciBvbmNlCmByZWFkeV9mb3JfZGVsaXZlcnlgIG9wZW5zIHRoZSB3aW5kb3cuIEluZGVwZW5kZW50IG9mCmBjbGFpbV93aW5kb3dfc2Vjc2Ag4oCUIHRoaXMgaXMgYSBwYXltZW50IGRlYWRsaW5lLCBub3QgYSBjbGFpbSBvbmUuAAAAFXJlbWFpbmRlcl93aW5kb3dfc2VjcwAAAAAAAAYAAAFeU2V0IGJ5IGBjb25maXJtX2RlbGl2ZXJ5YDogdGhlIGNvbWJpbmVkIHF1YW50aXR5IHggZ3JhZGUKbXVsdGlwbGllciwgaW4gYmFzaXMgcG9pbnRzIG9mIGB0b3RhbF9hbW91bnRgLCB0aGF0IGBzZXR0bGVgIHBheXMKb3V0IGFnYWluc3QuIGBtaW4oZGVsaXZlcmVkX3F1YW50aXR5LCBjb250cmFjdGVkX3F1YW50aXR5KSAvCmNvbnRyYWN0ZWRfcXVhbnRpdHkgKiBncmFkZV9wcmljZV9icHNbZ3JhZGVfaW5kZXhdYCDigJQgY2FwcGVkIGF0CjEwXzAwMCBieSBjb25zdHJ1Y3Rpb24gKG92ZXItZGVsaXZlcnkgaXNuJ3QgcGFpZCBleHRyYSBpbiB2MTsgc2VlCmBjb25maXJtX2RlbGl2ZXJ5YCdzIGRvYyBjb21tZW50KS4AAAAAAA5zZXR0bGVtZW50X2JwcwAAAAAABAAAAAAAAAAGc3RhdHVzAAAAAAfQAAAABlN0YXR1cwAAAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAADHRvdGFsX2Ftb3VudAAAAAsAAAAAAAAAEndhcmVob3VzZV9vcGVyYXRvcgAAAAAAEw==",
        "AAAAAQAAAQxBIGxpdmUgcmF0ZSByZWFkIGZyb20gdGhlIGNvbmZpZ3VyZWQgUmVmbGVjdG9yIG9yYWNsZSwgcmV0dXJuZWQgYnkKYG9yYWNsZV9yYXRlYC4gTWlycm9ycyBSZWZsZWN0b3IncyBvd24gYFByaWNlRGF0YWAgc2hhcGUgKHNlZQpgcmVmbGVjdG9yLnJzYCkgcmF0aGVyIHRoYW4gcmV1c2luZyB0aGF0IHR5cGUgZGlyZWN0bHksIHNpbmNlIHRoaXMgaXMKdGhpcyBjb250cmFjdCdzIG93biBwdWJsaWMgaW50ZXJmYWNlLCBub3QgYSByZS1leHBvcnQgb2YgUmVmbGVjdG9yJ3MuAAAAAAAAAApPcmFjbGVSYXRlAAAAAAACAAABXmBwcmljZSAvIDEwXmRlY2ltYWxzYCBpcyB0aGUgYWN0dWFsIHJhdGUsIHdoZXJlIGBkZWNpbWFsc2AgY29tZXMKZnJvbSB0aGUgb3JhY2xlIGNvbnRyYWN0J3Mgb3duIGBkZWNpbWFscygpYCAoMTQsIGZvciBSZWZsZWN0b3IncwpmaWF0IGV4Y2hhbmdlIHJhdGUgb3JhY2xlIGFzIG9mIHRoaXMgd3JpdGluZykg4oCUIG5vdCBkdXBsaWNhdGVkCmludG8gYE9yYWNsZUNvbmZpZ2AsIHNpbmNlIGl0J3MgYSBwcm9wZXJ0eSBvZiB0aGUgb3JhY2xlLCBub3Qgb2YKdGhpcyBjb21taXRtZW50LCBhbmQgY2FsbGVycyBjYW4gcmVhZCBpdCBkaXJlY3RseSBvZmYgdGhlIG9yYWNsZQpjb250cmFjdCBpZiB0aGV5IG5lZWQgaXQuAAAAAAAFcHJpY2UAAAAAAAALAAAAvlRoZSBsZWRnZXIgdGltZXN0YW1wIHRoZSBvcmFjbGUgcmVjb3JkZWQgdGhpcyBxdW90ZSBhdCDigJQgYWxyZWFkeQpjaGVja2VkIGFnYWluc3QgYG1heF9hZ2Vfc2Vjc2AgYnkgdGhlIHRpbWUgdGhpcyBpcyByZXR1cm5lZCwgYnV0CnN1cmZhY2VkIGFueXdheSBzbyBhIGNhbGxlciBjYW4gZGlzcGxheSAiYXMgb2YiIGZyZXNobmVzcy4AAAAAAAl0aW1lc3RhbXAAAAAAAAAG",
        "AAAAAQAAANVXaGljaCBSZWZsZWN0b3Igb3JhY2xlIGluc3RhbmNlIGFuZCBhc3NldCBzeW1ib2wgYG9yYWNsZV9yYXRlYCBhbmQKYHJlc29sdmVfZnhfc2hvcnRmYWxsYCByZWFkIGZyb20sIGhvdyBvbGQgYSBxdW90ZSB0aGV5J2xsIGFjY2VwdCwgYW5kCndoYXQgdGhlIGRlYWwgaXMgYWN0dWFsbHkgd29ydGggaW4gdGhhdCBjdXJyZW5jeS4gU2V0IG9uY2UsIGF0CmBpbml0aWFsaXplYC4AAAAAAAAAAAAADE9yYWNsZUNvbmZpZwAAAAQAAAPiVGhlIGRlYWwncyB0cnVlIHZhbHVlIGluIGBwcmljZV9hc3NldGAsIGluIHRoZSBzYW1lICJzbWFsbGVzdAp1bml0LCA3IGRlY2ltYWwgcGxhY2VzIiBjb252ZW50aW9uIGB0b3RhbF9hbW91bnRgIGFscmVhZHkgdXNlcwpmb3IgdGhlIHNldHRsZW1lbnQgdG9rZW4gKHNvIGUuZy4gMSBOR04gPT0gMTBfMDAwXzAwMCB1bml0cyBoZXJlLAptYXRjaGluZyBob3cgMSBYTE0gPT0gMTBfMDAwXzAwMCBzdHJvb3BzKSDigJQgKipub3QqKiB0aGUgc2FtZQpudW1iZXIgYXMgYHRvdGFsX2Ftb3VudGAsIHdoaWNoIGlzIGBkZW5vbWluYXRlZF9hbW91bnRgJ3MgdmFsdWUKKmF0IHdoYXRldmVyIHJhdGUgaGFwcGVuZWQgdG8gaG9sZCB3aGVuIHRoZSBkZWFsIHdhcyBmdW5kZWQqLgpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgIHJlcHJpY2VzIHRoaXMgYXQgYSBmcmVzaCByYXRlOyB0aGUgZ2FwCmJldHdlZW4gdGhhdCBhbmQgd2hhdCdzIGFjdHVhbGx5IGVzY3Jvd2VkIGlzIFBSRCDCpzQuMidzIEZYIHJpc2ssCm1ha2UgY29uY3JldGUuIFVzaW5nIHRoZSBzYW1lIDctZGVjaW1hbCBjb252ZW50aW9uIG9uIGJvdGggc2lkZXMKb2YgdGhlIG11bHRpcGxpY2F0aW9uIGlzIHdoYXQgbGV0cyBgcmVzb2x2ZV9meF9zaG9ydGZhbGxgJ3MKY29udmVyc2lvbiBmb3JtdWxhIHNraXAgYSBzZXBhcmF0ZSB1bml0LW5vcm1hbGl6YXRpb24gc3RlcCDigJQgc2VlCml0cyBkb2MgY29tbWVudCBmb3IgdGhlIHdvcmtlZCBleGFtcGxlLgoKKipOb3QqKiB0aGUgb3JhY2xlJ3MgYGRlY2ltYWxzKClgIOKAlCB0aGF0IGlzIGEgcHJvcGVydHkgb2YgdGhlCm9yYWNsZSwgcmVhZCBsaXZlIGF0IHNldHRsZW1lbnQgYW5kIGFzc3VtZWQgaW1tdXRhYmxlIChzZWUKYGRvY3MvcmVmbGVjdG9yLWRlY2ltYWxzLWFzc3VtcHRpb24ubWRgLCBpc3N1ZSAjNjMpOyB0aGlzIGZpZWxkIGlzCnRoZSBkZWFsJ3MgdmFsdWUgaW4gdGhlIHNhbWUgNy1kZWNpbWFsIHVuaXRzIGFzIGB0b3RhbF9hbW91bnRgLgAAAAAAEmRlbm9taW5hdGVkX2Ftb3VudAAAAAAACwAAAHVNYXhpbXVtIGFnZSwgaW4gc2Vjb25kcywgYSBxdW90ZSBtYXkgaGF2ZSBhbmQgc3RpbGwgYmUgYWNjZXB0ZWQg4oCUClBSRCDCpzE2LjMncyBvcmFjbGUgc3RhbGVuZXNzIGJvdW5kLiBNdXN0IGJlID4gMC4AAAAAAAAMbWF4X2FnZV9zZWNzAAAABgAAAKtUaGUgUmVmbGVjdG9yIG9yYWNsZSBjb250cmFjdCB0byBjYWxsIOKAlCBlLmcuIHRoZSB0ZXN0bmV0CiJGaWF0IGV4Y2hhbmdlIHJhdGVzIiBpbnN0YW5jZSwgYENDU1NPSFRCTDNMRVdVQ0JCRUI1TkpGQzJPS0ZSQzc0T1dFSUpJWkxSSkJHQUFVNFZNVTVOVjRXYAooc2VlIGByZWZsZWN0b3IucnNgKS4AAAAAD29yYWNsZV9jb250cmFjdAAAAAATAAAAwVRoZSBhc3NldCBzeW1ib2wgdG8gcXVvdGUgYWdhaW5zdCB0aGF0IG9yYWNsZSdzIGBiYXNlKClgIOKAlCBlLmcuCmBOR05gLiBBbHdheXMgcXVlcmllZCBhcyBgUmVmbGVjdG9yQXNzZXQ6Ok90aGVyYCwgc2luY2UgdGhpcwpjb250cmFjdCdzIHVzZSBjYXNlIGlzIGZpYXQvRlggc3ltYm9scywgbm90IFN0ZWxsYXItbmF0aXZlIGFzc2V0cy4AAAAAAAALcHJpY2VfYXNzZXQAAAAAEQ==",
        "AAAABQAABABQdWJsaXNoZWQgb24gZXZlcnkgcmVhbCBzdGF0dXMgdHJhbnNpdGlvbiDigJQgYGxvY2tgLCBib3RoIGFkdmFuY2UKcmVsZWFzZXMsIGNoZWNrcG9pbnQsIGRlbGl2ZXJ5IGNvbmZpcm1hdGlvbiwgc2V0dGxlbWVudCwgY2FuY2VsbGF0aW9uLApib3RoIGRlZmF1bHQgcGF0aHMsIGFuZCB0aGUgZGlzcHV0ZSBmcmVlemUvcmVzdW1lIHBhaXIuIEV2ZXJ5IGVtaXNzaW9uCnBvaW50IGlzIGBzZXRfc3RhdHVzYCAodGhlIG9ubHkgcGxhY2UgYHN0YXR1c2AgaXMgYXNzaWduZWQgYWZ0ZXIKYGluaXRpYWxpemVgKSwgc28gYSBmdXR1cmUgdHJhbnNpdGlvbiBnZXRzIGFuIGV2ZW50IGJ5IGNvbnN0cnVjdGlvbgpyYXRoZXIgdGhhbiBieSByZW1lbWJlcmluZyB0byBhZGQgb25lLgoKQm90aCBzdGF0dXNlcyByaWRlIGFzIHRvcGljcywgaW4gdGhlIHNoYXBlIFNFUC00MSdzIGB0cmFuc2ZlcmAgdXNlcwooaW5kZXhlZCBkaW1lbnNpb25zIGZpcnN0LCBwYXlsb2FkIHNlY29uZCk6IGFuIG9mZi1jaGFpbiBjb25zdW1lciBjYW4KdG9waWMtZmlsdGVyIGZvciBhIHNwZWNpZmljIHRyYW5zaXRpb24g4oCUIGUuZy4gZXZlcnl0aGluZyB0aGF0IGp1c3QKYmVjYW1lIGBEZWZhdWx0ZWRgIOKAlCBpbnN0ZWFkIG9mIGZldGNoaW5nIGV2ZXJ5IGV2ZW50IGFuZCBkZWNvZGluZyBkYXRhLgpgaW5pdGlhbGl6ZWAgZGVsaWJlcmF0ZWx5IGRvZXNuJ3QgZW1pdDogYSBjb21taXRtZW50IGJlaW5nIGNyZWF0ZWQgaW4KYERyYWZ0YCBpcyBhbiBpbml0aWFsIHN0YXRlLCBub3QgYSB0cmFuc2l0aW9uIGludG8gb25lLgoKQWRkZWQgZm9yICM2OSwgd2hpY2ggYXNrZWQgd2hldGhlciBhbiBpbmRleGVyIHdvdWxkIGJlbmVmaXQ6IG5vbmUKZXhpc3RzIHlldCwgYnV0IHRoZSBBUEkncyBjYWNoZSBpcyByZWFkLXRyaWdnZXJlZCBwcmVjaXNlbHkgYmVjYXVzZQp0aGVyZSBpcyBubyBjaGVhcGVyIHdheSB0byBsZWFybiBhYm91dCBhIHN0YXRlIGNoYW5nZSAoc2VlCmBhcGkvSEFORE9GRi5tZGApLCBhbmQgdGhpcyBpcyB0aGUgcHJpbWl0AAAAAAAAAA1TdGF0dXNDaGFuZ2VkAAAAAAAAAQAAAA5zdGF0dXNfY2hhbmdlZAAAAAAAAgAAAAAAAAAEZnJvbQAAB9AAAAAGU3RhdHVzAAAAAAABAAAAAAAAAAJ0bwAAAAAH0AAAAAZTdGF0dXMAAAAAAAEAAAAC",
        "AAAAAQAAAmNPbmUgZmFybWVyIG1lbWJlcidzIHJlY29yZGVkIHNoYXJlIG9mIGEgY29tbWl0bWVudCwgY2FwdHVyZWQgYnkKYHNldF9hbGxvY2F0aW9uYC4gYG1lbWJlcl9oYXNoYCBpcyBhIHBlci1tZW1iZXIgc2FsdGVkIGhhc2gg4oCUIG5ldmVyIGEKYmFyZSBwaG9uZSBudW1iZXIgb3Igb3RoZXIgUElJIOKAlCBjb21wdXRlZCBvZmYtY2hhaW4gKEFQSSkgYXMKYEhNQUMtU0hBMjU2KHNhbHQsIHBob25lX251bWJlcilgIHdpdGggYSBmcmVzaCByYW5kb20gc2FsdCBwZXIKbWVtYmVyLiBUaGUgc2FsdCBhbmQgdGhlIHBob25lLW51bWJlciBtYXBwaW5nIGxpdmUgb25seSBpbiB0aGUgQVBJJ3MKUG9zdGdyZXMsIHdoaWNoIGlzIHdoYXQgbWFrZXMgdGhpcyBnZW51aW5lbHkgZXJhc2FibGUgcGVyIE5EUEEgcy4zNDoKZGVsZXRlIHRoZSBvZmYtY2hhaW4gcm93IGFuZCB0aGlzIGhhc2ggYmVjb21lcyBwZXJtYW5lbnRseQp1bmxpbmthYmxlIHRvIGEgcmVhbCBwZXJzb24sIHNpbmNlIHRoZSBzYWx0IGlzIGdvbmUgYW5kIGJydXRlLQpmb3JjaW5nIGEgcmFuZG9tIHNhbHQgaXMgaW5mZWFzaWJsZSByZWdhcmRsZXNzIG9mIGhvdyBzbWFsbCB0aGUKcGhvbmUtbnVtYmVyIGtleXNwYWNlIGlzLgAAAAAAAAAAEEFsbG9jYXRpb25NZW1iZXIAAAACAAAAAAAAAAttZW1iZXJfaGFzaAAAAAPuAAAAIAAAAL1CYXNpcyBwb2ludHMgb2YgdGhlIGNvbW1pdG1lbnQncyB0b3RhbCBwYXlvdXQgdGhpcyBtZW1iZXIgaXMKZW50aXRsZWQgdG8sIG9mZi1jaGFpbi4gUmVjb3JkLW9ubHkgaW4gdjEg4oCUIHNlZSBgc2V0X2FsbG9jYXRpb25gJ3MKZG9jIGNvbW1lbnQgZm9yIHdoeSBgc2V0dGxlYCBkb2Vzbid0IHByby1yYXRlIGFnYWluc3QgdGhpcy4AAAAAAAAJc2hhcmVfYnBzAAAAAAAABA==",
        "AAAAAAAAAzREcmFmdCAtPiBMb2NrZWQuIFB1bGxzIHRoZSBidXllcidzICoqZGVwb3NpdCoqIGludG8gdGhlIGNvbnRyYWN0IOKAlApgYWR2YW5jZTFfYnBzICsgYWR2YW5jZTJfYnBzYCBvZiBgdG90YWxfYW1vdW50YCwgbm90IHRoZSBmdWxsCmFtb3VudC4gU2VlIG1vZHVsZSBkb2NzIGZvciB3aHkgdGhpcyBjaGFuZ2VkIGZyb20gZWFybGllciB2ZXJzaW9ucwpvZiB0aGlzIGNvbnRyYWN0LCB3aGljaCBlc2Nyb3dlZCBldmVyeXRoaW5nIGhlcmUuCgpObyBwcmlvciBTRVAtNDEgYGFwcHJvdmUoKWAgbmVlZGVkIOKAlCB2ZXJpZmllZCBsaXZlIGFnYWluc3QgcmVhbAp0ZXN0bmV0LCBub3QgYXNzdW1lZC4gU29yb2JhbidzIHNpbmdsZS10cmFuc2FjdGlvbiBuZXN0ZWQtCmF1dGhvcml6YXRpb24gbW9kZWwgbWVhbnMgdGhlIGJ1eWVyJ3Mgc2lnbmF0dXJlIG9uIHRoZSB0b3AtbGV2ZWwKYGxvY2tgIGludm9jYXRpb24gYWxyZWFkeSBjb3ZlcnMgdGhlIG5lc3RlZCBgdG9rZW46OkNsaWVudDo6dHJhbnNmZXJgCmNhbGwncyBvd24gYHJlcXVpcmVfYXV0aCgpYCBiZWxvdzsgdGhpcyBkaWZmZXJzIGZyb20gdGhlIGNsYXNzaWMKRVJDLTIwLXN0eWxlIGFwcHJvdmUvdHJhbnNmZXJGcm9tIHBhdHRlcm4gYSByZWFkZXIgY29taW5nIGZyb20gRVZNCm1pZ2h0IGV4cGVjdC4gKEFuIGVhcmxpZXIgdmVyc2lvbiBvZiB0aGlzIGNvbW1lbnQgY2xhaW1lZCBhcHByb3ZlCndhcyByZXF1aXJlZCDigJQgdGhhdCB3YXMgbmV2ZXIgY2hlY2tlZCBhZ2FpbnN0IHJlYWwgYmVoYXZpb3I7IHRoaXMKcmVwbGFjZXMgaXQgd2l0aCB3aGF0IHdhcyBhY3R1YWxseSB2ZXJpZmllZC4pAAAABGxvY2sAAAAAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAABABNdXR1YWwgY2FuY2VsbGF0aW9uIOKAlCBQUkQgwqc3OiAiRGVmaW5lZCB1bndpbmQ6IGFkdmFuY2Ugc2V0dGxlZCBwZXIKYWdyZWVkIHNjaGVkdWxlLCByZW1haW5pbmcgZXNjcm93IHJldHVybmVkLCBubyBwZW5hbHR5LCBsb2dnZWQuIgpBbGxvd2VkIGZyb20gYW55IHByZS1kZWxpdmVyeSBzdGF0ZSAoYERyYWZ0YCB0aHJvdWdoCmBSZWFkeUZvckRlbGl2ZXJ5YCkuIE5vdCBmcm9tIGBEZWxpdmVyZWRgIG9ud2FyZCDigJQgYXQgdGhhdCBwb2ludApgc2V0dGxlYCBpcyB0aGUgY29ycmVjdCBwYXRoLCBhbiB1bndpbmQgZG9lc24ndCBhcHBseSBhbnltb3JlLgoKUmVxdWlyZXMgKipib3RoKiogdGhlIGJ1eWVyJ3MgYW5kIHRoZSBjb29wZXJhdGl2ZSdzIGF1dGggaW4gdGhlIHNhbWUKY2FsbCwgc2luY2UgdGhpcyBpcyBtdXR1YWwsIG5vdCB1bmlsYXRlcmFsIOKAlCB1bmxpa2UgYHJlY2xhaW1fKmAsCndoaWNoIGlzIHRoZSBidXllcidzIHVuaWxhdGVyYWwgcmlnaHQgb25jZSBhIGNsYWltIHdpbmRvdyBsYXBzZXMgb24KaXRzIG93bi4KCiJBZHZhbmNlIHNldHRsZWQgcGVyIGFncmVlZCBzY2hlZHVsZSwgbm8gcGVuYWx0eSI6IHdoYXRldmVyJ3MKYWxyZWFkeSBiZWVuIGNsYWltZWQgc3RheXMgd2l0aCB0aGUgY29vcGVyYXRpdmUg4oCUIHRoaXMgZG9lc24ndCBjbGF3CmFueXRoaW5nIGJhY2suICJSZW1haW5pbmcgZXNjcm93IHJldHVybmVkIjogd2hhdGV2ZXIncyBzdGlsbCBpbiB0aGUKY29udHJhY3QgKHplcm8sIGlmIG5vdGhpbmcgd2FzIGV2ZXIgbG9ja2VkKSBnb2VzIGJhY2sgdG8gdGhlIGJ1eWVyLAp2aWEgdGhlIHNhbWUgYmFsYW5jZS1iYXNlZCB0cmFuc2ZlciBgc2V0dGxlYCB1c2VzLCBzbyBpdCdzIGNvcnJlY3QKcmVnYXJkbGVzcyBvZiBjbGFpbS9yZWNsYWltIGhpc3Rvcnkgb3IgaG93IG11Y2ggb2YgdGhlIGRlcG9zaXQgdnMuCnJlbWFpbmRlciBoYWQgYmVlbiBmdW5kZWQuICJMb2dnZWQiOiB0aGUgc3RhdGUgdHJhbnNpdGlvbiBhbmQgdGhlCnRyYW5zZmVyIGJvdGggbGFuZCBvbiB0AAAABmNhbmNlbAAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAABABEZWxpdmVyZWQgLT4gU2V0dGxlZC4gU3BsaXRzIHRoZSBjb250cmFjdCdzIHJlbWFpbmluZyB0b2tlbiBiYWxhbmNlCih0aGUgZnVuZGVkIHJlbWFpbmRlciDigJQgYWR2YW5jZXMgYWxyZWFkeSBsZWZ0IHRoZSBjb250cmFjdCB2aWEKYGNsYWltX3RyYW5jaGVgL2ByZWNsYWltX3RyYW5jaGVgIGJ5IHRoaXMgcG9pbnQpIGJldHdlZW4gdGhlCmNvb3BlcmF0aXZlIGFuZCBhIHNob3J0ZmFsbCByZWZ1bmQgdG8gdGhlIGJ1eWVyLCBwZXIKYGNvbmZpcm1fZGVsaXZlcnlgJ3MgYHNldHRsZW1lbnRfYnBzYC4KCiJBZHZhbmNlIG5vdCBjbGF3ZWQgYmFjayIgKFBSRCDCpzcsIHBhcnRpYWwgZGVsaXZlcnkpOiB3aGF0ZXZlciB0aGUKY29vcGVyYXRpdmUgYWxyZWFkeSBjbGFpbWVkIHZpYSBgY2xhaW1fYWR2YW5jZV8xYC9gY2xhaW1fYWR2YW5jZV8yYApzdGF5cyB0aGVpcnMgcmVnYXJkbGVzcyBvZiB0aGUgZmluYWwgYHNldHRsZW1lbnRfYnBzYCDigJQgdGhpcyBvbmx5CmFkanVzdHMgdGhlICpyZW1haW5kZXIqIHBheW1lbnQuIGBhZGp1c3RlZF90b3RhbCA9IHRvdGFsX2Ftb3VudCAqCnNldHRsZW1lbnRfYnBzIC8gMTBfMDAwYCBpcyB0aGUgZnVsbCBjb250cmFjdCB2YWx1ZSB0aGUgZGVsaXZlcnkKYWN0dWFsbHkgZWFybmVkOyBzdWJ0cmFjdGluZyB3aGF0IHRoZSBjb29wZXJhdGl2ZSBhbHJlYWR5IGNsYWltZWQKZ2l2ZXMgd2hhdCdzIHN0aWxsIG93ZWQgdG8gdGhlbSBmcm9tIHRoZSByZW1haW5kZXIgKGZsb29yZWQgYXQKemVybywgc2luY2UgY2xhaW1lZCBhZHZhbmNlcyBhcmUgbmV2ZXIgY2xhd2VkIGJhY2sgZXZlbiBpZiBhIHNldmVyZQpzaG9ydGZhbGwgbWVhbnMgdGhleSB0ZWNobmljYWxseSBvdmVyLWVhcm5lZCBhbHJlYWR5KS4gV2hhdGV2ZXIgb2YKdGhlIHJlbWFpbmRlciBiYWxhbmNlIGlzbid0IG93ZWQgZ29lcyBiYWNrIHRvIHRoZSBidXllciBhcyBhCnNob3J0ZmFsbCByZWZ1bmQuCgoqKlJlcXVpcmVzIGJvdGggYWR2YW5jZSB0cmFuY2hlcyBhbHJlYWR5IHJlc29sdmVkKiogKGVhY2ggAAAABnNldHRsZQAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAADhObyBhdXRoIHJlcXVpcmVkIOKAlCByZWFkLW9ubHkgdmlldywgY2FsbGFibGUgYnkgYW55b25lLgAAAApnZXRfc3RhdHVzAAAAAAAAAAAAAQAAA+kAAAfQAAAABlN0YXR1cwAAAAAAAw==",
        "AAAAAAAABABDcmVhdGVzIHRoZSBjb21taXRtZW50IGluIGBEcmFmdGAuIE5vIGZ1bmRzIG1vdmUgeWV0LgoKUmVxdWlyZXMgdGhlIGJ1eWVyJ3MgYXV0aCBiZWNhdXNlIHRoZXkncmUgdGhlIG9uZSBjb21taXR0aW5nIHRvIGEKZnV0dXJlIGRlcG9zaXQg4oCUIHRoZSBjb29wZXJhdGl2ZSBhbmQgd2FyZWhvdXNlIG9wZXJhdG9yIGRvbid0IG5lZWQKdG8gY28tc2lnbiBjb250cmFjdCBjcmVhdGlvbiBpdHNlbGYsIG9ubHkgdGhlIHN0YXRlIHRyYW5zaXRpb25zCnRoYXQgbWF0dGVyIHRvIHRoZW0gKGBtYXJrX2NoZWNrcG9pbnRgLCBgY29uZmlybV9kZWxpdmVyeWApLgoKVGhlIHRocmVlIHBhcnR5IGFkZHJlc3NlcyBhcmUgKm5vdCogcmVxdWlyZWQgdG8gYmUgZGlzdGluY3QsIG9uCnB1cnBvc2UuIFJvbGUgc2VwYXJhdGlvbiBpcyBlbmZvcmNlZCB3aGVyZSBpdCBtYXR0ZXJzIOKAlCBhdCBlYWNoCnN0YXRlIHRyYW5zaXRpb24sIGJ5IHRoYXQgcm9sZSdzIG93biBgcmVxdWlyZV9hdXRoKClgIOKAlCByYXRoZXIgdGhhbgpieSBhIGRpc3RpbmN0bmVzcyBjaGVjayBoZXJlLiBDb2xsYXBzaW5nIGJ1eWVyLCBjb29wZXJhdGl2ZSBhbmQKd2FyZWhvdXNlIG9udG8gb25lIHByaW5jaXBhbCBpcyBhIGxlZ2l0aW1hdGUgZGVwbG95bWVudCBjaG9pY2UgKGEKY29vcGVyYXRpdmUgdGhhdCBhbHNvIHJ1bnMgdGhlIHdhcmVob3VzZSwgb3IgYSBtaW5pbWFsIHNpbmdsZS1zaWduZXIKdmFsaWRhdGlvbiBkZXBsb3ltZW50KTsgYGluaXRpYWxpemVgIHJlY29yZHMgdGhlIHBhcnRpZXMgaXQgaXMgaGFuZGVkCmFuZCBkb2VzIG5vdCBzZWNvbmQtZ3Vlc3MgdGhlbS4gVGhlIGNvbnNlcXVlbmNlIGlzIGFjY2VwdGVkLCBub3QKaGlkZGVuOiB3aXRoIG9uZSBwcmluY2lwYWwgaW4gYWxsIHRocmVlIHJvbGVzIGV2ZXJ5IG11bHRpLXBhcnR5CmNvbnNlbnQgZ2F0ZSByZWR1Y2VzIHRvIHRoYXQgc2luZ2xlIHNpZ25lci4gU2VlIHRoZSB0ZXN0cwpgaW5pdGlhbGl6ZV9hbGxvd3NfYnV5ZXJfY29vcGVyYXRpdmVfYW5kX3dhcmVob3VzZV90AAAACmluaXRpYWxpemUAAAAAAA0AAAAAAAAABWJ1eWVyAAAAAAAAEwAAAAAAAAALY29vcGVyYXRpdmUAAAAAEwAAAAAAAAASd2FyZWhvdXNlX29wZXJhdG9yAAAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAADHRvdGFsX2Ftb3VudAAAAAsAAAAAAAAADGFkdmFuY2UxX2JwcwAAAAQAAAAAAAAADGFkdmFuY2UyX2JwcwAAAAQAAAAAAAAAEWNsYWltX3dpbmRvd19zZWNzAAAAAAAABgAAAAAAAAAVcmVtYWluZGVyX3dpbmRvd19zZWNzAAAAAAAABgAAAAAAAAAUZGVsaXZlcnlfd2luZG93X3NlY3MAAAAGAAAAAAAAABNjb250cmFjdGVkX3F1YW50aXR5AAAAAAQAAAAAAAAAD2dyYWRlX3ByaWNlX2JwcwAAAAPqAAAABAAAAAAAAAANb3JhY2xlX2NvbmZpZwAAAAAAA+gAAAfQAAAADE9yYWNsZUNvbmZpZwAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAARFSZWFkcyB0aGUgY3VycmVudCByYXRlIGZvciB0aGlzIGNvbW1pdG1lbnQncyBjb25maWd1cmVkIG9yYWNsZQphc3NldCwgZW5mb3JjaW5nIHRoZSBzdGFsZW5lc3MgYm91bmQuIEEgcHVyZSByZWFkIGFnYWluc3QKUmVmbGVjdG9yJ3Mgb3duIHN0b3JlZCBoaXN0b3J5LCBub3QgYSBzdGF0ZSB0cmFuc2l0aW9uIOKAlCBjYWxsYWJsZQppbiBhbnkgc3RhdHVzLCBpbmNsdWRpbmcgYWZ0ZXIgYFNldHRsZWRgLgoKTm8gYXV0aCByZXF1aXJlZCDigJQgcGVybWlzc2lvbmxlc3MgcmVhZC4AAAAAAAALb3JhY2xlX3JhdGUAAAAAAAAAAAEAAAPpAAAH0AAAAApPcmFjbGVSYXRlAAAAAAAD",
        "AAAAAAAABABEaXNwdXRlIGZsYWdnaW5nIChQUkQncyBtdXN0LWhhdmUgImRpc3B1dGUgZmxhZ2dpbmcgd2l0aCBkZWZpbmVkCmVzY2FsYXRpb24iKS4gQW55ICoqb25lKiogb2YgdGhlIHRocmVlIG5hbWVkIHBhcnRpZXMg4oCUIGBmbGFnZ2VyYAptdXN0IGVxdWFsIGBidXllcmAsIGBjb29wZXJhdGl2ZWAsIG9yIGB3YXJlaG91c2Vfb3BlcmF0b3JgIOKAlCBjYW4KZnJlZXplIHRoZSBjb21taXRtZW50IGJ5IG1vdmluZyBpdCB0byBgU3RhdHVzOjpEaXNwdXRlZGAsIHdoaWNoCmJsb2NrcyBldmVyeSBvdGhlciBzdGF0ZS1jaGFuZ2luZyBjYWxsIGJ5IGNvbnN0cnVjdGlvbjogZXZlcnkgb25lCm9mIHRoZW0gbWF0Y2hlcyBhIHNwZWNpZmljIHJlcXVpcmVkIHN0YXR1cyAob3Igc2V0IG9mIHN0YXR1c2VzKQphbmQgYERpc3B1dGVkYCBpcyBuZXZlciBhbW9uZyB0aGVtLgoKVGhpcyBjb250cmFjdCBzdGlsbCBkb2Vzbid0IGFyYml0cmF0ZSAqd2hhdCogdGhlIGRpc3B1dGUgaXMgYWJvdXQK4oCUIHNlZSB0aGUgbW9kdWxlIGRvYydzIGV4cGxhbmF0aW9uIG9mIHdoeSB0aGF0J3MgYSBkZWxpYmVyYXRlCm5vbi1nb2FsLCBub3QgYW4gb3ZlcnNpZ2h0LiBXaGF0IHRoaXMgYnV5cyBpcyB0aGUgImZsYWdnaW5nIiBoYWxmCm9ubHk6IGEgdW5pbGF0ZXJhbCBwYXVzZSwgc28gYSBjb250ZXN0ZWQgc2l0dWF0aW9uIGRvZXNuJ3QgbGV0CnNvbWUgb3RoZXIgZGVhZGxpbmUtdHJpZ2dlcmVkIGZ1bmN0aW9uIChlLmcuIGBzZXR0bGVgLApgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgKSBydW4gdG8gYSBjb25jbHVzaW9uIHdoaWxlIGl0J3MgYmVpbmcKc29ydGVkIG91dCBvZmYtY2hhaW4uIFJlYWNoYWJsZSBmcm9tIGFueSBzdGF0ZSB3aXRoIGZ1bmRzIGFscmVhZHkKYXQgc3Rha2Ugb3IgYSBjbGFpbSBvdXRzdGFuZGluZyAoYExvY2tlZGAgdGhyb3VnaCBgRGVsaXZlcmVkYCkg4oCUCm5vdCBgRHJhZnRgIChub3RoaW5nIGVzY3Jvd2VkIHlldCB0byBmcmVlemUpIGFuZCBub3QgYW55IHRlcm1pbmFsCnN0YXR1cyBvciBgRGlzcHV0AAAADGZsYWdfZGlzcHV0ZQAAAAEAAAAAAAAAB2ZsYWdnZXIAAAAAEwAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAmlCdXllciBlc2Nyb3dzIGB0b3RhbF9hbW91bnQgLSBkZXBvc2l0YCDigJQgdGhlIHNlY29uZCBoYWxmIG9mCnR3by1waGFzZSBmdW5kaW5nLiBNdXN0IGhhcHBlbiB3aXRoaW4gdGhlIHdpbmRvdwpgcmVhZHlfZm9yX2RlbGl2ZXJ5YCBvcGVuZWQsIG9yIGl0J3MgYSBkZWZhdWx0IChzZWUKYGV4cGlyZV9yZW1haW5kZXJfd2luZG93YCkuIFJlcXVpcmVzIGJ1eWVyIGF1dGgg4oCUIHRoaXMgaXMgbmV3Cm1vbmV5IHRoZSBidXllciBzcGVjaWZpY2FsbHkgb3dlcy4KClRoZSB3aW5kb3cncyBlbmQgaXMgKippbmNsdXNpdmUgb2YgYHJlbWFpbmRlcl9kZWFkbGluZWAgaXRzZWxmKio6CmBub3cgPiByZW1haW5kZXJfZGVhZGxpbmVgIGlzIHRoZSBvbmx5IHJlamVjdGlvbiwgc28gYSBwYXltZW50CmxhbmRpbmcgaW4gdGhlIGRlYWRsaW5lIHNlY29uZCBzdGlsbCBjb3VudHMuCmBleHBpcmVfcmVtYWluZGVyX3dpbmRvd2AgbmVlZHMgc3RyaWN0bHkgbW9yZSB0aGFuIHRoZSBkZWFkbGluZSwKd2hpY2ggaXMgd2hhdCBrZWVwcyB0aGUgYm91bmRhcnkgZnJvbSBiZWluZyBhbWJpZ3VvdXMg4oCUIHRoZSB0d28KY2FsbHMgYXJlIG5ldmVyIGJvdGggbGVnYWwgYXQgdGhlIHNhbWUgdGltZXN0YW1wLgAAAAAAAA5mdW5kX3JlbWFpbmRlcgAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAHZUaGUgcmVjb3JkZWQgYWxsb2NhdGlvbiBsZWRnZXIsIGlmIGBzZXRfYWxsb2NhdGlvbmAgaGFzIHJ1bi4KCk5vIGF1dGggcmVxdWlyZWQg4oCUIHJlYWQtb25seSB2aWV3LCBjYWxsYWJsZSBieSBhbnlvbmUuAAAAAAAOZ2V0X2FsbG9jYXRpb24AAAAAAAAAAAABAAAD6QAAA+oAAAfQAAAAEEFsbG9jYXRpb25NZW1iZXIAAAAD",
        "AAAAAAAAAEdObyBhdXRoIHJlcXVpcmVkIOKAlCByZWFkLW9ubHkgdmlldywgcmV0dXJucyB0aGUgZnVsbCBjb21taXRtZW50IHN0YXRlLgAAAAAOZ2V0X2NvbW1pdG1lbnQAAAAAAAAAAAABAAAD6QAAB9AAAAAKQ29tbWl0bWVudAAAAAAAAw==",
        "AAAAAAAABABCdXllci1wb3NpdGlvbiBhc3NpZ25hYmlsaXR5IChQUkQgwqc0LjgpOiB0cmFuc2ZlcnMgYGJ1eWVyYCB0bwpgbmV3X2J1eWVyYC4gRGVsaWJlcmF0ZWx5ICoqbm90KiogYSBtYXJrZXQg4oCUIHRoZXJlJ3Mgbm8gb3JkZXIgYm9vaywKbm8gbGlzdGluZywgbm8gb24tY2hhaW4gcHJpY2UgZGlzY292ZXJ5IGhlcmUsIGp1c3QgYSBub3ZhdGlvbiBvZgp3aG8gaG9sZHMgdGhlIHBvc2l0aW9uLiBObyBmdW5kcyBtb3ZlOyB0aGlzIG9ubHkgZXZlciByZXdyaXRlcyB3aG8KYGJ1eWVyYCByZWZlcnMgdG8gZm9yIGV2ZXJ5IGZ1dHVyZSBgcmVjbGFpbV8qYC9gY2FuY2VsYC8KYGZ1bmRfcmVtYWluZGVyYCBhdXRoIGNoZWNrLgoKUmVxdWlyZXMgKip0aHJlZSoqIHNpZ25hdHVyZXMgaW4gdGhlIHNhbWUgY2FsbCwgbm90IHR3bzogdGhlCmN1cnJlbnQgYnV5ZXIncyAodGhleSdyZSBnaXZpbmcgdXAgdGhlIHBvc2l0aW9uKSwgdGhlCmNvb3BlcmF0aXZlJ3MgKFBSRCdzIGV4cGxpY2l0ICJ3aXRoIGNvb3BlcmF0aXZlIGNvbnNlbnQiKSwgYW5kIHRoZQpuZXcgYnV5ZXIncy4gVGhhdCB0aGlyZCBvbmUgaXNuJ3QgbmFtZWQgaW4gdGhlIFBSRCBsaW5lIHRoaXMKaW1wbGVtZW50cywgYnV0IGxlYXZpbmcgaXQgb3V0IHdvdWxkIGxldCB0aGUgY3VycmVudCBidXllciBhbmQKY29vcGVyYXRpdmUgc2FkZGxlIGEgdGhpcmQgcGFydHkgd2l0aCBhIHBvc2l0aW9uIOKAlCBpbmNsdWRpbmcgaXRzCm9ibGlnYXRpb25zIOKAlCB3aXRob3V0IHRoYXQgcGFydHkgZXZlciBhZ3JlZWluZyB0byB0YWtlIGl0IG9uLiBUd28KY29uc2VudHMgd2hlcmUgdGhlIFBSRCBhc2tlZCBmb3IgdHdvIHdvdWxkIGJlIHRoZSBzbWFsbGVyIGNoYW5nZTsKdGhyZWUgaXMgdGhlIHNhZmVyIG9uZSwgYW5kIHNhZmV0eSB3aW5zIGhlcmUuCgpSZWFjaGFibGUgZnJvbSB0aGUgc2FtZSBzdGF0ZXMgYXMgYGNhbmNlbGAgKGBEcmFmdGAgdGhyb3VnaApgUmVhZHlGb3JEZWxpdmVyeWApIGFuZCBmb3IgdGhlIHNhbWUgcmVhc29uOiBwYXN0IGBEZWxpdmVyAAAADnJlYXNzaWduX2J1eWVyAAAAAAABAAAAAAAAAAluZXdfYnV5ZXIAAAAAAAATAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAABABDb29wZXJhdGl2ZS1hdXRob3JlZCByZWNvcmQgb2YgbWVtYmVyIGZhcm1lcnMnIGVudGl0bGVtZW50IHNoYXJlcwrigJQgUFJEJ3MgIm1lbWJlciBhbGxvY2F0aW9uIGxlZGdlciBjYXB0dXJlIGF0IGxvY2staW4uIiAqKlJlY29yZC0Kb25seSBpbiB2MSoqOiBgc2V0dGxlYCBzdGlsbCBwYXlzIHRoZSBjb29wZXJhdGl2ZSB3YWxsZXQgYSBsdW1wCnN1bTsgdGhpcyBkb2Vzbid0IHByby1yYXRlIGFuIG9uLWNoYWluIHBheW91dCBhY3Jvc3MgbWVtYmVycy4gVGhhdAptYXRjaGVzIFBSRCDCpzQuOSdzIG93biBzdGF0ZWQgdjEgZGVmYXVsdCAoVHJhbnNwYXJlbmN5IExhZGRlcgpSdW5nIDEg4oCUICJQYXltZW50IHNldHRsZXMgdG8gdGhlIGNvb3BlcmF0aXZlIHdhbGxldC4gRWFjaAptZW1iZXIncyBlbnRpdGxlbWVudCBpcyBvbiBjaGFpbiBhbmQgcmVhZGFibGU7IG1lbWJlcnMgcmVjZWl2ZSBhbgpTTVMgc3RhdGluZyB0aGVpciBzaGFyZSIpIOKAlCBub3QgYW4gb3BlbiBxdWVzdGlvbiB0aGlzIGNvbnRyYWN0CmRlZmVycmVkLCBidXQgdGhlIGRvY3VtZW50ZWQgZGVmYXVsdC4gUHJvLXJhdGVkIG9uLWNoYWluIHBheW91dAooUnVuZyAyKykgd291bGQgbmVlZCByZWFsIGRlc2lnbiB3b3JrIChOIHRva2VuIHRyYW5zZmVycyBpbnN0ZWFkCm9mIG9uZSwgcm91bmRpbmctcmVtYWluZGVyIGhhbmRsaW5nKSBhbmQgaXNuJ3QgYnVpbHQuCgpPbmUtdGltZSBhbmQgaW1tdXRhYmxlIG9uY2Ugc2V0IOKAlCBubyBhbWVuZCBmdW5jdGlvbiBleGlzdHMuCmBtZW1iZXJzYCBtdXN0IGJlIG5vbi1lbXB0eSwgbWF5IGhvbGQgYXQgbW9zdApgTUFYX0FMTE9DQVRJT05fTUVNQkVSU2AgZW50cmllcywgYW5kIHRoZSBzaGFyZXMgbXVzdCBzdW0gdG8gYXQKbW9zdCAxMF8wMDAgYnBzLgpEZWxpYmVyYXRlbHkgKipub3QqKiByZXF1aXJlZCBiZWZvcmUgYGxvY2tgOiBhIHNvbG8tZmFybWVyCmNvbW1pdG1lbnQgd2l0aCBubyBjb29wZXJhdGl2ZSBwb29saW5nIHNob3VsZG4ndCBiZSBmb3JjZWQgdGhyb3VnaAphIGxlZGdlAAAADnNldF9hbGxvY2F0aW9uAAAAAAABAAAAAAAAAAdtZW1iZXJzAAAAA+oAAAfQAAAAEEFsbG9jYXRpb25NZW1iZXIAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAVNDb29wZXJhdGl2ZSBjbGFpbXMgdHJhbmNoZSAxLCBpZiB3aXRoaW4gdGhlIHdpbmRvdy4gUmVxdWlyZXMKY29vcGVyYXRpdmUgYXV0aC4KCkJvdW5kYXJ5ICgjMTExKTogIndpdGhpbiIgaXMgaW5jbHVzaXZlIG9mIHRoZSBkZWFkbGluZSBpbnN0YW50IOKAlApgY2xhaW1fdHJhbmNoZWAgcmVqZWN0cyBvbmx5IHdoZW4gYHRpbWVzdGFtcCA+IGFkdmFuY2UxX2RlYWRsaW5lYCwgc28KYGFkdmFuY2UxX2RlYWRsaW5lYCBpdHNlbGYgaXMgc3RpbGwgY2xhaW1hYmxlLCBhbmQgaXQgaXMgdGhlIGxhc3QKaW5zdGFudCB0aGF0IGlzLiBQaW5uZWQgYnkgYHRlc3RzL2RlYWRsaW5lX2JvdW5kYXJ5LnJzYC4AAAAAD2NsYWltX2FkdmFuY2VfMQAAAAAAAAAAAQAAA+kAAAACAAAAAw==",
        "AAAAAAAAALNDb29wZXJhdGl2ZSBjbGFpbXMgdHJhbmNoZSAyLCBpZiB3aXRoaW4gdGhlIHdpbmRvdy4gUmVxdWlyZXMKY29vcGVyYXRpdmUgYXV0aCDigJQgc2FtZSBtZWNoYW5pY3MgYXMgYGNsYWltX2FkdmFuY2VfMWAsIGluY2x1ZGluZyBpdHMKaW5jbHVzaXZlIGBhZHZhbmNlMl9kZWFkbGluZWAgYm91bmRhcnkgKCMxMTEpLgAAAAAPY2xhaW1fYWR2YW5jZV8yAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAATVBZHZhbmNlMVJlbGVhc2VkIC0+IENoZWNrcG9pbnRQYXNzZWQuIFJlcXVpcmVzIHRoZSB3YXJlaG91c2UKb3BlcmF0b3IncyBhdHRlc3RhdGlvbiDigJQgdGhpcyBpcyBhIGp1ZGdtZW50IGNhbGwsIG5vdCBhIG1lY2hhbmljYWwKc3RhdGUgYWR2YW5jZSwgc28gdW5saWtlIG9wZW5pbmcgYW4gYWR2YW5jZSB0cmFuY2hlIGl0ICppcyoKYXV0aC1nYXRlZC4gSW5kZXBlbmRlbnQgb2Ygd2hldGhlciB0cmFuY2hlIDEgaGFzIGFjdHVhbGx5IGJlZW4KY2xhaW1lZCB5ZXQg4oCUIGNyb3AgcHJvZ3Jlc3MgZG9lc24ndCB3YWl0IG9uIHBhcGVyd29yay4AAAAAAAAPbWFya19jaGVja3BvaW50AAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAA71SZXNvbHZlcyBhIGRpc3B1dGUgYnkgdW5hbmltb3VzIGNvbnNlbnQg4oCUICoqYWxsIHRocmVlKiogbmFtZWQKcGFydGllcycgYXV0aCwgbm90IGEgbWFqb3JpdHkgb3IgYW55IHNpbmdsZSBvbmUsIHNpbmNlIHJlc3VtaW5nCmlzIHRoZSBtaXJyb3Igb2YgYGZsYWdfZGlzcHV0ZWAncyB1bmlsYXRlcmFsIGZyZWV6ZSBhbmQgc2hvdWxkbid0Cml0c2VsZiBiZSB1bmlsYXRlcmFsLiBSZXN0b3JlcyBgc3RhdHVzYCB0byBleGFjdGx5IHdoYXRldmVyIGl0CndhcyB0aGUgbW9tZW50IGBmbGFnX2Rpc3B1dGVgIHJhbiDigJQgdGhpcyBjb250cmFjdCBkb2Vzbid0IGRlY2lkZSwKb3IgbGV0IHRoZSBwYXJ0aWVzIHJlZGlyZWN0IGl0IHRvLCBhICpkaWZmZXJlbnQqIG91dGNvbWUgKGUuZy4Kc3RyYWlnaHQgdG8gYENhbmNlbGxlZGApIGZyb20gaW5zaWRlIHRoaXMgY2FsbDsgdGhhdCB3b3VsZCBiZQphcmJpdHJhdGlvbiBieSBhbm90aGVyIG5hbWUuIElmIHRoZSBwYXJ0aWVzJyBvZmYtY2hhaW4gcmVzb2x1dGlvbgppcyAidW53aW5kIHRoZSBkZWFsLCIgdGhleSBjYWxsIHRoZSBleGlzdGluZyBgY2FuY2VsKClgIG5leHQsCnNhbWUgYXMgdGhleSB3b3VsZCBoYXZlIHdpdGhvdXQgZXZlciBkaXNwdXRpbmcuCgoqKkRlYWRsaW5lLWZyZWUgb24gcHVycG9zZSoqOiB1bmxpa2UgYGV4cGlyZV9kaXNwdXRlX3dpbmRvd2AsIHRoaXMKY2FycmllcyBubyBgZGlzcHV0ZV9kZWFkbGluZWAgY2hlY2sgYXQgYWxsLCBzbyBpdCBzdGF5cyBjYWxsYWJsZSBpbgp0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiBhbmQgYWZ0ZXIgaXQuIFVuYW5pbW91cyBjb25zZW50IGlzIHRoZQpvbmx5IGdhdGUg4oCUIHRoZSB0aW1lb3V0IGV4aXN0cyB0byBicmVhayBhIGRlYWRsb2NrLCBuZXZlciB0byB0YWtlCnRoZSBkZWNpc2lvbiBhd2F5IGZyb20gdGhyZWUgcGFydGllcyB3aG8gaGF2ZSBhbHJlYWR5IGFncmVlZC4AAAAAAAAPcmVzb2x2ZV9kaXNwdXRlAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABSZWFkeUZvckRlbGl2ZXJ5IC0+IERlbGl2ZXJlZC4gV2FyZWhvdXNlLW9wZXJhdG9yLWF0dGVzdGVkLCBzYW1lCnJlYXNvbmluZyBhcyBgbWFya19jaGVja3BvaW50YC4gUmVxdWlyZXMgdGhlIHJlbWFpbmRlciB0byBhbHJlYWR5CmJlIGZ1bmRlZCDigJQgZGVsaXZlcnkgc2hvdWxkbid0IGJlIGNvbmZpcm1hYmxlIHdoaWxlIHRoZSBidXllcgpzdGlsbCBvd2VzIG1vbmV5IG9uIHRoZSBkZWFsLgoKQXBwbGllcyB0aGUgUFJEIMKnNyBzaG9ydGZhbGwvZ3JhZGUgYWRqdXN0bWVudCBzY2hlZHVsZTogYGRlbGl2ZXJlZF9xdWFudGl0eWAKaXMgY29tcGFyZWQgYWdhaW5zdCBgY29udHJhY3RlZF9xdWFudGl0eWAgKHByb3BvcnRpb25hbCwgY2FwcGVkIGF0CjEwMCUg4oCUIG92ZXItZGVsaXZlcnkgaXNuJ3QgcGFpZCBleHRyYSBpbiB2MSwgbWF0Y2hpbmcgImV4Y2VzcwpvdGhlcndpc2UgcmVsZWFzZWQgdG8gY29vcGVyYXRpdmUiIG9mZi1jaGFpbiksIGBncmFkZV9pbmRleGAgc2VsZWN0cwphbiBlbnRyeSBmcm9tIHRoZSBwcmUtYWdyZWVkIGBncmFkZV9wcmljZV9icHNgIHRhYmxlLCBhbmQgdGhlIHR3bwptdWx0aXBseSB0b2dldGhlciBpbnRvIGBzZXR0bGVtZW50X2Jwc2AsIHdoaWNoIGBzZXR0bGVgIHBheXMgb3V0CmFnYWluc3QuIFRoZSB3YXJlaG91c2Ugb3BlcmF0b3IncyBhdHRlc3RhdGlvbiBpcyBhdXRob3JpdGF0aXZlIGhlcmUK4oCUIGEgZ3JhZGUgb3IgcXVhbnRpdHkgZGlzcHV0ZSBpcyB0aGUgb3BlcmF0b3IncyBvd24gYXBwZWFscwpwcm9jZXNzLCBub3QgYSBIYXJ2ZXN0TG9jayBkaXNwdXRlIChQUkQncyBlZGdlLWNhc2UgdGFibGUpLgoKKipTdG9yYWdlIGFjY2VzcyBwYXR0ZXJuKio6ClJlYWRzIHRoZSBgQ29tbWl0bWVudGAgc3RydWN0IGV4YWN0bHkgb25jZSBmcm9tIGluc3RhbmNlIHN0b3JhZ2UgdXBvbgplbnRyeSB2aWEgYFNlbGY6OmxvYWRgLiBBbGwgdmFsaWRhdGlvbiBjaGVja3MsIHNob3J0ZmFsbCBzY2FsaW5nLCBhbmQKYHNldHRsZW1lbnRfYnBzYCBtdWx0aXBsaWVyIGNhAAAAEGNvbmZpcm1fZGVsaXZlcnkAAAACAAAAAAAAABJkZWxpdmVyZWRfcXVhbnRpdHkAAAAAAAQAAAAAAAAAC2dyYWRlX2luZGV4AAAAAAQAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAXpCdXllciBwYXlzIGluIHRoZSBzaG9ydGZhbGwgYHJlc29sdmVfZnhfc2hvcnRmYWxsYCBjb21wdXRlZC4KQnV5ZXItZ2F0ZWQsIG1hdGNoaW5nIGBmdW5kX3JlbWFpbmRlcmAncyBvd24gcmVhc29uaW5nOiB0aGlzIGlzCm5ldyBtb25leSB0aGUgYnV5ZXIgc3BlY2lmaWNhbGx5IG93ZXMsIG5vdCBhIG1lY2hhbmljYWwgc3RlcAphbnlvbmUgY291bGQgdHJpZ2dlci4KCkRlYWRsaW5lLSoqaW5jbHVzaXZlKiosIGV4YWN0bHkgbGlrZSBgZnVuZF9yZW1haW5kZXJgOgpgbm93ID4gZnhfc2hvcnRmYWxsX2RlYWRsaW5lYCBpcyB0aGUgb25seSByZWplY3Rpb24sIHNvIHRoZQpkZWFkbGluZSBzZWNvbmQgaXRzZWxmIGlzIHN0aWxsIHRoZSBidXllcidzIHRvIHBheSBpbi4AAAAAABFmdW5kX2Z4X3Nob3J0ZmFsbAAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAAGlUaGUgY29uZmlndXJlZCBvcmFjbGUsIGlmIGBpbml0aWFsaXplYCBzZXQgb25lLgoKTm8gYXV0aCByZXF1aXJlZCDigJQgcmVhZC1vbmx5IHZpZXcsIGNhbGxhYmxlIGJ5IGFueW9uZS4AAAAAAAARZ2V0X29yYWNsZV9jb25maWcAAAAAAAAAAAAAAQAAA+kAAAPoAAAH0AAAAAxPcmFjbGVDb25maWcAAAAD",
        "AAAAAAAAAZJCdXllciByZWNsYWltcyB0cmFuY2hlIDEsIGlmIHRoZSB3aW5kb3cgaGFzIHBhc3NlZCB1bmNsYWltZWQuClJlcXVpcmVzIGJ1eWVyIGF1dGguCgpCb3VuZGFyeSAoIzExMSk6IHRoZSBleGFjdCBjb21wbGVtZW50IG9mIGBjbGFpbV9hZHZhbmNlXzFgIOKAlApgcmVjbGFpbV90cmFuY2hlYCByZWplY3RzIHdoaWxlIGB0aW1lc3RhbXAgPD0gYWR2YW5jZTFfZGVhZGxpbmVgLCBzbwp0aGUgZGVhZGxpbmUgaW5zdGFudCBiZWxvbmdzIHRvIHRoZSBjb29wZXJhdGl2ZSdzIGNsYWltIGFuZCB0aGUgYnV5ZXIncwpyZWNsYWltIGZpcnN0IGJlY29tZXMgcG9zc2libGUgYXQgYGFkdmFuY2UxX2RlYWRsaW5lICsgMWAuIE5vIHRpbWVzdGFtcApldmVyIGFkbWl0cyBib3RoLCBhbmQgbm9uZSBhZG1pdHMgbmVpdGhlci4AAAAAABFyZWNsYWltX2FkdmFuY2VfMQAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAALlCdXllciByZWNsYWltcyB0cmFuY2hlIDIsIGlmIHRoZSB3aW5kb3cgaGFzIHBhc3NlZCB1bmNsYWltZWQuClJlcXVpcmVzIGJ1eWVyIGF1dGgg4oCUIHNhbWUgbWVjaGFuaWNzIGFzIGByZWNsYWltX2FkdmFuY2VfMWAsIGluY2x1ZGluZwp0aGUgZXhjbHVzaXZlIGBhZHZhbmNlMl9kZWFkbGluZWAgYm91bmRhcnkgKCMxMTEpLgAAAAAAABFyZWNsYWltX2FkdmFuY2VfMgAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABMb2NrZWQgLT4gQWR2YW5jZTFSZWxlYXNlZC4gT3BlbnMgdHJhbmNoZSAxJ3MgY2xhaW0gd2luZG93IOKAlCBkb2VzCioqbm90KiogbW92ZSBmdW5kcy4gVGhlIGNvb3BlcmF0aXZlIGNsYWltcyB2aWEgYGNsYWltX2FkdmFuY2VfMWAsCm9yIHRoZSBidXllciByZWNsYWltcyB2aWEgYHJlY2xhaW1fYWR2YW5jZV8xYCBhZnRlciB0aGUgd2luZG93CnBhc3NlcyB1bmNsYWltZWQuCgpEZWxpYmVyYXRlbHkgbm90IGF1dGgtZ2F0ZWQ6IGl0IHN0YXJ0cyBhIGNsb2NrLCBpdCBkb2Vzbid0IG1vdmUKbW9uZXkgb3IgZ3JhbnQgYW55b25lIGFueXRoaW5nIHRoZXkgd2VyZW4ndCBhbHJlYWR5IGVudGl0bGVkIHRvCnVuZGVyIHRoZSBhZ3JlZWQgdGVybXMuCgpTZWN1cml0eS1yZXZpZXdlZCBvbiB0aGUgcmVjb3JkICgjMik6IHRoYXQgcmVhc29uaW5nIGhvbGRzIHVuZGVyCmFkdmVyc2FyaWFsIHRpbWluZyDigJQgYSB0aGlyZCBwYXJ0eSBjYW4gcGljayB0aGUgaW5zdGFudCB0aGUgd2luZG93Cm9wZW5zIGJ1dCBnYWlucyBub3RoaW5nIGJ5IGl0IChubyBmdW5kcyBtb3ZlLCB0aGUgZGVhZGxpbmUgaXMKd3JpdHRlbiBleGFjdGx5IG9uY2Ugc28gdGhlIGNsb2NrIGNhbiBuZXZlciBiZSByZXN0YXJ0ZWQgb3IKZXh0ZW5kZWQsIHRoZSBsYXBzZSBiZW5lZmljaWFyeSBzdGF5cyB0aGUgYnV5ZXIsIGFuZCB0aGUKY29vcGVyYXRpdmUgYWx3YXlzIGdldHMgdGhlIGZ1bGwgYGNsYWltX3dpbmRvd19zZWNzYCBmcm9tIHdoZW5ldmVyCnRoZSB3aW5kb3cgb3BlbmVkKS4gVGhlIGZ1bGwgcmVhZGVyIGF1ZGl0IGFuZCB0aGUgcGlubmVkIHNjZW5hcmlvcwpsaXZlIGluIGBkb2NzL3Blcm1pc3Npb25sZXNzLXJlbGVhc2UtcmV2aWV3Lm1kYCBhbmQKYGNvbnRyYWN0cy9lc2Nyb3cvdGVzdHMvcGVybWlzc2lvbmxlc3NfcmVsZWFzZV9yZXZpZXcucnNgLgpBdXRoLWdhdGluZyB3b3VsZCBhZGQgYSB0aW1pbmcgdmV0byB0aGUgbm9uLWN1c3RvZGlhbCBkZXNpZ24KZGVsaWJlcmF0ZWx5IGdpdmVzIG5vYm9keSAoUFJEIMKnNC41AAAAEXJlbGVhc2VfYWR2YW5jZV8xAAAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAghDaGVja3BvaW50UGFzc2VkIC0+IEFkdmFuY2UyUmVsZWFzZWQuIFNhbWUgbWVjaGFuaWNzIGFzCmByZWxlYXNlX2FkdmFuY2VfMWAsIGZvciB0cmFuY2hlIDIuIFBlcm1pc3Npb25sZXNzIGxpa2UKYHJlbGVhc2VfYWR2YW5jZV8xYCDigJQgZGVsaWJlcmF0ZWx5IG5vdCBhdXRoLWdhdGVkLCBpdCBvbmx5IHN0YXJ0cwphIGNsb2NrLiBUaGUgIzIgc2VjdXJpdHkgcmV2aWV3IGNvdmVycyBib3RoIGNhbGxzIHRvZ2V0aGVyICh0aGV5CnNoYXJlIGBvcGVuX3RyYW5jaGVgKTsgc2VlIGByZWxlYXNlX2FkdmFuY2VfMWAncyBkb2MgY29tbWVudCBhbmQKYGRvY3MvcGVybWlzc2lvbmxlc3MtcmVsZWFzZS1yZXZpZXcubWRgLiBOb3RlIHRoYXQgdHJhbmNoZSAyJ3MKY2xvY2sgYWRkaXRpb25hbGx5IGNhbm5vdCBiZSBwdWxsZWQgZWFybHkgYXQgYWxsOiByZWFjaGluZwpgQ2hlY2twb2ludFBhc3NlZGAgcmVxdWlyZXMgdGhlIHdhcmVob3VzZSBvcGVyYXRvcidzIGF1dGgtZ2F0ZWQKYG1hcmtfY2hlY2twb2ludGAgZmlyc3QuAAAAEXJlbGVhc2VfYWR2YW5jZV8yAAAAAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAAVZBZHZhbmNlMlJlbGVhc2VkIC0+IFJlYWR5Rm9yRGVsaXZlcnkuIFRoZSBjb29wZXJhdGl2ZSBzaWduYWxpbmcKInNldHRpbmcgb3V0IGZvciBkZWxpdmVyeSIg4oCUIG9wZW5zIHRoZSByZW1haW5kZXItcGF5bWVudCB3aW5kb3cKKGByZW1haW5kZXJfd2luZG93X3NlY3NgIGZyb20gbm93KS4gQXV0aC1nYXRlZCwgdW5saWtlCmByZWxlYXNlX2FkdmFuY2VfKmA6IHRoaXMgb25lIGhhcyByZWFsIHN0YWtlcyBmb3IgdGhlIGJ1eWVyIChpdApzdGFydHMgdGhlIGNsb2NrIHRoYXQgY2FuIGVuZCBpbiBgRGVmYXVsdGVkYCksIHNvIGl0IHNob3VsZG4ndCBiZQp0cmlnZ2VyYWJsZSBieSBqdXN0IGFueW9uZS4AAAAAABJyZWFkeV9mb3JfZGVsaXZlcnkAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABQUkQgwqc0LjIgb3B0aW9uIChiKSwgdGhlIGFjdHVhbCBjb252ZXJzaW9uIHN0ZXA6IHJlYWRzIGEgZnJlc2gKb3JhY2xlIHJhdGUsIHJlcHJpY2VzIGBvcmFjbGVfY29uZmlnLmRlbm9taW5hdGVkX2Ftb3VudGAgKGFkanVzdGVkCmJ5IGBzZXR0bGVtZW50X2Jwc2AsIHNhbWUgc2hvcnRmYWxsL2dyYWRlIG1hdGggYHNldHRsZWAgaXRzZWxmCnVzZXMpIGludG8gdGhlIHNldHRsZW1lbnQgdG9rZW4sIGFuZCBjb21wYXJlcyB0aGF0IGFnYWluc3Qgd2hhdCdzCmFjdHVhbGx5IGVzY3Jvd2VkLiBXaGF0ZXZlciBnYXAgcmVtYWlucyDigJQgdGhlIGJ1eWVyJ3MgRlggcmlzaywKbWFkZSBjb25jcmV0ZSDigJQgYmVjb21lcyBgZnhfc2hvcnRmYWxsX2Ftb3VudGAgZm9yIGBmdW5kX2Z4X3Nob3J0ZmFsbGAKdG8gY29sbGVjdC4KCldvcmtlZCBleGFtcGxlLCB1c2luZyB0aGlzIHNlc3Npb24ncyBsaXZlLXRlc3RlZCBudW1iZXJzOiBhIGRlYWwKd29ydGggMSwwMDAsMDAwIE5HTiAoYGRlbm9taW5hdGVkX2Ftb3VudCA9IDEwXzAwMF8wMDBfMDAwXzAwMGAgYXQKdGhlIDctZGVjaW1hbCBjb252ZW50aW9uKSwgZnVsbHkgZGVsaXZlcmVkIGF0IGZ1bGwgZ3JhZGUKKGBzZXR0bGVtZW50X2JwcyA9IDEwXzAwMGApLCBSZWZsZWN0b3IgcXVvdGluZyBgcHJpY2UgPQo2Nl83MDBfMDAwXzAwMGAgYXQgYGRlY2ltYWxzID0gMTRgICjiiYgxIE5HTiA9IDAuMDAwNjY3IFVTRCwgaS5lLgriiYgxLDUwMCBOR04vVVNEKTogYG5nbl9vd2VkID0gMTBfMDAwXzAwMF8wMDBfMDAwYCAodW5jaGFuZ2VkLCBmdWxsCmJwcyksIGBmeF9hZGp1c3RlZF90b3RhbCA9IDEwXzAwMF8wMDBfMDAwXzAwMCAqIDY2XzcwMF8wMDBfMDAwIC8KMTBeMTQgPSA2XzY3MF8wMDBfMDAwYCDigJQgNjY3IFVTREMsIG1hdGNoaW5nIOKCpjEsMDAwLDAwMCBhdAriiYgxLDUwMCBOR04vVVNEIGJ5IGhhbmQuIElmIGB0b3RhbF9hbW91bnRgIGhhZCBiZWVuIGZ1bmRlZAphc3N1bWluZyBhIHNsaWdodGx5IGJldHRlciByYXRlLCBlc2Nyb3cgbWlnaHQgaG9sAAAAFHJlc29sdmVfZnhfc2hvcnRmYWxsAAAAAAAAAAEAAAPpAAAAAgAAAAM=",
        "AAAAAAAAA85UaGUgZGlzcHV0ZS1zaWRlIGVzY2FwZSBoYXRjaCwgc2FtZSBzaGFwZSBhcwpgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgL2BleHBpcmVfZnhfc2hvcnRmYWxsX3dpbmRvd2A6CnBlcm1pc3Npb25sZXNzLCBkZWFkbGluZS1nYXRlZC4gSWYgdGhlIHRocmVlIHBhcnRpZXMgY2FuJ3QgcmVhY2gKdGhlIHVuYW5pbW91cyBjb25zZW50IGByZXNvbHZlX2Rpc3B1dGVgIHJlcXVpcmVzIGJlZm9yZQpgZGlzcHV0ZV9kZWFkbGluZWAsIHRoaXMgZG9lcyAqKm5vdCoqIGd1ZXNzIHdobyB3YXMgYXQgZmF1bHQg4oCUCmNvbnNpc3RlbnQgd2l0aCBgZmxhZ19kaXNwdXRlYCdzIGRvYyBjb21tZW50LCB0aGlzIGNvbnRyYWN0IHN0aWxsCmlzbid0IGFyYml0cmF0aW5nIGFueXRoaW5nLiBJdCBqdXN0IHJlc3RvcmVzIHRoZSBwcmUtZGlzcHV0ZQpzdGF0dXMsIGV4YWN0bHkgbGlrZSBgcmVzb2x2ZV9kaXNwdXRlYCB3b3VsZCwgc28gdGhlIGZyZWV6ZSBjYW4ndApiZWNvbWUgcGVybWFuZW50IGJ5IG9uZSBwYXJ0eSBzaW1wbHkgcmVmdXNpbmcgdG8gZXZlciBjb25zZW50LgpXaGF0ZXZlciBub3JtYWwgZGVhZGxpbmUtdHJpZ2dlcmVkIG1lY2hhbmlzbSB3b3VsZCBvdGhlcndpc2UgaGF2ZQphcHBsaWVkIChgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgLCBgcmVjbGFpbV9vbl9ub25kZWxpdmVyeWAsCmV0Yy4pIGlzIGZyZWUgdG8gcnVuIGFnYWluIGZyb20gdGhlcmUuCgpEZWFkbGluZS0qKmV4Y2x1c2l2ZSoqOiBgbm93ID4gZGlzcHV0ZV9kZWFkbGluZWAgaXMgcmVxdWlyZWQsIHNvIGluCnRoZSBkZWFkbGluZSBzZWNvbmQgaXRzZWxmIHRoaXMgc3RpbGwgcmVmdXNlcyB3aXRoCmBEaXNwdXRlV2luZG93Tm90UGFzc2VkYCB3aGlsZSBgcmVzb2x2ZV9kaXNwdXRlYCByZW1haW5zIGF2YWlsYWJsZSDigJQKdGhlIGJvdW5kYXJ5IHNlY29uZCBpcyBuZXZlciBhIGRlYWQgem9uZSB3aGVyZSBuZWl0aGVyIHJlc3VtZSBwYXRoCndvcmtzLgAAAAAAFWV4cGlyZV9kaXNwdXRlX3dpbmRvdwAAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAApZUaGUgc2VsbGVyLW5vbi1kZWxpdmVyeSBwYXRoOiBgZGVsaXZlcnlfZGVhZGxpbmVgIGhhcyAqc3RyaWN0bHkqCnBhc3NlZCAoYG5vdyA+IGRlbGl2ZXJ5X2RlYWRsaW5lYDsgdGhlIGRlYWRsaW5lIHNlY29uZCBpdHNlbGYgc3RpbGwKcmVmdXNlcyB3aXRoIGBEZWxpdmVyeURlYWRsaW5lTm90UGFzc2VkYCkgd2l0aG91dCBgY29uZmlybV9kZWxpdmVyeWAKZXZlciBoYXZpbmcgcnVuLiBSZXR1cm5zIHdoYXRldmVyJ3MgY3VycmVudGx5IGVzY3Jvd2VkIHRvIHRoZSBidXllcgphbmQgc2V0cyBgRm9yZmVpdGVkYC4KCkJ1eWVyLWdhdGVkLCBub3QgcGVybWlzc2lvbmxlc3Mg4oCUIHVubGlrZSBgZXhwaXJlX3JlbWFpbmRlcl93aW5kb3dgLAp0aGUgYmVuZWZpY2lhcnkgaGVyZSBpcyBhIHNwZWNpZmljIHBhcnR5ICh0aGUgYnV5ZXIpIHJlY2xhaW1pbmcKd2hhdCdzIHJpZ2h0ZnVsbHkgdGhlaXJzLCB0aGUgc2FtZSByZWFzb25pbmcgYHJlY2xhaW1fdHJhbmNoZWAgdXNlcy4KCk5vdCByZWFjaGFibGUgZnJvbSBgRHJhZnRgOiBub3RoaW5nJ3MgZXZlciBiZWVuIGVzY3Jvd2VkIGF0IHRoYXQKcG9pbnQsIHNvIHRoZXJlJ3Mgbm90aGluZyB0byByZWNsYWltIGFuZCBubyByZWFsIGV2ZW50IGhhcwpvY2N1cnJlZCB3b3J0aCByZWNvcmRpbmcgYXMgYSBmb3JmZWl0dXJlLgAAAAAAFnJlY2xhaW1fb25fbm9uZGVsaXZlcnkAAAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAAA4ZUaGUgYnV5ZXItZGVmYXVsdCBwYXRoOiBgcmVtYWluZGVyX2RlYWRsaW5lYCBoYXMgKnN0cmljdGx5KiBwYXNzZWQKKGBub3cgPiByZW1haW5kZXJfZGVhZGxpbmVgKSB3aXRoIHRoZSByZW1haW5kZXIgbmV2ZXIgZnVuZGVkIOKAlCBpbgp0aGUgZGVhZGxpbmUgc2Vjb25kIGl0c2VsZiB0aGlzIHJlZnVzZXMgd2l0aApgUmVtYWluZGVyV2luZG93Tm90UGFzc2VkYCwgc28gdGhlIGJ1eWVyJ3Mgb3duIGZ1bmRpbmcgd2luZG93IGlzCm5ldmVyIGN1dCBzaG9ydCBieSBhIHNlY29uZC4gU3dlZXBzIHdoYXRldmVyJ3MgY3VycmVudGx5IGVzY3Jvd2VkCih0aGUgZGVwb3NpdCwgb3Igd2hhdGV2ZXIgb2YgaXQgdGhlIGNvb3BlcmF0aXZlIGhhc24ndCBhbHJlYWR5CmNsYWltZWQpIHRvIHRoZSBjb29wZXJhdGl2ZSBhbmQgc2V0cyBgRGVmYXVsdGVkYCDigJQgdW5jb250ZXN0ZWQgYnkKY29uc3RydWN0aW9uLCBzbyB1bmxpa2UgYGNhbmNlbGAvYHJlYXNzaWduX2J1eWVyYCB0aGlzIG5lZWRzIG5vCmNvbnNlbnQgZnJvbSBhbnlvbmUuCgpQZXJtaXNzaW9ubGVzcywgZGVsaWJlcmF0ZWx5OiB0aGUgb3V0Y29tZSAoc3dlZXAgdG8gY29vcGVyYXRpdmUpCmRvZXNuJ3QgZGVwZW5kIG9uIHdobyBjYWxscyBpdCwgb25seSBvbiB3aGV0aGVyIHRoZSBkZWFkbGluZSBoYXMKcGFzc2VkLCB0aGUgc2FtZSByZWFzb25pbmcgYHJlY2xhaW1fdHJhbmNoZWAgd291bGQgdXNlIGlmIGl0CndlcmVuJ3QgYWxyZWFkeSBzY29wZWQgdG8gYmVuZWZpdCB0aGUgYnV5ZXIgc3BlY2lmaWNhbGx5LiBBbnlvbmUg4oCUCmluY2x1ZGluZyBhbiBvZmYtY2hhaW4gd2F0Y2hlciDigJQgY2FuIHRyaWdnZXIgaXQgb25jZSB0aGUgZmFjdCBvZgp0aGUgbWF0dGVyIChkZWFkbGluZSBwYXNzZWQsIHVuZnVuZGVkKSBpcyB0cnVlLgAAAAAAF2V4cGlyZV9yZW1haW5kZXJfd2luZG93AAAAAAAAAAABAAAD6QAAAAIAAAAD",
        "AAAAAAAABABSZWNvcmRzIHdoZXRoZXIgc2V0dGxlbWVudCdzIEZYIHJlc29sdXRpb24gaXMgY3VycmVudGx5IGJsb2NrZWQKb24gYSBzdGFsZSBvcmFjbGUgcXVvdGUgKCMxKSDigJQgdGhlICJ3YWl0aW5nIG9uIGEgZnJlc2ggcXVvdGUiCnNpZ25hbCwgbWFkZSByZWFkYWJsZSBpbnN0ZWFkIG9mIGluZmVyYWJsZSBvbmx5IGZyb20gYSBmYWlsZWQKYHJlc29sdmVfZnhfc2hvcnRmYWxsYCBjYWxsLgoKV2h5IHRoaXMgZXhpc3RzIGFzIGl0cyBvd24gY2FsbDogU29yb2JhbiByb2xscyBiYWNrIGV2ZXJ5IHN0YXRlCndyaXRlIGFuIGludm9jYXRpb24gbWFrZXMgd2hlbiBpdCByZXR1cm5zIGBFcnJgLCBzbwpgcmVzb2x2ZV9meF9zaG9ydGZhbGxgJ3MgYE9yYWNsZVN0YWxlYCByZWZ1c2FsIGNhbm5vdCBwZXJzaXN0IGFueQptYXJrZXIgb2YgaXRzIG93biDigJQgYnkgdGhlIHRpbWUgYSBjYWxsZXIgc2VlcyB0aGF0IGVycm9yLCBub3RoaW5nCndhcyByZWNvcmRlZC4gVGhpcyBwcm9iZSBpcyB0aGUgd3JpdGUgdGhhdCByZWZ1c2FsIGNhbid0IGJlOiBpdApydW5zIHRoZSBzYW1lIHN0YWxlbmVzcyBjaGVjayBhbmQgcmVjb3JkcyB0aGUgYW5zd2VyIGluCmBDb21taXRtZW50OjpmeF9xdW90ZV9zdGFsZWAsIHdoaWNoIGBnZXRfY29tbWl0bWVudGAgc3VyZmFjZXMgdG8KdGhlIEFQSS9pbmRleGVycy9VSS4KClNldHMgdGhlIGZsYWcgd2hlbiB0aGUgcXVvdGUgaXMgc3RhbGU7IGNsZWFycyBpdCB3aGVuIGEgZnJlc2gKcXVvdGUgaXMgcmVhZCDigJQgc28gdGhlIGZsYWcgc2VsZi1jb3JyZWN0cyB0aGUgbW9tZW50IFJlZmxlY3RvcgpwdWJsaXNoZXMgYSBmcmVzaCBvbmUsIHdpdGggbm8gcmV2ZXJzZSB0cmFuc2l0aW9uIHRvIGxlYXJuLiBBCm1pc3NpbmcgcXVvdGUgKGBPcmFjbGVQcmljZVVuYXZhaWxhYmxlYCkgcHJvcGFnYXRlcyBhcyBhbiBlcnJvcgppbnN0ZWFkIG9mIHNldHRpbmcgYW55dGhpbmc6ICJ0aGUgb3JhY2xlIGRvZXNuJ3QgcXVvdGUgdGhpcyBhc3NldAphdCBhbGwiIGlzIGEgcGVybWFuZW50IGNvbmZpAAAAF3JlZnJlc2hfZnhfcXVvdGVfc3RhdHVzAAAAAAAAAAABAAAD6QAAAAIAAAAD",
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
        fund_fx_shortfall: this.txFromJSON<Result<void>>,
        get_oracle_config: this.txFromJSON<Result<Option<OracleConfig>>>,
        reclaim_advance_1: this.txFromJSON<Result<void>>,
        reclaim_advance_2: this.txFromJSON<Result<void>>,
        release_advance_1: this.txFromJSON<Result<void>>,
        release_advance_2: this.txFromJSON<Result<void>>,
        ready_for_delivery: this.txFromJSON<Result<void>>,
        resolve_fx_shortfall: this.txFromJSON<Result<void>>,
        expire_dispute_window: this.txFromJSON<Result<void>>,
        reclaim_on_nondelivery: this.txFromJSON<Result<void>>,
        expire_remainder_window: this.txFromJSON<Result<void>>,
        refresh_fx_quote_status: this.txFromJSON<Result<void>>,
        expire_fx_shortfall_window: this.txFromJSON<Result<void>>
  }
}