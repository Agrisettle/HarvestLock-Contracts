# Minimal-commitment walkthrough (#97)

The smallest *real* commitment this contract can legally carry, walked
from `initialize` to `settle`, with every optional field at its simplest
valid value. Every existing doc example in this repo shows the
full-featured case (three grades, an oracle, a 200-member allocation
ledger); this one shows what a two-party deal with no FX exposure and
one quality tier actually has to say.

This walkthrough is **executable, not illustrative**: the test
[`minimal_commitment_walkthrough_settles_end_to_end`](../contracts/escrow/src/test.rs)
in `contracts/escrow/src/test.rs` runs exactly the sequence below,
locally, with the exact numbers. Run it with:

```bash
cd contracts/escrow
cargo test minimal_commitment_walkthrough_settles_end_to_end
```

## Why a Rust test and not a live testnet deploy

This issue asked for the case extracted from `api/test/stellar.test.ts`'s
simplest existing test. That file lives in the `HarvestLock` application
repo, not in this contracts repo (this checkout has no `api/` directory
at all — see `HANDOFF.md`, which references it as external), so the
minimal case was extracted from the simplest lifecycle tests here
(`basic lifecycle` + `advance_to_remainder_funded` +
`confirm_delivery` + `settle`) and pared down to the smallest valid
arguments instead. The walkthrough below was verified against the local
Rust test, not against a live testnet deploy — no funded network keys
were assumed. The local host is the same Soroban `env-host` the deployed
WASM runs on; a testnet run would add only network latency, not new
behavior.

## The minimal `initialize` call, field by field

```text
initialize(
    buyer,                  // any Address; signs this call
    cooperative,            // any Address; signs claim_advance_* later
    warehouse_operator,     // any Address; signs mark_checkpoint/confirm_delivery
    token,                  // any SEP-41 token contract address
    total_amount = 1_000,   // smallest readable number for the 7-decimal convention (i128 > 0)
    advance1_bps = 6_000,   // 60% first advance — any 0..=10_000 value works; zero would trivialize the tranche steps
    advance2_bps = 2_000,   // 20% second advance; 6_000 + 2_000 <= 10_000 (the `initialize` ceiling)
    claim_window_secs = 1,  // minimum legal: must be > 0 (`InvalidWindow` rejects 0)
    remainder_window_secs = 1,  // minimum legal: must be > 0
    delivery_window_secs = 1,   // minimum legal: must be > 0
    contracted_quantity = 1,    // minimum legal: must be > 0 (`InvalidQuantity` rejects 0)
    grade_price_bps = vec![10_000],  // ONE entry, full price — the schedule must be non-empty, each entry <= 10_000
    oracle_config = None,   // the only "fully optional" field: no FX conversion for this deal
)
```

Why each simplest choice is the simplest:

- `oracle_config: None` — `initialize` accepts it (PRD §4.2's conversion
  path activates only for `Some`), so `oracle_rate`,
  `resolve_fx_shortfall`, `fund_fx_shortfall`, and
  `expire_fx_shortfall_window` drop out of the lifecycle entirely. With
  `None`, `settle` derives the payout from `total_amount` directly.
- `grade_price_bps = [10_000]` — one grade at 10_000 bps means grade 0
  is "the agreed price, in full." It is the shortest schedule the
  contract accepts (empty is `InvalidGradeSchedule`) and makes
  `confirm_delivery(1, 0)` a no-arithmetic case.
- Windows of `1` second each — zero is rejected outright, so `1` is the
  floor. Nothing in the walkthrough waits on a window: every claim lands
  before its deadline and `fund_remainder` lands inside its own
  deadline, so wall-clock time never advances.
