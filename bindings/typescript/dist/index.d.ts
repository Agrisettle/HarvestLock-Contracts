import { Buffer } from "buffer";
import { AssembledTransaction, Client as ContractClient, ClientOptions as ContractClientOptions, MethodOptions, Result } from "@stellar/stellar-sdk/contract";
import type { u32, u64, i128, Option } from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";
export declare const Errors: {
    1: {
        message: string;
    };
    2: {
        message: string;
    };
    3: {
        message: string;
    };
    4: {
        message: string;
    };
    5: {
        message: string;
    };
    6: {
        message: string;
    };
    7: {
        message: string;
    };
    8: {
        message: string;
    };
    9: {
        message: string;
    };
    10: {
        message: string;
    };
    11: {
        message: string;
    };
    /**
     * A tranche is neither claimed nor expired yet — `settle` refuses to
     * run until someone resolves it via `claim_advance_*` or
     * `reclaim_advance_*`. See `settle`'s doc comment for why this is a
     * hard requirement, not just a nicety.
     */
    12: {
        message: string;
    };
    13: {
        message: string;
    };
    /**
     * `fund_remainder` called after `remainder_deadline` has *strictly*
     * passed — the deadline second itself is still in time.
     */
    14: {
        message: string;
    };
    /**
     * `expire_remainder_window` called at or before
     * `remainder_deadline` — the deadline second itself still belongs to
     * the buyer's funding window, so expiry needs strictly more than the
     * deadline, not merely the deadline.
     */
    15: {
        message: string;
    };
    /**
     * `confirm_delivery` called while `ReadyForDelivery` but the buyer
     * hasn't funded the remainder yet.
     */
    16: {
        message: string;
    };
    /**
     * `reclaim_on_nondelivery` called at or before `delivery_deadline` —
     * the deadline second itself still belongs to the cooperative.
     */
    17: {
        message: string;
    };
    /**
     * `contracted_quantity` was zero at `initialize`.
     */
    18: {
        message: string;
    };
    /**
     * `grade_price_bps` was empty, or contained an entry over 10_000
     * (a grade worth more than the pre-agreed full unit price) at
     * `initialize`.
     */
    19: {
        message: string;
    };
    /**
     * `confirm_delivery`'s `grade_index` didn't land inside
     * `grade_price_bps`.
     */
    20: {
        message: string;
    };
    /**
     * `set_allocation` called a second time — the allocation ledger is
     * one-time and immutable once recorded.
     */
    21: {
        message: string;
    };
    /**
     * `set_allocation`'s `members` was empty, or the sum of every
     * entry's `share_bps` exceeded 10_000.
     */
    22: {
        message: string;
    };
    /**
     * `get_allocation` called before `set_allocation` ever ran.
     */
    23: {
        message: string;
    };
    /**
     * `initialize`'s `oracle_config` was `Some` with `max_age_secs == 0`
     * — a zero staleness bound would reject every real quote, which is
     * never what a caller actually wants (that's what leaving
     * `oracle_config` as `None` is for).
     */
    24: {
        message: string;
    };
    /**
     * `oracle_rate` or `resolve_fx_shortfall` called but `initialize`
     * never set an `oracle_config` for this commitment — the only two
     * functions that read the oracle at all (#124).
     */
    25: {
        message: string;
    };
    /**
     * The configured Reflector oracle returned `None` for
     * `oracle_config.price_asset` — it doesn't quote that asset at all
     * (call the oracle's own `assets()` to check what it does quote).
     */
    26: {
        message: string;
    };
    /**
     * The most recent quote is older than `oracle_config.max_age_secs`
     * allows (PRD §16.3's staleness bound) — refused rather than
     * returned, since a caller silently getting a too-old rate is worse
     * than getting no rate at all.
     */
    27: {
        message: string;
    };
    /**
     * `resolve_fx_shortfall` called a second time — one-time and
     * immutable, same reasoning as `set_allocation`: re-resolving later
     * against a different rate would let either side game the number
     * by choosing when to call it.
     */
    28: {
        message: string;
    };
    /**
     * `fund_fx_shortfall`/`expire_fx_shortfall_window`/`settle` called
     * before `resolve_fx_shortfall` has run for an oracle-configured
     * commitment.
     */
    29: {
        message: string;
    };
    /**
     * `settle` called while a resolved FX shortfall is still unfunded.
     */
    30: {
        message: string;
    };
    /**
     * `fund_fx_shortfall`/`expire_fx_shortfall_window` called but
     * `resolve_fx_shortfall` found no shortfall at all (the escrowed
     * amount already covered the fresh-rate-converted obligation) —
     * there's nothing to fund or expire.
     */
    31: {
        message: string;
    };
    /**
     * `fund_fx_shortfall` called a second time.
     */
    32: {
        message: string;
    };
    /**
     * `fund_fx_shortfall` called after `fx_shortfall_deadline` has
     * *strictly* passed — too late to cure, though the deadline second
     * itself is still in time. `expire_fx_shortfall_window` is the only
     * path left from here.
     */
    33: {
        message: string;
    };
    /**
     * `expire_fx_shortfall_window` called at or before
     * `fx_shortfall_deadline` — the deadline second itself still belongs
     * to the buyer's funding window, matching `fund_fx_shortfall`'s own
     * inclusive cut-off (`now > deadline`).
     */
    34: {
        message: string;
    };
    /**
     * `flag_dispute`'s `flagger` argument wasn't the buyer, cooperative,
     * or warehouse operator — only a named party to the deal can flag
     * one, not an arbitrary third address.
     */
    35: {
        message: string;
    };
    /**
     * `expire_dispute_window` called at or before `dispute_deadline` —
     * the deadline second itself is still inside the freeze. Only that
     * timeout path can produce this: `resolve_dispute` has no deadline
     * gate at all. (Either call made while `status` isn't `Disputed` is
     * covered by the same `InvalidState` every other status-gated call
     * already uses.)
     */
    36: {
        message: string;
    };
    /**
     * `initialize`'s `grade_price_bps` held more than
     * `MAX_GRADE_ENTRIES` entries — longer than any real grading
     * schedule needs, and unbounded work the contract shouldn't accept.
     */
    37: {
        message: string;
    };
    /**
     * `set_allocation`'s `members` held more than
     * `MAX_ALLOCATION_MEMBERS` entries — beyond the PRD's specified
     * cooperative scale, and unbounded storage the contract shouldn't
     * accept.
     */
    38: {
        message: string;
    };
};
export type Status = {
    tag: "Draft";
    values: void;
} | {
    tag: "Locked";
    values: void;
} | {
    tag: "Advance1Released";
    values: void;
} | {
    tag: "CheckpointPassed";
    values: void;
} | {
    tag: "Advance2Released";
    values: void;
} | {
    tag: "ReadyForDelivery";
    values: void;
} | {
    tag: "Delivered";
    values: void;
} | {
    tag: "Settled";
    values: void;
} | {
    tag: "Cancelled";
    values: void;
} | {
    tag: "Defaulted";
    values: void;
} | {
    tag: "Forfeited";
    values: void;
} | {
    tag: "Disputed";
    values: void;
};
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
    lock: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    cancel: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    settle: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a get_status transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * No auth required — read-only view, callable by anyone.
     */
    get_status: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Status>>>;
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
    initialize: ({ buyer, cooperative, warehouse_operator, token, total_amount, advance1_bps, advance2_bps, claim_window_secs, remainder_window_secs, delivery_window_secs, contracted_quantity, grade_price_bps, oracle_config }: {
        buyer: string;
        cooperative: string;
        warehouse_operator: string;
        token: string;
        total_amount: i128;
        advance1_bps: u32;
        advance2_bps: u32;
        claim_window_secs: u64;
        remainder_window_secs: u64;
        delivery_window_secs: u64;
        contracted_quantity: u32;
        grade_price_bps: Array<u32>;
        oracle_config: Option<OracleConfig>;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a oracle_rate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Reads the current rate for this commitment's configured oracle
     * asset, enforcing the staleness bound. A pure read against
     * Reflector's own stored history, not a state transition — callable
     * in any status, including after `Settled`.
     *
     * No auth required — permissionless read.
     */
    oracle_rate: (options?: MethodOptions) => Promise<AssembledTransaction<Result<OracleRate>>>;
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
    flag_dispute: ({ flagger }: {
        flagger: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    fund_remainder: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a get_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * The recorded allocation ledger, if `set_allocation` has run.
     *
     * No auth required — read-only view, callable by anyone.
     */
    get_allocation: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<AllocationMember>>>>;
    /**
     * Construct and simulate a get_commitment transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * No auth required — read-only view, returns the full commitment state.
     */
    get_commitment: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Commitment>>>;
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
    reassign_buyer: ({ new_buyer }: {
        new_buyer: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    set_allocation: ({ members }: {
        members: Array<AllocationMember>;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    claim_advance_1: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a claim_advance_2 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Cooperative claims tranche 2, if within the window. Requires
     * cooperative auth — same mechanics as `claim_advance_1`, including its
     * inclusive `advance2_deadline` boundary (#111).
     */
    claim_advance_2: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a mark_checkpoint transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Advance1Released -> CheckpointPassed. Requires the warehouse
     * operator's attestation — this is a judgment call, not a mechanical
     * state advance, so unlike opening an advance tranche it *is*
     * auth-gated. Independent of whether tranche 1 has actually been
     * claimed yet — crop progress doesn't wait on paperwork.
     */
    mark_checkpoint: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    resolve_dispute: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    confirm_delivery: ({ delivered_quantity, grade_index }: {
        delivered_quantity: u32;
        grade_index: u32;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    fund_fx_shortfall: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a get_oracle_config transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * The configured oracle, if `initialize` set one.
     *
     * No auth required — read-only view, callable by anyone.
     */
    get_oracle_config: (options?: MethodOptions) => Promise<AssembledTransaction<Result<Option<OracleConfig>>>>;
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
    reclaim_advance_1: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a reclaim_advance_2 transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Buyer reclaims tranche 2, if the window has passed unclaimed.
     * Requires buyer auth — same mechanics as `reclaim_advance_1`, including
     * the exclusive `advance2_deadline` boundary (#111).
     */
    reclaim_advance_2: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    release_advance_1: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    release_advance_2: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a ready_for_delivery transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Advance2Released -> ReadyForDelivery. The cooperative signaling
     * "setting out for delivery" — opens the remainder-payment window
     * (`remainder_window_secs` from now). Auth-gated, unlike
     * `release_advance_*`: this one has real stakes for the buyer (it
     * starts the clock that can end in `Defaulted`), so it shouldn't be
     * triggerable by just anyone.
     */
    ready_for_delivery: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    resolve_fx_shortfall: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    expire_dispute_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    reclaim_on_nondelivery: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    expire_remainder_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    refresh_fx_quote_status: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
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
    expire_fx_shortfall_window: (options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
}
export declare class Client extends ContractClient {
    readonly options: ContractClientOptions;
    static deploy<T = Client>(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions & Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
    }): Promise<AssembledTransaction<T>>;
    constructor(options: ContractClientOptions);
    readonly fromJSON: {
        lock: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        cancel: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        settle: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_status: (json: string) => AssembledTransaction<Result<Status, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        initialize: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        oracle_rate: (json: string) => AssembledTransaction<Result<OracleRate, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        flag_dispute: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        fund_remainder: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_allocation: (json: string) => AssembledTransaction<Result<AllocationMember[], import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_commitment: (json: string) => AssembledTransaction<Result<Commitment, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        reassign_buyer: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        set_allocation: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        claim_advance_1: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        claim_advance_2: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        mark_checkpoint: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        resolve_dispute: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        confirm_delivery: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        fund_fx_shortfall: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_oracle_config: (json: string) => AssembledTransaction<Result<Option<OracleConfig>, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        reclaim_advance_1: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        reclaim_advance_2: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        release_advance_1: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        release_advance_2: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        ready_for_delivery: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        resolve_fx_shortfall: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        expire_dispute_window: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        reclaim_on_nondelivery: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        expire_remainder_window: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        refresh_fx_quote_status: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        expire_fx_shortfall_window: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
    };
}
