# Multi-asset / multi-token support — spike

Issue #46 asked for a research pass on what it would take to support
commitments denominated in more than one token — for example, a deal
where advance tranches settle in a stablecoin and the remainder settles
in a commodity-backed token, or where FX conversion involves two
distinct on-chain assets rather than an off-chain rate.

Ground truth for the current design is `contracts/escrow/src/lib.rs`.
The scope here is feasibility and rough cost, not a full spec.

## Current model

One token address (`token: Address`) is stored in `Commitment` and used
for every transfer. All amounts — `total_amount`, advance BPS, remainder,
FX shortfall top-up — are denominated in that one asset. The token is
set at `initialize` and never changed.

## What "multi-asset" could mean

Three distinct interpretations emerged during the spike; they have
different costs.

### A. Two tokens: advance token vs. remainder token

Advance tranches pay in token A; the remainder (and FX shortfall) pays
in token B. Example: advances in USDC, remainder in a commodity token
whose price is oracle-tracked.

**What changes:**
- `Commitment` gains `remainder_token: Address`.
- `lock` deposits `deposit_amount` in token A; `fund_remainder` deposits
  in token B.
- `claim_tranche` / `reclaim_tranche` transfer token A; `settle`
  transfers token B.
- `fund_fx_shortfall` / `expire_fx_shortfall_window` operate on token B.
- The FX oracle comparison now has a natural unit: "how much token B
  does `total_amount` of token A buy?" — which is what Reflector already
  provides for asset pairs.

**Effort:** medium. Schema migration adds one field; the transfer sites
are localized (`claim_tranche`, `reclaim_tranche`, `fund_remainder`,
`settle`, `fund_fx_shortfall`, `expire_fx_shortfall_window`). The BPS
arithmetic is unchanged.

### B. Per-tranche tokens

Each tranche has its own token. Tranche 1 settles in token A, tranche 2
in token B, remainder in token C.

**What changes:**
- `Commitment` gains a token per configurable tranche slot, or a
  small `Vec<Address>` of tokens (bounded by `MAX_GRADE_SCHEDULES`).
- Every transfer site must look up the right token for its tranche index.
- FX resolution becomes per-token, requiring multiple oracle calls.

**Effort:** high. The complexity grows with the number of token slots,
and the oracle integration has to become a loop or a per-token config.
The tranche-window logic is already the most complex part of the
contract; adding token dispatch into it increases the audit surface
significantly.

### C. Atomic swap / multi-party settlement

The cooperative delivers token A; the buyer pays in token B; a DEX or
AMM converts at settlement. The contract becomes a programmatic market
order, not just an escrow.

**What changes:** everything. This is a new product, not a feature flag.
The existing BPS arithmetic, oracle integration, and dispute mechanism
all assume a single-asset escrow; a cross-asset settlement needs an
AMM interface, slippage bounds, and partial-fill handling. The contract
would need to hold positions in two assets simultaneously.

**Effort:** very high. Out of scope for this spike.

## Recommendation

**Interpretation A** (two tokens: advance vs. remainder) is the
highest-value option at the lowest marginal cost, and it covers the
real-world case most likely to come up first: advance payments in a
liquid stablecoin, final settlement in a commodity or locally-issued
token. It requires no changes to the BPS arithmetic or the dispute
mechanism, and the schema diff is a single new field.

**Interpretation B** should wait until there is a concrete buyer or
cooperative requesting it; the tranche-index dispatch and multi-oracle
path add audit surface without a proven demand signal.

**Interpretation C** is a separate product and should not be pursued
here.

## Soroban-specific notes

- Soroban's `token::Client` works identically for any SEP-41-compliant
  token; there is no protocol difference between USDC, XLM-wrapped, or
  a custom asset. Adding a second token is purely a contract-state
  question, not a VM constraint.
- Storage cost for one additional `Address` field is negligible relative
  to the existing `Commitment` struct size (see `instance_storage_layout_stays_well_below_the_entry_size_limit`).
- Two tokens means two `token.transfer` calls in some paths; each
  transfer is a cross-contract call that consumes instruction budget.
  Current cost estimates in `docs/cost-estimation.md` assume one
  transfer per call; two-token paths would roughly double the transfer
  line in that table, still well inside the per-operation budget.
- Authorization: `lock` and `fund_remainder` already require buyer auth
  for the token transfer (the buyer signs the full invocation tree,
  including the nested `token.transfer`). A two-token design keeps that
  pattern; the buyer signs both token A's `lock` deposit and token B's
  `fund_remainder` deposit at their respective call sites. No auth
  architecture change is needed.

## Open questions before implementation

1. Should the BPS denominator apply to both tokens, or only to token A
   (advances)? The current BPS math computes advance amounts as fractions
   of `total_amount`; if the remainder is a different asset, `total_amount`
   needs a unit.
2. Should the oracle still compare "how much did the buyer's token A
   payment convert to in token B terms?" or should it compare against a
   fixed token-B amount set at initialization?
3. Does `cancel` refund token A only (what was deposited), or also any
   partially-funded token B amount?

These are product decisions; the spike does not answer them.