- `contracted_quantity = 1` — `u32 > 0`; with a single grade at full
  price, delivered `1` of contracted `1` yields `settlement_bps = 10_000`
  (the multiplier is capped at 10_000 by construction, so even
  over-delivery couldn't need a bigger schedule).
- `total_amount = 1_000` — only `> 0` is required (`ZeroAmount`), but a
  round 1,000 keeps every bps split below integer-exact (60% = 600,
  20% = 200, remainder = 200) so the arithmetic is checkable by hand.
  Soroban does not allow negative or zero amounts, and `advance1_bps +
  advance2_bps > 10_000` is rejected, so 60/20 with a 20% remainder is
  about the smallest non-degenerate funding split: a `6_000/0_000` split
  would make step 6's second tranche claim a zero-amount no-op, which
  hides (rather than simplifies) the tranche-resolution requirement in
  `settle`.

## The walkthrough, step by step

State column shows `get_status()`; balances are the three parties'
`token.balance()` at each point. Auth column is the minimum signer set
(see `docs/authorization-matrix.md` for the full rule table).

| # | Call | Auth | Status after | Contract holds | Buyer holds | Coop holds |
|---|------|------|--------------|----------------|-------------|------------|
| 0 | *(deploy — instance entry created, no commitment yet)* | deployer | *(none)* | 0 | 1,000 | 0 |
| 1 | `initialize(…)` (args above) | buyer | `Draft` | 0 | 1,000 | 0 |
| 2 | `lock()` | buyer | `Locked` | 800 | 200 | 0 |
| 3 | `release_advance_1()` | *(permissionless)* | `Advance1Released` | 800 | 200 | 0 |
| 4 | `claim_advance_1()` | cooperative | `Advance1Released` | 200 | 200 | 600 |
| 5 | `mark_checkpoint()` | warehouse | `CheckpointPassed` | 200 | 200 | 600 |
| 6 | `release_advance_2()` | *(permissionless)* | `Advance2Released` | 200 | 200 | 600 |
| 7 | `claim_advance_2()` | cooperative | `Advance2Released` | 0 | 200 | 800 |
| 8 | `ready_for_delivery()` | cooperative | `ReadyForDelivery` | 0 | 200 | 800 |
| 9 | `fund_remainder()` | buyer | `ReadyForDelivery` | 200 | 0 | 800 |
| 10 | `confirm_delivery(1, 0)` | warehouse | `Delivered` | 200 | 0 | 800 |
| 11 | `settle()` | *(permissionless)* | `Settled` | 0 | 0 | 1,000 |

Notes on the steps that make this case "minimal" rather than just
"small":

- **Step 1 is the only `Draft` state.** `lock` is the first call in the
  entire contract that moves funds — a commitment abandoned before it
  costs the buyer nothing (see `initialize`'s doc comment and issue
  #96); a plain `cancel()` from `Draft` needs only buyer + cooperative
  consent and moves zero.
- **Step 2 escrows the deposit, not the whole deal**: `lock` pulls
  `advance1_bps + advance2_bps` = 60% + 20% = 800 (`deposit_amount`),
  leaving the 200 remainder to step 9. That two-phase split is the whole
  design of `lock`/`fund_remainder`; the minimal deal still exercises
  both halves because a zero remainder (`advance1 + advance2 = 10_000`)
  would make step 9's transfer a skipped no-op (`remainder > 0` guard)
  and erase the buyer-default exposure (`expire_remainder_window`) the
  product actually exists to bound.
- **Steps 3–7 are the claimable-tranche pair.** Both tranches must be
  *resolved* (claimed here; the buyer could have reclaimed them after
  their windows lapsed instead) before `settle` runs — this is the
  `TrancheUnresolved` guard, and even the minimal commitment cannot skip
  it. Claims land within their 1-second windows because each `claim_*`
  immediately follows its `release_*` in the same test clock instant.
- **Step 10**: `confirm_delivery(delivered_quantity, grade_index)` with
  `(1, 0)` → `quantity_bps = min(1, 1) * 10_000 / 1 = 10_000`,
  `settlement_bps = 10_000 * 10_000 / 10_000 = 10_000`. Full delivery,
  full price.
- **Step 11** (the payout math, all integers, no rounding loss):
  `adjusted_total = 1_000 * 10_000 / 10_000 = 1_000`;
  `claimed_by_coop = 600 + 200 = 800`;
  `owed_from_remainder = 1_000 − 800 = 200`; contract balance is exactly
  200 → cooperative receives 200, buyer's shortfall refund is 0. The
  contract ends at balance 0, and the ledger has recorded seven
  `status_changed` events (`Draft → … → Settled` has no skips in this
  path — the two claim steps are not status transitions).

## What the minimal case still does *not* get to skip

A reader evaluating "can we use this contract for a simpler deal"
should know exactly what stays mandatory even here:

1. The three named parties must all sign at least one call each — the
   contract has no role-collapse requirement (`initialize` accepts one
   address holding all three roles, see issue #94's tests), but the
   lifecycle above still routes through buyer, cooperative, and
   warehouse signatures at the rows marked in the Auth column.
2. Both tranche resolutions (steps 4/7 — or their reclaim paths once
   the windows lapse) are hard prerequisites for `settle`, even with
   zero disputes and zero FX exposure.
3. `grade_price_bps` must exist even if it has one entry, and every
   entry ≤ 10_000. The one-entry `[10_000]` schedule is the floor; there
   is no "grading is off" mode.
4. Storage lifetime keeps applying: the instance entry holding this
   commitment is re-rented by every call and read
   (`docs/storage-ttl.md`, #98), so a deal paused at step 8 for months
   stays alive only as long as *something* touches the contract at least
   once per network `max_entry_ttl` window.

## Deploying this for real (not done in this walkthrough)

For a real testnet run, the same sequence through `soroban-cli` is:

```bash
stellar contract deploy --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --network testnet --source buyer
stellar contract invoke --id <contract> --network testnet --source buyer -- \
  initialize <buyer> <cooperative> <warehouse> <token_address> \
  1000 6000 2000 1 1 1 1 '[10000]' null
# ...then the steps 2–11 of the table, in order.
```

Every argument above maps one-to-one to the test's `initialize` call;
`null` in the CLI positional encoding is the `None` oracle_config. This
was **not executed against testnet** — no funded network key was
assumed — the local Rust test is the verification of record.
