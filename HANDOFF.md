# Handoff — HarvestLock-Contracts

**Read this before touching the code.** It exists so a different session, a
different AI agent, or a future you with no memory of this session can pick
up exactly where things stopped, without re-deriving decisions that are
already made.

Keep this file honest and current. When you finish a work session on this
repo, update it — not the roadmap in the main repo, this file specifically —
before you stop. A stale HANDOFF.md is worse than none, because it actively
misleads the next reader.

---

## Where this sits in the bigger picture

This repo is `contracts/` from the original monorepo, split out per the
main repo's `README.md`. The PRD (linked from that README) is the spec.
`ROADMAP.md` in the `harvestlock` repo, Phase 0 Track B, is the plan this
work is executing against. This file is neither of those — it's the
as-built state, which will drift from both the spec and the plan as real
implementation surfaces things neither anticipated. When it drifts, trust
this file for "what actually exists" and flag the drift rather than
silently reconciling it.

## Toolchain state (as of this session)

- Rust `1.97.1`, target `wasm32v1-none` already installed (this is the
  current recommended Soroban target, not the older `wasm32-unknown-unknown`
  — don't "fix" this back).
- `soroban-sdk = "28.0.0"` — bumped from `27.0.6` (itself resolved via
  `cargo add`, not hand-picked) by dependabot in PR #5, 14 Oct 2026, with
  both CI jobs green. Don't downgrade without a reason; don't upgrade
  without checking the changelog against this contract's API usage first.
  **Version check done, 16 Oct 2026 (issue #67):** latest is `29.0.1`
  (12 Oct 2026). v29.0.0 (7 Oct) is the soroban-env `29.0.0` / Protocol 29
  host bump — its release notes name no SDK API changes, and none of the
  surface this contract uses (the `contract*` macros, `token`, `Address`,
  `BytesN`, `Vec`, instance storage, testutils) is touched — and `29.0.1`
  exists only to fix a wasmi runtime error `29.0.0` shipped with. Verdict:
  **the bump is safe but not urgent**; leave it to dependabot as its own
  CI-verified PR rather than bundling it into an unrelated change, and
  don't bump mid-season without a reason to.
- `stellar-cli` **v28.0.0 — installed and confirmed working.** Not via
  `cargo install` — that path is broken on this machine: the default Rust
  host toolchain here is `stable-x86_64-pc-windows-gnu`, and compiling
  `stellar-cli` from source needs `dlltool.exe` (MinGW binutils), which
  isn't installed and isn't trivial to add. **Don't retry `cargo install
  stellar-cli` on this machine without fixing that first.** What actually
  worked: downloaded the prebuilt `stellar-cli-28.0.0-x86_64-pc-windows-msvc.tar.gz`
  from the GitHub release (`gh api repos/stellar/stellar-cli/releases/latest`),
  extracted `stellar.exe`, dropped it into `~/.cargo/bin/` (already on PATH).
  MSVC-built binaries run fine regardless of the local Rust host toolchain —
  it's a standalone executable, not something that needs to match your
  `rustup` setup.
- `cargo test` runs on the host target and does **not** require
  `stellar-cli` — only `stellar contract build` (producing the deployable
  `.wasm`) and deployment do. If you only need to verify contract logic,
  `cargo test` is faster and doesn't wait on the CLI.
- Testnet identities already generated on this machine via `stellar keys
  generate <name> --network testnet --fund`: `deployer`, `buyer`,
  `cooperative`, `warehouse`. Addresses are in `stellar keys address <name>`
  output, not repeated here since they're throwaway testnet keys, not
  anything to treat as stable — regenerate freely.

## What's implemented

One contract, `contracts/escrow`, crate name `harvestlock-escrow`. One
`Commitment` per contract instance, matching PRD §4.8's "one instance per
commitment."

The state machine, now with **two-phase funding** and the **buyer-default /
seller-non-delivery forfeiture paths** — this was the top item in last
session's "what's deliberately NOT implemented" and is no longer a gap:

```
Draft --lock()--> Locked --release_advance_1()--> Advance1Released
      --mark_checkpoint()--> CheckpointPassed --release_advance_2()--> Advance2Released
      --ready_for_delivery()--> ReadyForDelivery --fund_remainder()--> (still ReadyForDelivery)
      --confirm_delivery()--> Delivered --settle()--> Settled
```

`lock` no longer escrows the full `total_amount` — it escrows only the
**deposit** (`advance1_bps + advance2_bps` of the total). The remainder is
escrowed later via `fund_remainder`, once the cooperative calls
`ready_for_delivery` to signal they're setting out. This isn't a bug fix
over the earlier one-shot-escrow design, it's a deliberate redesign: it's
what makes a buyer's default a clean, contract-enforceable deadline
(`expire_remainder_window`) instead of something that has to be asserted by
a person. Two new deadline-gated, uncontested-by-construction terminal
paths came with it:

- `ready_for_delivery` — **cooperative-auth-gated.** `Advance2Released` ->
  `ReadyForDelivery`. Opens the remainder-payment window
  (`remainder_window_secs` from now).
- `fund_remainder` — **buyer-auth-gated.** Escrows `total_amount - deposit`,
  if called within the window `ready_for_delivery` opened. Rejects with
  `RemainderWindowPassed` after the deadline — no late cure, that's what
  `expire_remainder_window` is for.
- `expire_remainder_window` — **permissionless** (deliberately — the outcome
  doesn't depend on who calls it, only on whether the deadline passed
  unfunded, same reasoning `reclaim_tranche` would use if it weren't already
  scoped to the buyer specifically). Sweeps the contract's current balance
  to the cooperative, sets `Status::Defaulted`. **This is the buyer-default
  path** — per this session's product decision, buyer default carries an
  immediate off-chain permanent bar (see `api/` once that lands), not a
  graduated strike system.
- `reclaim_on_nondelivery` — **buyer-auth-gated.** Once `delivery_deadline`
  (an absolute deadline set at `initialize`, independent of the remainder
  window) passes with `confirm_delivery` never having run, returns the
  contract's current balance to the buyer, sets `Status::Forfeited` — a
  deliberately distinct status from `Defaulted`, not a reuse of it, since
  the two represent opposite parties' failure. **This is the seller-
  non-delivery path** — per this session's product decision, this one *does*
  carry a graduated 3-strike system before a cooperative is barred, unlike
  the buyer's immediate bar (again, `api/`-side, once built).
- `confirm_delivery`'s guard changed from `Advance2Released` to
  `ReadyForDelivery` **and** `remainder_funded == true` — delivery can't be
  confirmed while the buyer still owes money on the deal. New error
  `RemainderNotFunded` covers the latter half of that guard.
- `cancel` and `reassign_buyer`'s reachable-state range both extended to
  include `ReadyForDelivery` — mutual unwind and buyer-position transfer
  are both still sensible even after the remainder's been funded, same
  balance-based-refund/no-money-moves mechanics as before, no new logic
  needed in either function itself.

Full function list, everything not already covered above unchanged from
last session: `initialize` (now also takes `remainder_window_secs` and
`delivery_window_secs`, both `> 0`-validated the same way
`claim_window_secs` is), `release_advance_1`/`_2`, `claim_advance_1`/`_2`,
`reclaim_advance_1`/`_2`, `mark_checkpoint`, `settle`, `get_status`,
`get_commitment`.

Every real status transition also publishes a **`status_changed`** event
(a `#[contractevent]` with `from`/`to` as topics — the shape SEP-41's
`transfer` uses), added with the #68–#71 bundle. Emission is centralized
in a private `set_status` helper — the single place `status` is assigned
after `initialize` — so "every transition emits" holds by construction,
not by convention. `initialize` deliberately emits nothing: a commitment
being created in `Draft` is an initial state, not a transition into one.
This is the primitive `api/`'s read-triggered cache refresh has been
waiting for; shipped contract-side only, no consumer exists yet.

Tests (**210 passing: 164 in `src/test.rs`, 46 across `tests/`** — see the
"Last updated"
entry for the count's history) cover, beyond last session's set: `lock` escrows
only the deposit, not the full total; `ready_for_delivery` opens the window
and is cooperative-gated; `fund_remainder` transfers exactly the remainder,
rejects before `ReadyForDelivery`, rejects a second call, rejects after the
window passes, requires buyer auth; **the buyer-default path** —
`expire_remainder_window` correctly sweeps the contract's *current* balance
(not a naive full-total assumption) to the cooperative and sets `Defaulted`,
rejects before the deadline, rejects if already funded, and — a dedicated
test — genuinely has no auth requirement at all; **the seller-non-delivery
path** — `reclaim_on_nondelivery` returns the current balance to the buyer
and sets `Forfeited`, works from every pre-`Delivered` state including
`ReadyForDelivery` with the remainder already funded, rejects before the
deadline, rejects from `Draft` (nothing at risk) and after `Delivered`
(wrong path — `settle` applies then), requires buyer auth;
`confirm_delivery` rejects both before `ReadyForDelivery` and before the
remainder is funded, as two independently-tested guards; every existing
happy-path/cancel/reassign test updated to route through
`ready_for_delivery`/`fund_remainder` and re-verified against the new
balance shape (a real gotcha caught three times over: forgetting that an
already-claimed tranche stays with the cooperative through `cancel`/
`reclaim_on_nondelivery` — those only ever return the contract's *current*
balance, never claw back what already left).

**Run it**: `cd contracts/escrow && cargo test`

## Verified on testnet — twice now, and the second time specifically exercised the new mechanic

Per-deployment summaries live in [`CHANGELOG.md`](./CHANGELOG.md) — dates
there were verified against the deployed instances, with each WASM hash
cross-checked against the one named below. This section stays the
transaction-level walk-through; that one is the index, and this one is the
record.

**Deployment 1** (prior session, contract `CDUWXPAC2AT353J4UF5WWJVW3ZUMATZH7PGNZ2AEXWOGT2USDRR77JSO`) validated the pre-claimable-balance happy path. Superseded — that contract predates this session's changes and shouldn't be used as a reference for current behavior.

**Deployment 2** (this session) — **contract `CDVF6UVJOLF3OHCFSYSJ72RMG2T6DUQ42VRJ6IHL6MVEFDYEBZ3KTFK4`**
([stellar.expert](https://stellar.expert/explorer/testnet/contract/CDVF6UVJOLF3OHCFSYSJ72RMG2T6DUQ42VRJ6IHL6MVEFDYEBZ3KTFK4)),
WASM 13,472 bytes optimized, 13 exported functions. Initialized with a
**deliberately short 20-second `claim_window_secs`** so the reclaim/expiry
path could actually be exercised live within a session rather than only
tested in the simulated-clock unit tests — this is a demo-only choice, not
a suggested production value (see the open question about a sane minimum
window, below).

Walked through, on real testnet transactions, against the real native-XLM
SAC, 1,000,000,000 stroops (100 XLM), 15%/20% advance split:

1. `lock` — 1,000,000,000 moved buyer → contract.
2. `release_advance_1`, then `claim_advance_1` **within** the window — 150,000,000 to the cooperative.
3. `mark_checkpoint`, `release_advance_2`, `confirm_delivery`.
4. **Attempted `settle` immediately — rejected on-chain with `Error(Contract, #12)`**, i.e. `TrancheUnresolved`, because tranche 2 hadn't been claimed or reclaimed yet. This is the actual deployed WASM enforcing the guard, not just the native test build — confirms the fix is real, not something that only works in `cargo test`.
5. Waited past the 20-second window (real time), then `reclaim_advance_2` — 200,000,000 returned buyer ← contract.
6. `settle` now succeeds — sweeps the remaining 650,000,000 to the cooperative.
7. Final check: `get_status` → `Settled`, contract token balance → `0`. Cooperative received 150M + 650M = 800,000,000; buyer got back 200,000,000. **800,000,000 + 200,000,000 = 1,000,000,000 exactly.**

Every number above was read back from the ledger, not asserted. Re-run it
yourself with the identities from the toolchain section if you want to
confirm independently.

**Deployment 3** (this session, 1 Sept 2026) — **contract `CAVWBS5USXL5WRIB2HBGKBEABQZ6AT6YH7P4JO452WWEMWPWER5T6RDF`**,
WASM hash `c6b73c5c483c5946be73fe68e0cb798c6cd39440ec1430e05cf1f048c1018327`
(14,825 bytes optimized, 14 exported functions — the +1 is `cancel`).
Initialized and `lock`ed live on testnet, confirming the new WASM deploys
and the pre-existing functions still work unchanged.

`cancel` is now verified **both** at the unit-test level (30/30,
including the dual-auth-trace check) **and** live on testnet with two
genuinely different, correctly-authorized signers, via `@stellar/stellar-
sdk` in the `api/` repo (`api/test/helpers.ts`'s `submitMultiPartyCall`,
exercised by `api/test/stellar.test.ts`).

Getting there took two false starts worth recording so nobody re-treads
them. First, ad-hoc `stellar-cli`: `stellar contract invoke` only signs
with one key (the tx source), and hand-assembling a multi-party
transaction via `stellar tx sign --sign-with-key <second party>` on top
of that fails with `TxBadAuthExtra` — the second signature lands as an
extra classic envelope signature, not a proper per-entry Soroban auth
credential. Second, the seemingly-obvious SDK equivalent —
`Transaction.sign()` called once per party on the same transaction
object — **fails the exact same way**, for the exact same reason: neither
approach produces a real per-entry `SorobanAuthorizationEntry` signature
for the non-source party, both just pile up classic envelope signatures.
The actual fix: sign each non-source auth entry individually via
`authorizeEntry()`, then rebuild the operation/transaction around the
signed entries (mutating the built transaction's auth array in place
doesn't work either — it's a derived view). Full detail, including a
third gotcha about unfunded accounts, is in `api/test/helpers.ts`'s doc
comment — read that before trying to hand-verify a multi-party call
again, from the CLI or otherwise.

**Deployment 4** (this session, 2 Sept 2026) — WASM hash
`667e4a50c8ad9af081bf2c8a5be9d34069a45e5d582753cc665346455a1096b5`
(16,130 bytes optimized, 15 exported functions — `reassign_buyer` is the
new one). Uploaded via `stellar contract upload` (not `deploy` this
time — the `api/` repo's own tests deploy fresh instances from this
hash as needed, so no standalone contract address to record here).
`reassign_buyer` verified live with three genuinely different,
freshly-funded signers via `api/test/stellar.test.ts` — buyer field
confirmed changed via a fresh `get_commitment` read, not assumed from
a successful submission alone.

One real process gap this surfaced: bumping `ESCROW_WASM_HASH` in
`api/.env` to a new build's hash is **not** the same as that build
actually being on testnet. `stellar contract build` only compiles
locally; `deployContractInstance`'s `createCustomContract` op
references a wasm hash that has to already be uploaded, and it doesn't
upload anything itself. Forgetting the `stellar contract upload` step
(or the `deploy` step, if a standalone reference instance is also
wanted) fails every deploy with `Error(Storage, MissingValue)` /
"Wasm does not exist" — confusing the first time, obvious once you
know what it means.

**Deployment 5** (this session, 2 Sept 2026) — WASM hash
`3351e0de0edc918130fb2f88973b90cc717307a789bb0b7ee0802c50f3bd6832`
(22,048 bytes optimized, 19 exported functions — `ready_for_delivery`,
`fund_remainder`, `expire_remainder_window`, `reclaim_on_nondelivery` are
the four new ones). Uploaded via `stellar contract upload`. Three fresh
instances deployed and walked through real testnet transactions to prove
all three of the new terminal outcomes, each confirmed by a fresh
`get_status`/`get_commitment` read, not assumed from submission success:

1. **Happy path with two-phase funding** — `CB5MR3IYQ6R4QTFMTXTBFG4WSCZTJ3QTND6GMBSASGORPQSD3VKYCTCW`,
   1,000,000,000 stroops, 15%/20% split. `lock` moved exactly 350,000,000
   (the deposit, not the full total). `claim_advance_1`/`claim_advance_2`
   paid 150,000,000 + 200,000,000. `ready_for_delivery` then
   `fund_remainder` moved the remaining 650,000,000. `confirm_delivery`
   then `settle` swept that 650,000,000 to the cooperative. Final tally:
   cooperative received 150M + 200M + 650M = 1,000,000,000 exactly, buyer
   and contract both at 0, status `Settled`.
2. **Buyer-default path** — `CA3AB4QXJ5ZZ4SIIH3VHI7C37MDWBUQSZECT64XBVHPDQI473BJK4G52`,
   10%/20% split, a deliberately short 15-second `remainder_window_secs`
   (demo-only, same reasoning as Deployment 2's short `claim_window_secs`).
   `claim_advance_1` paid 100,000,000; advance 2's 200,000,000 was
   deliberately left unclaimed. `ready_for_delivery` opened the window; the
   buyer never called `fund_remainder`. After the window passed (waited
   real time), `expire_remainder_window` — called by the **deployer
   account, neither the buyer nor the cooperative** — swept the contract's
   remaining 200,000,000 to the cooperative and set `Defaulted`. Proves
   both the sweep amount (the actual unclaimed balance, not a naive
   full-total assumption) and the genuinely-permissionless auth shape in
   one transaction.
3. **Seller-non-delivery path** — `CCOVWDEWTQPI57OCUUHGSIFSMAUQWJOX5SVZI2J5KFYDRVGNDVTFXSNQ`,
   15%/15% split, a deliberately short 15-second `delivery_window_secs`.
   `lock` moved the 300,000,000 deposit; the cooperative then never took
   another action at all. After the delivery deadline passed (waited real
   time), the buyer's `reclaim_on_nondelivery` returned the full
   300,000,000 and set `Forfeited`.

**Deployment 6** (3 Sept 2026) — WASM hash
`f935d469a6714a0c00075dab1fb26dd4a309d85cc4e321be1256d3be1ff28c8f`
(25,370 bytes optimized, still 19 exported functions — no new functions
this time, `initialize` and `confirm_delivery` changed signature
instead). Uploaded via `stellar contract upload`. Implements the PRD §7
shortfall/grade adjustment schedule `confirm_delivery`'s doc comment had
flagged as unbuilt since Deployment 5: `initialize` now takes
`contracted_quantity: u32` and a pre-agreed `grade_price_bps: Vec<u32>`
table (grade -> price multiplier in bps, "pre-agreed" meaning fixed at
initialize, never renegotiated at settlement); `confirm_delivery` takes
`delivered_quantity`/`grade_index` and computes `settlement_bps` (the
combined quantity x grade multiplier, capped at 10_000 — over-delivery
isn't paid extra in v1); `settle` now pays the cooperative only what's
still owed against `settlement_bps` (already-claimed advances are never
clawed back, matching "advance not clawed back" in the PRD's
partial-delivery row) and refunds the rest of the remainder to the buyer
as a shortfall credit — a real behavior change from Deployment 5's
"sweep the entire balance to the cooperative," not just an additive
feature. 67/67 unit tests (9 new). One fresh instance deployed and
walked through a real testnet transaction proving the exact math, not
just the state transition:

1. **Partial delivery + grade adjustment together** —
   `CBDDWE4CKHAAFZPNNUVZRCHKAJQ3F2PMQA547LMPPM6ULRZJ6AKAHXAQ`, 100,000,000
   stroops, 15%/20% advance split, `contracted_quantity: 1000`,
   `grade_price_bps: [10000, 9000, 7500]`. Walked to `ReadyForDelivery`
   with both advances claimed (15,000,000 + 20,000,000 to the
   cooperative) and the 65,000,000 remainder funded. `confirm_delivery`
   called with `delivered_quantity: 500` (50%) and `grade_index: 1`
   (90%) — `get_commitment` confirmed `settlement_bps: 4500` (50% x 90%)
   before `settle` ran. `settle`'s actual on-chain transfer events:
   10,000,000 to the cooperative, 55,000,000 back to the buyer — exactly
   `adjusted_total (45,000,000) - already_claimed (35,000,000)` to the
   cooperative, and the rest of the remainder refunded, matching the
   hand-computed expectation before the call was ever made, not fit to
   the result afterward.

**Deployment 7** (4 Sept 2026) — WASM hash
`9f1843c9c9c299182d307f7d9a25bfe1b7558ff2f8313daf0aa233aa2adb014e`
(28,767 bytes optimized, 21 exported functions — `set_allocation`,
`get_allocation` are the two new ones). Uploaded via `stellar contract
upload`. Implements the PRD §4.8/§16.1 allocation ledger — the last
"must have" v1 feature from the PRD's own feature list with nothing
built for it before this: `set_allocation(members: Vec<AllocationMember>)`
records each member farmer's entitlement as a **per-member salted
hash** plus a `share_bps`, cooperative-gated, one-time, callable only
from `Draft` (before `lock`). Deliberately **not required** before
`lock` — a solo-farmer commitment with no cooperative pooling shouldn't
be forced through a step that doesn't apply to it; see the function's
own doc comment. **Record-only in v1**, not a behavior change to
`settle`: PRD §4.9 already states the v1 default explicitly
("Transparent allocation... payment settles to the cooperative wallet")
— so unlike the shortfall/grade work, this genuinely was additive, no
existing test needed updating. The salt-scheme decision TASKS.md
flagged as compliance-load-bearing: per-*member* (not just per-contract)
random salts, hashed off-chain (API's Postgres) — the on-chain entry is
never a bare phone-number hash, and deleting the off-chain salt+phone
row (NDPA s.34 erasure) makes the on-chain hash permanently unlinkable
to a real person, since brute-forcing a random salt is infeasible
regardless of phone-number keyspace size. 76/76 unit tests (9 new).
One fresh instance deployed and walked through real testnet
transactions:

1. **Allocation ledger record + read-back** —
   `CBRZPZPXBYW2QHGU5KOFWG27AS4YM7EQLULOALYAE3VGJ7BMZTZLKARZ`.
   `set_allocation` called with two members (6,000/4,000 bps split);
   `get_allocation` read back the exact same two entries. A second
   `set_allocation` call correctly rejected on-chain with
   `Error(Contract, #21)` (`AllocationAlreadySet`). `lock` then ran
   successfully with the allocation already set, confirming the two
   features compose — `Status::Locked` confirmed via a fresh
   `get_status` read.

**Deployment 8** (4 Sept 2026) — WASM hash
`5322b601fc6ea29e4edfaac260573347d6b47672ecaeb5e13e7cbd75ff0afcab`
(33,488 bytes optimized, 23 exported functions — `oracle_rate`,
`get_oracle_config` are the two new ones). Uploaded via `stellar
contract upload`. Builds PRD §16.3's oracle staleness bound, the item
TASKS.md and this file's own "Next steps" both named as the last
genuinely-unbuilt "must have" from the PRD's feature list: `initialize`
gained an `Option<OracleConfig>` (oracle contract address + price
asset symbol + max quote age), and `oracle_rate()` calls the configured
[Reflector](https://reflector.network) SEP-40 oracle's `lastprice`,
rejecting with `OracleStale` if the quote is older than the configured
bound, `OraclePriceUnavailable` if the oracle doesn't quote that asset
at all, `OracleNotConfigured` if `initialize` never set one. **Read
primitive only** — doesn't change `settle`'s payout math yet; see the
module doc in `lib.rs` for why that's a deliberate stopping point, not
an oversight. 86/86 unit tests (10 new, against a local mock oracle
contract — see `test.rs`'s `mock_oracle` module).

Before writing any of this, spent real effort confirming Reflector's
actual interface and address rather than assuming one: fetched the
official Stellar docs' oracle-providers page and the
`reflector-network/reflector-contract` repo's own README (its
published client interface is copied near-verbatim into `reflector.rs`
— Reflector doesn't publish a crate, and cross-contract calls match on
XDR wire shape, not Rust type identity, so this doesn't need to be a
dependency). That surfaced a real, load-bearing finding, not an
assumption: **Reflector's live testnet "Fiat exchange rates" oracle
(`CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W`) does not
quote NGN.** Confirmed by actually calling it —
`stellar contract invoke --id CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W --source-account deployer --network testnet --send=no -- assets`
returned `EUR, GBP, CHF, CAD, MXN, ARS, BRL, THB, XAU` against a `USD`
base at 14 decimals — no NGN entry at all. This is the same category
of gap PRD §4.4 already named for commodity prices ("no on-chain
oracle" for farm-gate prices), just discovered here for the
currency-conversion half of §4.2 too, which PRD §4.3's own table had
listed Reflector against as if the FX-reference half were already
solved.

Live-verified on testnet anyway, using a currency Reflector *does*
quote to prove the mechanism itself, with `NGN` left as the intended
production symbol (this activates automatically the day Reflector adds
it — no code change needed, just a different `price_asset` string at
`initialize`):

1. **Live cross-contract oracle read** —
   `CCUDE3MMW5JGK4QM3CA5PGW23KCYEIXFJ46754LKIWNJR4OKACU6HMA4`,
   initialized with `oracle_config` pointing at the real Reflector fiat
   oracle above, `price_asset: "GBP"`, `max_age_secs: 3600`. `oracle_rate`
   returned `{"price":"135300006985028","timestamp":1788560400}` —
   135300006985028 / 10^14 = **1.353 USD per GBP**, a realistic live FX
   rate, not a fixture — read back from a real cross-contract call this
   contract made to Reflector's actual deployed testnet oracle, at a
   timestamp 4m19s before the call (well inside Reflector's own 5-minute
   update resolution and comfortably under the 3600s bound configured).
   `get_oracle_config` read back the exact stored config afterward,
   confirming `initialize`'s `Option` correctly persisted it.

**Deployment 9** (6 Sept 2026) — WASM hash
`7940147db99f8222a0710de46566a479d42dbf0626575d17af16e1698aa045f0`
(42,175 bytes optimized, 26 exported functions — `resolve_fx_shortfall`,
`fund_fx_shortfall`, `expire_fx_shortfall_window` are the three new
ones). Uploaded via `stellar contract upload`. Wires PRD §4.2's option
(b) — buyer tops up or is refunded at settlement — into `settle`'s
actual payout math, a product decision made explicitly this session
(the PRD names three options and leaves the choice to pilot partners;
this repo doesn't get to assume one). `OracleConfig` gained
`denominated_amount: i128` (the deal's true value in `price_asset`,
same 7-decimal convention `total_amount` already uses); `resolve_fx_shortfall`
— reachable once `Delivered` with both tranches already resolved, so
the comparison uses final figures, not a snapshot that could still
change — reads a fresh oracle rate, converts `denominated_amount`
(adjusted by `confirm_delivery`'s `settlement_bps`) into the settlement
token, and records the gap against what's actually escrowed as
`fx_shortfall_amount`. A shortfall has to be paid in via
`fund_fx_shortfall` before `settle` will run
(`FxNotResolved`/`FxShortfallUnfunded`); missing that deadline is
`expire_fx_shortfall_window`, which reuses `Status::Defaulted` — the
*same* immediate-permanent-bar consequence `expire_remainder_window`'s
buyer-default already carries, by explicit product decision (asked and
answered this session, not assumed), and for free at the API layer
since the existing reputation machinery already reacts to any fresh
transition into `Defaulted`. 27 new contract unit tests (103/103
total) against the local mock oracle, including the full resolve →
shortfall-found → fund → settle chain and its `expire` mirror.

Live-verified on testnet with real numbers, not fixtures — a fresh
instance (`CBOZLCNHWFHTK24WO7RCDBEWUFV6LWQKZFHJUIOZZRVBMNRTGLXRMKKH`),
`denominated_amount: 100,000,000`, `total_amount: 100,000,000`,
oracle_config pointed at the real Reflector fiat oracle (GBP, same
stand-in reasoning as Deployment 8 — Reflector still doesn't quote
NGN). Walked through the full lifecycle for real: `lock` (30,000,000
deposit), both tranches claimed (15,000,000 each), `fund_remainder`
(70,000,000), `confirm_delivery` at full quantity/grade, then
`resolve_fx_shortfall` — against the live GBP/USD rate at that exact
moment (≈1.353), it computed `fx_adjusted_total: 135,300,006` and
`fx_shortfall_amount: 35,300,006` (135,300,006 - 30,000,000 already
claimed - 70,000,000 escrowed), matching a hand calculation from the
same live rate before the call, not fit to the result afterward.
`fund_fx_shortfall` moved exactly 35,300,006 from the buyer; `settle`
then paid the cooperative exactly 105,300,006 from the remainder
balance. Cooperative's total take across all three payments: 15,000,000
+ 15,000,000 + 105,300,006 = **135,300,006 — exactly `fx_adjusted_total`**,
confirmed via a fresh Horizon balance read, not assumed from submission
success. The whole point of option (b), proven with real testnet
transfers: the cooperative received the deal's actual current-rate
value, not the stale amount `total_amount` alone would have paid out.

**Deployment 10** (6 Sept 2026) — WASM hash
`21a4bb2efb129d0252d32b575f2ca595259b1c2fe841d2d0554dc02c44aca580`
(47,171 bytes optimized, 29 exported functions — `flag_dispute`,
`resolve_dispute`, `expire_dispute_window` are the three new ones).
Closes the "no dispute path" item below — PRD's must-have "dispute
flagging with defined escalation." Any one of the three named parties
(`flagger` must equal `buyer`, `cooperative`, or `warehouse_operator`)
can freeze a commitment into `Status::Disputed` from `Locked` through
`Delivered`, which blocks every other state-changing call for free
(none of them ever accept `Disputed` as a required status). This still
isn't arbitration — see `flag_dispute`'s doc comment for why deciding
*who was right* remains an explicit non-goal, same as before.
`resolve_dispute` (unanimous three-party auth) or `expire_dispute_window`
(permissionless, once `dispute_deadline` passes — reuses
`remainder_window_secs` as the window, same reasoning as
`fx_shortfall_deadline`) both restore the *exact* pre-dispute status,
recorded in a new `dispute_pre_status` field (`Status::Draft` doubles
as the "unset" sentinel, since `flag_dispute` never accepts `Draft` as
a status to dispute from — not `Option<Status>`, which the
`contracttype` macro's `ScVal` conversion doesn't support for a
unit-variant-only enum like `Status`, confirmed by trying it first and
hitting a real compile error). One documented, deliberate limitation:
freezing `status` doesn't pause any other absolute deadline already
ticking on the commitment — see `flag_dispute`'s doc comment, and the
`dispute_does_not_pause_an_already_ticking_deadline_known_limitation`
test that proves it rather than just asserting it in prose. 17 new
contract unit tests (120/120 total).

Live-verified on real testnet
(`CCHQLA3LZXWXWJJR75TWFM4POBPA673VIWJX4YWSAHUZEWUSXSF6HJBO`, native XLM
as the settlement token, `remainder_window_secs: 120` for a fast real
timeout): `lock` (300,000 native-stroop deposit, a real transfer, not a
fixture), cooperative called `flag_dispute` — status genuinely flipped
to `Disputed`, and a subsequent `release_advance_1` call was rejected
live (`InvalidState`), proving the freeze is real, not just a label.
Waited out the real 120-second `dispute_deadline`, then the deployer
account (not a party to the commitment at all) called
`expire_dispute_window` successfully — status restored to exactly
`Locked`, `dispute_pre_status` cleared back to `Draft`, with zero
authorization from any of the three named parties. `resolve_dispute`'s
three-party path was verified live too, through the `api/` repo's
staged multi-party HTTP layer (see its own HANDOFF.md) rather than raw
`stellar-cli` — no CLI flag exists here for co-signing three separate
Soroban auth entries in one invocation, and this project already built
exactly that mechanism for `reassign_buyer`.

**Deployment 11** (this session, 20 Oct 2026) — WASM hash
`e097f1ffe08f878007cd9511d5fc70da0e238ac64491181e158d58f91e4714c5`
(60,043 bytes, 29 exported functions — no new functions; this was a
**measurement** deployment, not a feature one). Uploaded via `stellar
contract install`; four instances deployed
(`CCTQTECYATFFE3GJWYSND6CJOGB6KLGSF3JKK5KW3HRBNTKLFBFJEUWJ` for the
walk-through, `CDG2V5HIJ5KJS6D3QK4VI4NRCRVUZYHHZQWYSONULUKQYJMOPFDBWAQ7`
and `CBTFFKL24HYNUUG4K26FBB3B47T5KROWR3FTMBH4SST5NA3NCT5IFATV` as
controls, and
`CDKP2KUIDUJ4MS3G4GDS372BK3HMRZZEMJGM4C5GULTGAISI2FFGZAXK` as an
independent reproduction of the `confirm_delivery` figure). The point of
the deployment: the delivery-group methods
(`mark_checkpoint`, `confirm_delivery`) had **never had a systematic
resource-cost pass** — only correctness testing — so this session measured
them with
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
against the deployed instances and wrote the numbers up in
[`docs/gas-costs.md`](./docs/gas-costs.md). The same WASM is what the
dependabot SDK bump to 29.0.1 (19 Oct 2026) produces at this HEAD.

Walked through on real testnet transactions against the real native-XLM SAC,
100,000,000 stroops, 15%/20% advance split, `contracted_quantity: 1000`,
`grade_price_bps: [10000, 9000, 7500]`:

1. `lock` (35,000,000 deposit), `release_advance_1`, then **`mark_checkpoint`**
   by the warehouse operator — status genuinely flipped to `CheckpointPassed`.
2. `release_advance_2`, `claim_advance_1` (15,000,000 to the cooperative),
   `claim_advance_2` (20,000,000), `ready_for_delivery` (cooperative),
   `fund_remainder` (65,000,000 from the buyer).
3. **`confirm_delivery`** by the warehouse operator with `delivered_quantity:
   1000`, `grade_index: 0` — a fresh `get_commitment` read afterwards shows
   `status: Delivered`, `delivered_quantity: 1000`, `grade_index: 0`,
   `settlement_bps: 10000`, `remainder_funded: true`. Full delivery at the
   top grade, so the arithmetic path really ran rather than erroring out.

Measured resource costs (protocol 29, ledgers ~5290578–5290808), all against
the deployed instances, not a local harness:

| Method | CPU insns | Read/write entries | Fee non-refundable | Fee total (simulated) | Fee charged (real) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `mark_checkpoint` | 1,523,347 | 2 / 2 | 4,579 | 906,370 | 4,181 |
| `confirm_delivery` | 1,530,217 | 2 / 2 | 4,584 | 906,976 | 4,242 |

Two findings worth keeping:

- **The auth-gated calls are the expensive ones, not the ones that move
  money.** `mark_checkpoint`/`confirm_delivery` move *zero* stroops and
  still cost ~1.52M CPU instructions — ~53% more than the permissionless
  `release_advance_1` (996,645 on the same build, same instance) and within
  9% of `claim_advance_1` (1,671,867), which *does* move 15,000,000 stroops.
  The delta appears only on the auth-gated path: one extra read entry, one
  extra write entry, ~527k CPU instructions. Not a defect — a fact worth
  knowing before anyone assumes "cheap call" means "cheap".
- **99.5% of the estimator's `Fee (total)` is simulated storage rent, not
  execution.** The real submissions were charged 4,181 and 4,242 stroops
  total (see `docs/gas-costs.md`'s CLI cross-check). Budget off the
  non-refundable column, not the total.

**A real upstream tooling limitation surfaced here**, and is documented in
full in [`docs/gas-costs.md`](./docs/gas-costs.md#tooling-finding--soroban-cost-estimator-v010-cannot-encode-a-u32-argument):
`soroban-cost-estimator` v0.1.0's `--arg` infers an argument's type from the
literal alone (`parse_arg_scval` → `ScVal::I64` for any integer), ignoring the
contract spec it has already parsed. `confirm_delivery` takes two `u32`
parameters, so the stock binary cannot invoke it at all — the host aborts with
`Error(WasmVm, InvalidAction)` / `VM call trapped: UnreachableCodeReached`
before the contract body runs. Upstream `main` (commit `9627919`) has the
same defect: it adds `validate_args_against_spec`, which *accepts* `1000` for
a `u32` parameter, and then still encodes it as `ScVal::I64`. The measurement
above used a two-hunk patch to v0.1.0 that encodes `--arg` values with the
spec's declared type; the patch touches only argument encoding, and was
verified to leave every measured value byte-identical on the zero-argument
path. **Worth reporting upstream** — it blocks the tool for any contract
with a `u32`/`i32`/`u64` parameter, which is most of them.

**All eleven deployments' instances are validation artifacts, not
infrastructure.** Redeploy fresh for future testing; don't build anything
that depends on any of these addresses continuing to exist or hold correct
state.

## What's deliberately NOT implemented yet

Don't assume these are oversights — each one is a scoping decision, listed
so nobody "fixes" them without knowing what they're trading off.

1. ~~No dispute path.~~ **Built** (Deployment 10, 6 Sept 2026, see
   above): `flag_dispute`/`resolve_dispute`/`expire_dispute_window`
   implement PRD's must-have "dispute flagging with defined escalation"
   — a bounded, unilateral freeze plus unanimous-or-timeout resume.
   **Still true, on purpose**: this still doesn't arbitrate *who was
   right* about anything — the PRD is explicit that grade/quantity
   disputes are the warehouse operator's own appeals process, not a
   HarvestLock dispute, and this contract has no mechanism to decide a
   contested fault claim any other way either. What changed is only
   that a contested situation can now pause the state machine instead
   of letting some other deadline-triggered function run to a
   conclusion while it's being sorted out off-chain. Buyer-default
   forfeiture and seller-non-delivery forfeiture (`expire_remainder_window`
   and `reclaim_on_nondelivery`) and mutual cancellation (`cancel`) were
   all built earlier and separately — they're *uncontested*,
   deadline-triggered (or mutually-agreed) cases that never needed to
   wait on a dispute mechanism to begin with.
2. ~~No shortfall/grade adjustment at delivery.~~ **Built** (Deployment 6,
   3 Sept 2026, see above): `confirm_delivery` takes `delivered_quantity`/
   `grade_index`, computes `settlement_bps` against the pre-agreed
   `grade_price_bps` table, and `settle` pays out against it. What's
   still not here: the warehouse operator's attestation is trusted
   as-is — there's no separate receipt-hash/signature artifact beyond
   the `require_auth()` call itself, and grade/quantity disputes are
   explicitly off-chain (the operator's own appeals process, per the
   PRD's edge-case table) — this contract doesn't arbitrate them.
3. ~~No NGN/oracle conversion.~~ **Built** (Deployment 8, 5 Sept 2026 —
   the staleness-bound read; Deployment 9, 6 Sept 2026 — the actual
   `settle` payout wiring, see above): `oracle_rate()`/`resolve_fx_shortfall()`
   read a live Reflector SEP-40 quote enforcing PRD §16.3's staleness
   bound, and `settle` now pays out against the fresh-rate-converted
   `denominated_amount` on any oracle-configured commitment — PRD §4.2's
   option (b), buyer tops up or is refunded, asked and answered
   explicitly this session rather than assumed. Live-verified on
   testnet with a genuine top-up: a real GBP/USD rate move meant the
   cooperative was owed 135,300,006 against an escrowed 100,000,000; the
   buyer funded the 35,300,006 gap and the cooperative received the
   full fresh-rate amount, not the stale one. **Still open**:
   Reflector's live testnet fiat oracle doesn't quote NGN at all
   (verified via a real `assets()` call, see Deployment 8) — the actual
   currency this PRD section is about has no real feed to convert
   against yet. `price_asset` works with anything Reflector does quote
   today (this session's tests use GBP) and activates for NGN
   automatically the day Reflector adds it — no code change needed,
   just a different `price_asset` string at `initialize`.
   **#1's "waiting on a fresh quote" signal is now built**: a stale
   quote still refuses `resolve_fx_shortfall` outright, and since a
   Soroban invocation returning `Err` rolls back every write, that
   refusal can't record why — so the new permissionless probe
   `refresh_fx_quote_status` records the answer in the new
   `Commitment::fx_quote_stale` flag, which `get_commitment` surfaces.
   Deliberately a boolean flag alongside `fx_resolved`, not a distinct
   `Status` variant (it's a condition on one settlement sub-step, not a
   position in the deal's lifecycle), and informational only — no guard
   reads it, `resolve_fx_shortfall` still enforces staleness itself.
4. ~~No allocation ledger.~~ **Built** (Deployment 7, 4 Sept 2026, see
   above): `set_allocation`/`get_allocation`, per-member salted hashes,
   record-only (PRD §4.9 Rung 1, the v1 default, not a deferred
   decision). What's still not here: pro-rated on-chain payout (Rung 2+)
   — `settle` still pays the cooperative a lump sum — and the off-chain
   identity map itself (the API's Postgres side, not this contract's
   job; see `api/HANDOFF.md`).
5. **Still no minimum (or maximum) enforced on `claim_window_secs` at the
   contract level** — this remains deliberate, not an oversight. The
   `api/` repo now enforces a 1 hour floor / 90 day ceiling before it will
   even build an `initialize` transaction (`api/src/server.ts`), resolving
   the "at minimum whatever calls initialize should validate this" note
   this item used to end on. The contract itself staying unenforced is
   still correct: anything calling `initialize` directly (a different
   future API, a test, `stellar-cli`) bypasses that check entirely, same
   as it always could — this was never a security boundary, just the
   right layer for a business-judgment default.
6. **`release_advance_1`/`release_advance_2` remain not auth-gated** (see
   "Design decisions" — this is deliberate, not new this session).
   **Reviewed on the record (#2)**: `docs/permissionless-release-review.md`
   stress-tests the adversarial-timing scenarios this note used to flag
   (early clock-start griefing, restart/extension, every downstream
   reader of the two deadline fields, dispute-freeze interaction) and
   concludes the permissionless shape holds — recorded confirmation, not
   a code change to the auth shape. Every claim in that review is pinned
   by the dedicated suite
   `contracts/escrow/tests/permissionless_release_review.rs` (own CI
   job), including the issue's end-to-end third-party griefing scenario.
   If a deployment ever needs a *minimum* time before a tranche may
   open, the review's own answer is a term fixed at `initialize` (a new
   eligibility condition both parties can see), not a signature gate —
   the latter would create a timing veto the non-custodial design
   deliberately gives nobody (PRD §4.5) while adding a liveness
   dependency, not safety.

## Design decisions worth knowing before you change anything

- **Why claims/reclaims are built as contract-native state (a deadline +
  two booleans) instead of an actual classic Stellar `ClaimableBalanceEntry`**:
  Soroban contracts don't have a clean host-function path to construct a
  classic-ledger claimable balance from within contract code without
  juggling cross-VM interop. Reimplementing the same user-facing semantics
  natively — a stored deadline, a `claim_*` the cooperative can call within
  it, a `reclaim_*` the buyer can call after — delivers identical behavior
  with a much smaller, fully-auditable surface inside one contract. If a
  future need specifically requires a *classic* claimable balance (e.g.
  external tooling that only understands that ledger entry type), that's a
  deliberate reconsideration, not a "fix."
- **Why `claim_tranche`/`reclaim_tranche`/`open_tranche` are private
  functions parameterized by a `Tranche` enum, rather than each having a
  separate hand-written implementation for tranche 1 and tranche 2**: two
  independently-maintained copies of the same claim/reclaim logic is
  exactly the kind of duplication where one copy quietly gets a bugfix and
  the other doesn't. One implementation, called by four thin public
  wrappers (`claim_advance_1`, `claim_advance_2`, etc.), means the rules
  can't drift between tranches by construction.
- **Why `settle` requires both tranches already resolved, rather than
  auto-resolving whatever's left**: this is the one genuine bug this
  session's audit caught before it shipped. The first version of `settle`
  swept the contract's balance to the cooperative unconditionally,
  marking any still-open tranche as claimed. That's wrong: if a tranche's
  claim window had already passed but the buyer simply hadn't gotten
  around to calling `reclaim_advance_*` yet, that sweep would silently
  hand the buyer's already-vested reclaim right to the cooperative
  instead — no adversarial timing required, just an inactive buyer and a
  cooperative or buyer calling `settle`. Requiring explicit resolution
  first means every stroop's destination is always the result of an
  actual `claim`/`reclaim` call, never something `settle` infers. The
  cost is one extra required call per unresolved tranche before
  settlement can complete — worth it for removing the ambiguity entirely.
  **If you're tempted to "simplify" `settle` back to auto-resolving,
  re-read this paragraph first.**
- **Why state is mutated and saved *before* the external token transfer
  in `lock`, `claim_tranche`, `reclaim_tranche`, and `settle`
  (checks-effects-interactions ordering)**: defensive hardening against a
  hypothetical future token with transfer hooks that could call back into
  this contract. The current native/SAC token has no such hook, so this
  isn't closing a demonstrated exploit — but Soroban's atomicity guarantee
  (a panic anywhere unwinds the *entire* invocation, including earlier
  storage writes) means this ordering costs nothing in the failure case
  and only helps in a reentrancy scenario, so there's no reason not to.
- **Why `release_advance_1`/`release_advance_2` still have no
  `require_auth` call**: opening a tranche only starts a clock — it moves
  no funds, and the recipient/amount of any later claim are fixed at
  `initialize` regardless of who calls `release_advance_*`. There's no
  privilege to gate.
- **Why multi-party functions call `require_once_each`, not a raw
  `require_auth()` per named party**: Soroban's auth manager rejects a
  *second* `require_auth()` for the **same** address within a single call
  frame (`Auth/InvalidAction`) — the frame is already marked authorized and
  there's no further matching entry left to consume. When one principal
  legitimately holds two of a commitment's named roles (a cooperative that
  also operates the warehouse; a buyer that is also the cooperative), asking
  twice aborts the whole invocation, turning a valid single-signature
  authorization into a hard failure. `cancel`, `reassign_buyer`, and
  `resolve_dispute` therefore route their multi-party checks through
  `require_once_each`, which calls `require_auth()` once per *distinct*
  address. This was latent because every auth test ran under `mock_all_auths()`,
  whose recording mode tolerates the duplicate; the `#139`/`#140` tests use
  `mock_auths(&[...])` with explicit per-address entries to pin the enforcing
  behavior.
- **Why `mark_checkpoint`/`confirm_delivery` *are* auth-gated to the
  warehouse operator**: these represent a judgment call by a specific
  trusted party (PRD §4.1 — the warehouse operator is the enforcement
  backstop), not a mechanical consequence of contract state. Checkpoint
  status is independent of whether tranche 1 was actually claimed yet —
  crop progress doesn't wait on paperwork.
- **Why one contract instance per commitment, not one contract managing
  many commitments**: matches PRD §4.8 directly. It also means there's no
  need for a caller to pass a commitment ID anywhere. Don't refactor to a
  multi-commitment registry without updating the PRD first — that's an
  architecture change, not a code change.
- **`saturating_add` for the bps check in `initialize`**: with `u32`
  inputs that could theoretically be very large, a normal `+` would panic
  on overflow (a trap, not a graceful `Error::InvalidBps`), while
  `saturating_add` guarantees the comparison against 10,000 always
  executes and rejects cleanly.
- **Why `lock` only escrows the deposit, not the full `total_amount`**:
  this is what makes buyer default a *deadline*, not something requiring a
  person to assert fault. If `lock` still pulled everything upfront, there
  would be no clean on-chain signal that the buyer failed to pay the
  remainder — the money would already be there. Splitting funding into two
  phases (deposit at `lock`, remainder at `fund_remainder`) turns "did the
  buyer pay the remainder in time" into the same kind of deadline check
  `claim_tranche`/`reclaim_tranche` already use, rather than needing new
  machinery.
- **Why `expire_remainder_window` is permissionless but `reclaim_on_nondelivery`
  is buyer-gated**, even though both are "deadline passed, sweep the
  balance" in shape: the *destination* differs. `expire_remainder_window`
  always pays the cooperative — a fixed party, so the outcome doesn't
  depend on who calls it, same reasoning that would apply to
  `reclaim_tranche` if it weren't already scoped to the buyer specifically.
  `reclaim_on_nondelivery` pays whoever calls it their own reclaim right
  (the buyer, specifically, since only the buyer's `require_auth()` is
  checked) — gating it prevents a third party's call from being confused
  for the buyer's own decision to reclaim.
- **Why `Defaulted` and `Forfeited` are two separate status variants, not
  one shared "commitment failed" status**: they represent opposite
  parties' failure — a buyer not paying vs. a cooperative not delivering —
  and this session's product decision treats the two very differently
  off-chain (buyer default is an immediate permanent bar; cooperative
  forfeiture is a 3-strike system before a bar). Collapsing them into one
  status would make an already-settled commitment's on-chain history
  ambiguous about who actually failed, forcing the off-chain reputation
  system to infer fault from *which function* fired instead of just
  reading `status`.
- **Why `confirm_delivery`'s guard changed to require `remainder_funded`,
  not just the right status**: without it, a cooperative could confirm
  delivery — and therefore eventually `settle` — while the buyer still
  owed the remainder, since `ReadyForDelivery` alone doesn't imply the
  remainder arrived. The two are checked as separate guards (wrong status
  vs. status-right-but-unfunded) so the specific `RemainderNotFunded`
  error tells a caller exactly which precondition failed, rather than a
  generic `InvalidState` covering both.
- **Why `resolve_fx_shortfall` is its own step, not folded into
  `confirm_delivery`**: `confirm_delivery` only requires `remainder_funded`,
  not both advance tranches resolved — claiming/reclaiming an advance can
  still happen after delivery is confirmed. Computing the FX comparison
  at `confirm_delivery` time would use a `claimed_by_coop`/escrowed-balance
  snapshot that could still change before `settle` actually runs, since
  tranche resolution isn't required yet at that point. Splitting it into
  its own step, gated on the *same* both-tranches-resolved requirement
  `settle` itself has, means the numbers it records are already final —
  the same "explicit resolution required, never inferred from a snapshot"
  principle `settle`'s own tranche-resolution requirement established
  first, applied to a second kind of number that can also still move.
- **Why `resolve_fx_shortfall` is permissionless but `fund_fx_shortfall`
  is buyer-gated**, even though `expire_fx_shortfall_window` mirrors
  `expire_remainder_window`'s reasoning exactly: `resolve_fx_shortfall`
  only computes and records a number — it doesn't move funds or grant
  anyone anything, same category as `release_advance_*`. `fund_fx_shortfall`
  moves the buyer's own money specifically, so it needs their auth, same
  as `fund_remainder`.
- **Why `expire_fx_shortfall_window` reuses `Status::Defaulted` instead
  of a new status**: an explicit product decision this session, not an
  implementation shortcut — asked directly ("what happens if the buyer
  doesn't fund the shortfall in time?") and answered "same as the
  existing buyer-default policy." Reusing the status means the API's
  existing reputation consequence (immediate permanent bar on any fresh
  transition into `Defaulted`) applies automatically, with zero API
  changes needed to enforce it consistently with the original
  `expire_remainder_window` default.
- **Why TTL maintenance lives in `load()`/`initialize()` rather than in
  every mutating method** (#98): every key this contract has — the
  `Commitment`, the `OracleConfig`, each `Allocation` entry, and the
  dispute fields (which live *inside* `Commitment`, not as separate keys)
  — lives in one instance storage entry sharing one TTL, and Soroban does
  **not** auto-extend an instance on invocation. `load()` is the single
  choke point every state-reading call passes through, so the private
  `bump_instance_ttl` helper there (plus one direct call in `initialize`,
  the only entry point that doesn't read through `load`) keeps the whole
  entry — the host bumps the WASM code entry alongside it — live for any
  active commitment without scattering rent management across 29 methods.
  The threshold is `max_ttl / 2`, so a commitment in normal use pays
  extension rent once per half-max TTL (amortized), not per call. The
  residual gap this deliberately does not paper over: a commitment
  touched zero times for more than `max_entry_ttl` ledgers still
  archives; recovery is off-chain (`stellar contract extend`/`restore`,
  or polling a read path like `get_status` as a keeper — every read
  re-rents). Full audit: `docs/storage-ttl.md`.

## Next steps, in priority order

Matches `ROADMAP.md` (main repo) Phase 0 Track B — the claimable-balance
item that used to be #1 here is done; everything shifts up by one.

1. ~~**Allocation ledger** (Week 4-5) — salted-hash member entries, decided
   per-contract-instance.~~ **Done, contract side** (Deployment 7, 4 Sept
   2026, see above) — per-*member* salts (stronger than the per-contract
   floor PRD §16.1 asked for), record-only settlement (PRD §4.9's own v1
   default). **The off-chain identity map (API's Postgres, the actual
   NDPA-erasability half) is the next item** — this contract only stores
   the hash, the salt+phone-number mapping that makes it erasable has to
   live off-chain.
2. ~~**Settlement logic against a real attestation** (Week 5-6) — replace
   the boolean `confirm_delivery` with something that takes delivered
   quantity/grade and applies PRD §7's adjustment schedule, plus the
   oracle staleness bound from §16.3.~~ **The quantity/grade half is
   done** (Deployment 6, 3 Sept 2026) — see above. **The oracle staleness
   bound (§16.3) is still open**, and stays open until NGN/oracle work
   starts at all (item 3 below) — nothing to bound yet.
3. ~~**Assignability** (Week 6-7) — buyer position transfer with cooperative
   consent.~~ **Done** — `reassign_buyer`, see above. Went one signer
   further than "with cooperative consent" alone implies; see its doc
   comment for why.
4. ~~**Regression tests for the remaining edge cases** (Week 7-8) — partial
   delivery, over-delivery, buyer default, side-selling forfeiture.~~
   **All done now** — buyer default and seller-non-delivery via
   `expire_remainder_window`/`reclaim_on_nondelivery` (58/58 unit tests,
   three-scenario live testnet verification); partial delivery and
   over-delivery via `confirm_delivery`'s `settlement_bps` math
   (Deployment 6, 9 new unit tests, live-verified — see above).
5. ~~**Decide the `claim_window_secs` minimum question**~~ — **already
   decided**, this item just never got updated to say so (see "What's
   deliberately NOT implemented," item 5, for the actual reasoning,
   which was written the same session as the API-side fix but never
   echoed back here). The answer: bound it at the API layer (`api/src/
   server.ts`, 1 hour floor / 90 day ceiling, checked before an
   `initialize` transaction is even built), leave the contract itself
   unbounded on purpose — this was never a security boundary, only a
   business-judgment default, and the contract is the wrong layer for
   that judgment call since anything invoking `initialize` directly
   (a different future API, a test, `stellar-cli`) bypasses an
   API-level check entirely regardless. Nothing left to decide here.
6. ~~**NGN/oracle conversion** (PRD §4.2/§16.3)~~ — **done** (Deployment
   8, 5 Sept 2026: the staleness-bound read; Deployment 9, 6 Sept 2026:
   `settle` payout wiring, PRD §4.2 option (b), see above), live-verified
   with a genuine cross-rate top-up on real testnet. What's still open:
   Reflector's real testnet fiat oracle doesn't quote NGN at all yet
   (confirmed live, not assumed) — the actual currency this feature is
   for has no feed to convert against. Not this contract's problem to
   solve; activates automatically the day Reflector adds it.
7. ~~**Dispute path** (PRD's must-have "dispute flagging with defined
   escalation")~~ — **done** (Deployment 10, 6 Sept 2026, see above):
   `flag_dispute`/`resolve_dispute`/`expire_dispute_window`. Still not
   arbitration of who's at fault — a bounded pause only, by explicit
   design; see the "What's deliberately NOT implemented" entry above for
   why that stays true on purpose.

~~Deploy to testnet, exercise the happy path end-to-end.~~ **Done, five
times now.** ~~Claimable-balance-with-expiry for the advance tranches.~~
**Done, a previous session** — see "Verified on testnet" above for the
live proof, including the negative case (`settle` correctly rejected
on-chain before resolution). ~~Mutual cancellation (`cancel`).~~ **Done**
— 30/30 unit tests at the time, plus a genuine two-signer live-testnet
run via the `api/` repo's SDK layer. ~~Assignability (`reassign_buyer`).~~
**Done** — 34/34 unit tests, plus a genuine three-signer live-testnet
run, including a functional proof (reclaim rights actually transfer, not
just the field). ~~Buyer default / seller-non-delivery forfeiture, two-phase
funding.~~ **Done** — 58/58 unit tests, three separate live-testnet
scenarios (happy path, buyer-default sweep by an unrelated third-party
signer, seller-non-delivery reclaim). See "Verified on testnet" above for
all of it.

## If you're an AI agent picking this up cold

Read, in this order: this file, then `contracts/escrow/src/lib.rs` (it's
short, read the whole thing, the doc comments carry real design intent),
then `contracts/escrow/src/test.rs`. Run `cargo test` before changing
anything, to confirm your starting point actually matches this document —
if it doesn't, trust the code and the test output over this file, and fix
this file to match before doing anything else.

---
*Last updated: 20 Oct 2026 — the delivery group's resource-cost pass:
`mark_checkpoint` and `confirm_delivery` measured for the first time, on a
deployed testnet instance, with
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator).
**Docs only — no contract code changed, no test added or removed.** The
measured baseline is now **211 tests passing** (measured, not carried over:
165 in `src/test.rs`, 46 across `tests/` — 7 `address_equality`, 12
`deadline_boundary`, 13 `oracle_fault_injection`, 10
`permissionless_release_review`, 4 `snapshot_replay`). `cargo fmt --check`,
the two guard scripts (discriminants 1..38 sequential; all 30 public
methods' doc comments name their auth party) and the repo's advisory clippy
invocation are all green at this HEAD.

What was measured, and what it found (full write-up, with reproduce steps
and the CLI cross-check, in [`docs/gas-costs.md`](./docs/gas-costs.md)):

- `mark_checkpoint` — 1,523,347 CPU instructions, 2 read / 2 write entries,
  4,579 non-refundable stroops, 906,370 total simulated.
- `confirm_delivery` — 1,530,217 CPU instructions, 2 read / 2 write entries,
  4,584 non-refundable stroops, 906,976 total simulated. Real submitted
  transactions were charged 4,181 and 4,242 stroops respectively — 99.5% of
  the estimator's "total" is simulated storage rent, not execution.
- **The auth-gated calls are the expensive ones, not the money-moving
  ones.** Both calls move zero stroops and still cost ~53% more CPU than
  the permissionless `release_advance_1` (996,645 on the same build, same
  instance, same balance) and within 9% of `claim_advance_1` (1,671,867),
  which does move 15,000,000 stroops. The delta appears only on the
  auth-gated path — one extra read entry, one extra write entry, ~527k CPU
  instructions. Worth knowing before anyone reads "cheap call" as "cheap".
- A **real upstream tooling limitation**, documented in full in
  `docs/gas-costs.md`: `soroban-cost-estimator` v0.1.0's `--arg` infers an
  argument's type from the literal alone (`ScVal::I64` for any integer),
  ignoring the contract spec it has already parsed — so it cannot invoke
  any contract with a `u32`/`i32`/`u64` parameter at all. `confirm_delivery`
  takes two `u32`s, so the stock binary aborts with
  `Error(WasmVm, InvalidAction)` before the contract body runs. Upstream
  `main` (commit `9627919`) has the same defect. The measurement used a
  two-hunk patch that encodes `--arg` with the spec's declared type; the
  patch touches argument encoding only and was verified to leave every
  measured value byte-identical on the zero-argument path. **Worth
  reporting upstream.**
- Deployment 11 (this session): WASM hash `e097f1ff…`, 60,043 bytes, four
  instances, all validation artifacts — see "Verified on testnet" above.

Prior entry: 17 Oct 2026 (later) — issues **#1** and **#2**, one new
contract call plus a written security review. 210 tests total, measured:
164 in `src/test.rs` (6 new), 46 across `tests/` (7 `address_equality`,
12 `deadline_boundary`, 13 `oracle_fault_injection`, 10 new
`permissionless_release_review`, 4 `snapshot_replay`). **#1 — the
"waiting on a fresh quote" signal**: a Soroban invocation returning
`Err` rolls back every write, so `resolve_fx_shortfall`'s `OracleStale`
could never persist a marker of why settlement is stuck. New
permissionless `refresh_fx_quote_status` runs the same staleness check
and records the answer in the new `Commitment::fx_quote_stale` (struct
field 33), which `get_commitment` surfaces; a fresh probe clears it, and
so does a successful `resolve_fx_shortfall`, so it can't linger past
resolution. A *missing* quote propagates as an error rather than setting
anything — "never quoted at all" is a config gap, not "come back later."
Refuses `InvalidState` outside `Delivered`, `OracleNotConfigured`,
`FxAlreadyResolved`. Deliberately a boolean alongside `fx_resolved`,
not a `Status` variant (a condition on one settlement sub-step, not a
lifecycle position — plus every extra variant is one more restore target
the dispute freeze/resume pair would need to reason about);
informational only, no guard reads it, worst case is a misleading
display. 6 new tests in `src/test.rs`, none removed — measured lib
total 164, which corrects the prior entries' 159/156 by one (the
count line above is the measured one). **#2 — permissionless
release review, on the record**: `docs/permissionless-release-review.md`
pins the verdict that `release_advance_1/2` stay permissionless (no auth
gate added): clock written exactly once, no funds move, lapse beneficiary
is always the buyer, cooperative always gets the full
`claim_window_secs` from whenever the window opened — with "what would
change the verdict" spelled out (a term fixed at `initialize`, not a
signature gate). Every claim is pinned by the new 10-test suite
`contracts/escrow/tests/permissionless_release_review.rs`, given its own
CI job in `.github/workflows/test.yml`, including the issue's
end-to-end third-party griefing scenario. Doc comments on both calls
updated to state the review's conclusion; `docs/authorization-matrix.md`
(29 → 30 public functions, 7 → 8 permissionless, new row) and
`docs/threat-model.md` (§21 reworded with the #2 suite named, roles
table updated) brought in line; `docs/ts-bindings-comparison.md` stopped
citing a now-drifting field count. Guard scripts re-run green after the
new method: discriminants 1..38 sequential, auth docs cover all 30
public methods. Snapshots: the record-only convention applied harder
this session — the new suites' snapshots are untracked and the 136
tracked files that a local re-run re-recorded (the new `fx_quote_stale`
field, `live_until` drift) were restored to HEAD rather than committed,
same reasoning as the prior entries below.

Prior entry: 17 Oct 2026 — #63, the Reflector `decimals()` question:
documented finding plus one test, no contract behavior changed. `oracle_rate()`
performs *no* conversion math — it returns the raw `price` from `lastprice`
and never calls the oracle's `decimals()` — so a change to Reflector's
`decimals()` cannot affect it; the issue's "oracle_rate()'s conversion math"
is a slight misattribution. The only place `decimals()` enters the math is
`resolve_fx_shortfall`'s `10i128.pow(decimals)` divisor, read live per
settlement. The contract's implicit assumption — that `decimals()` is
immutable — is now verified rather than assumed: Reflector's real contract
(`oracle/src/settings.rs`) writes `decimals` once in `init`, which aborts with
`AlreadyInitialized` on a second call, and exposes no `set_decimals`; SEP-40
states the value "should never change"; Reflector's own integration docs say
the same. New `docs/reflector-decimals-assumption.md` records the finding in
full. New test
`a_plausible_decimals_change_silently_rescales_resolve_fx_shortfall`
(`tests/oracle_fault_injection.rs`) pins the behavior if the invariant is ever
violated: a *plausible* new scale (15 vs 14) is accepted with no error and
silently rescales the conversion by a power of ten, whereas the separate F5
finding covers an absurd value that *traps* instead. Doc comments in
`reflector.rs` and `lib.rs` (`OracleConfig`, `resolve_fx_shortfall`) now state
the assumption instead of leaving it implicit. 156 lib tests unchanged;
`oracle_fault_injection` 12 → 13 (192 total). No contract code path changed.
Snapshots for the new test deliberately **not committed** — same record-only
reasoning as below.

Prior entry: 16 Oct 2026 (night) — exact-deadline boundary coverage for all
four absolute deadlines (#112–#115): tests only, plus doc-comment corrections,
no contract behavior changed. Each of `remainder_deadline`,
`delivery_deadline`, `fx_shortfall_deadline` and `dispute_deadline` now has a
test that reads the deadline back out of contract state, sets the ledger clock
to exactly that timestamp, and asserts both sides of it — previously every
deadline test used a clearly-before/clearly-after pair, so the
exclusive/inclusive edge each guard implements was never actually proven.
What the four guards do, now stated in `lib.rs` instead of left implicit: the
deadline second is the *last* second the original window is open
(`fund_remainder` and `fund_fx_shortfall` inclusive; `resolve_dispute` carries
no deadline gate at all), and every deadline-triggered failure path
(`expire_remainder_window`, `expire_fx_shortfall_window`,
`reclaim_on_nondelivery`, `expire_dispute_window`) needs `now` strictly greater
than the deadline. Confirming that surfaced two doc contradictions, both
fixed: `FxShortfallWindowNotPassed` said "called before the deadline" while its
guard is `now <= deadline`, and `DisputeWindowNotPassed`'s wording implied
`resolve_dispute` could produce it, which it can't. Mutation-checked before
committing: flipping any of the four comparison operators, or loosening either
funding guard by one second, turns the corresponding test red — these tests
pin the boundary rather than passing vacuously. 143 → 147 tests. Snapshots for
the four new tests once again deliberately **not committed** — same
record-only reasoning recorded in the prior entry below.

Prior entry: 16 Oct 2026 (later evening) — bundled issue-response session
(#68–#71), one contract-behavior change, one new test, two written/comment
answers. **#69**: the contract now emits a **`status_changed` event on
every real transition** (`#[contractevent]`, `from`/`to` as topics;
`initialize` ships no event — creation is not a transition). All 13
mutation sites route through a new private `set_status` helper, the only
place `status` is assigned after `initialize`, so the property can't rot.
Migration was mechanical — the behavior of every existing method is
unchanged apart from the emitted event. One hard-won test-harness fact,
recorded in `test.rs`: **`env.events().all()` scopes to the last contract
invocation, not the whole test** (the trait doc means it literally — a
first cut asserting the full accumulated sequence across ten calls
failed against exactly the final call's event); the new `assert_transition`
/ `assert_no_transition` helpers assert per-call, which also pins that
non-transitions (claims, `fund_remainder`, `settle`'s payout) emit
nothing. **#68**: `docs/constructor-batching.md` — recommendation is
**not to add a `__constructor`**: on protocol 22+ every deploy invokes it,
so a parameterized constructor is a breaking *replacement* for every
existing deploy path (the API's bare deploy with empty `constructorArgs`
would fail outright), it could only fold deploy+initialize (one
`InvokeHostFunctionOp` per tx — initialize+lock batching is impossible),
and the Draft window it would delete is exactly where PRD §4.9's
`set_allocation` ledger must be written. Flip conditions and the full
adoption ripple are listed in the doc. **#70**: genuine gap —
`cannot_mark_checkpoint_twice` added. **#71**: already covered —
answered with a comment naming
`cannot_call_ready_for_delivery_before_advance_2_released`, no new test.
138 → 143 tests. Snapshot note for whoever re-runs locally: this change
makes several previously-`"events": []` snapshots re-record *with* the new
event — deliberately not committed here, consistent with the record-only
finding below.

Prior entry: 16 Oct 2026 (evening) — bundled issue-response session
(#64–#67), one contract-behavior change plus three written deliverables.
**#65**: `grade_price_bps` (`initialize`) and `set_allocation`'s member
list were the contract's only two unbounded `Vec` inputs — neither the
contract nor the `api/` layer capped them. Both now rejected over-cap at
call time: `MAX_GRADE_ENTRIES = 16` (the PRD's grading examples are 3
tiers; 16 is headroom, not a researched maximum) and
`MAX_ALLOCATION_MEMBERS = 200` (anchored to PRD §16.3's 200-farmer
cooperative scale), via new terminal error variants
`GradeScheduleTooLong = 37` and `AllocationTooLarge = 38` appended to the
enum and the discriminant snapshot. 4 new boundary tests (at-cap
accepted, over-cap rejected, both lists), 138/138 total. **#66**:
`docs/authorization-matrix.md` — every `require_auth()` site in `lib.rs`
mapped function-by-function against the PRD (all 29 public functions: 13
single-party gates, 7 permissionless, 3 multi-party, 1 flagger-checked,
5 no-auth views), with the two deliberate divergences named so neither
gets "corrected" back: `reassign_buyer`'s third signer (beyond §4.8's
"cooperative consent") and the member-list "joint amendment" clause
having no on-chain path. **#64**: `docs/ts-bindings-comparison.md` —
regenerated `stellar contract bindings typescript` output from this
repo's own WASM and byte-compared it against the `api/` app's hand-built
ScVal encoders: all 13 `initialize` args plus the `set_allocation`
members vector come out **byte-identical**, both documented encoder
pitfalls (u32 inference, manual map key ordering) are structurally
eliminated by the spec-driven path, recommendation is to adopt it
app-side as a follow-up there. **#67**: soroban-sdk version check
documented in the toolchain section above — 28.0.0 pinned vs 29.0.1
latest, no API-affecting changes across the gap, bump deliberately left
to dependabot.
Also worth recording because it shaped this PR: the
`contracts/escrow/test_snapshots/test/*.json` files are **record-only**
in this repo's setup — the test harness writes them but never compares
or gates on them (checked the hard way: hand-editing a snapshot's
`share_bps` value and re-running still passes; CI runs plain
`cargo test`). The random-`BytesN` allocation tests re-record different
hash bytes on every local run. That's why the four new tests'
snapshots were deliberately **not committed**: the 200-member one alone
is ~8,700 lines of JSON with nothing load-bearing in it, and this repo's
auto-merge bot holds PRs adding more than 1,000 lines for manual review
(MFrancis's test-only PR #158 has sat open for exactly that reason).

Prior entry: 16 Oct 2026 (later same day) — docs-only session, no contract
code touched. Consolidated the two dev-environment gotchas into
`docs/dev-setup-troubleshooting.md` (Windows `cargo install stellar-cli`
failing on a missing `dlltool.exe`, and the two native dev libraries a fresh
Linux runner needs — dbus-1 for `libdbus-sys`, libudev for `hidapi`,
discovered one at a time), linked from the README. Extracted the deployment
history above into `CHANGELOG.md`: Keep a Changelog, one entry per
deployment, cross-linked here rather than duplicated. The two
test-verification issues came back **already covered, not gaps** — two of
three distinct parties on `resolve_dispute` already fails and leaves the
commitment `Disputed`
(`resolve_dispute_requires_three_distinct_signatures_when_parties_are_distinct`),
and a commitment initialized without an `oracle_config` already reads `None`
(`get_oracle_config_is_none_when_initialize_never_set_one`); both answered
with a comment naming the test, and no new tests added. Three current-state
corrections, each so no two docs here contradict each other: **Deployment 8
is 4 Sept, not 5 Sept** (the instance it names, `CCUDE3MM…`, was created
2026-09-04 22:16 UTC and its WASM hash matches the one above); the toolchain
section names `soroban-sdk = "28.0.0"`, which is what `Cargo.toml` says since
dependabot PR #5; and the test count is 134, not 129 (measured on `main`).

Prior entry: 16 Oct 2026 — auth-model scrutiny of the staged multi-party
Soroban flow (#138–#141). **#138**: proven a staged auth entry whose
`signature_expiration_ledger` lapses is rejected *by the host*, before the
contract body runs — it surfaces as a host auth/`InvokeError::Abort`, not a
contract-returned `Error`, and the aborted invocation leaves no partial
state (the expiry check is atomic with the call). **#140/#141**: proven
`resolve_dispute`'s three-party check genuinely needs three *distinct*
signatures when the parties are distinct, that a `MockAuth` entry scoped to
one `fn_name`/argument set cannot authorize a different method or a
different argument set, and that a stranger's signature satisfies no role.
**#139 surfaced a real bug**: calling `require_auth()` twice for the same
address in one call frame fails under enforcing auth (`Auth/InvalidAction`)
even though `mock_all_auths()` tolerates it — so `cancel` (buyer ==
cooperative), `reassign_buyer`, and `resolve_dispute` were broken on-chain
for a single principal holding overlapping roles. Fixed with an internal
`require_once_each` helper that calls `require_auth()` once per distinct
address; the three methods now route their multi-party checks through it.
8 new unit tests (129/129 total), all under explicit `mock_auths(&[...])`
rather than `mock_all_auths()` so rejection is actually observable.

Prior entry: 16 Oct 2026 — audited Commitment struct on-chain footprint (#143): conducted a comprehensive field-by-field necessity review across all 32 fields. Confirmed that every field serves an active role in state transition enforcement, auth checks, financial payout calculations, or dispute recovery, and cannot be derived off-chain because smart contracts cannot query historical events. Retained struct as-is to preserve off-chain API compatibility and avoid instruction overhead from bitmask packing. Documented findings in `lib.rs` and added initialization invariant test coverage in `test.rs`.

Prior entry: 6 Sept 2026 (later same day) — dispute flagging with
defined escalation (PRD must-have), Deployment 10:
`flag_dispute`/`resolve_dispute`/`expire_dispute_window`. Closes the
"no dispute path" item that's existed since the very first version of
this file — still not arbitration of who's at fault, just a bounded
freeze (unilateral to open, unanimous or timed-out to close), by
explicit, documented design. 17 new unit tests (120/120 total),
live-verified on real testnet: `flag_dispute` and `expire_dispute_window`
directly via `stellar-cli`, `resolve_dispute`'s three-party path via the
`api/` repo's staged multi-party HTTP layer. See "Verified on testnet"
above for the full walk.

Prior entry (2 Sept 2026, later same day) — two-phase funding plus the
buyer-default / seller-non-delivery forfeiture paths, per this session's
product decisions on default thresholds and penalty severity. `lock` now
escrows only the deposit; `ready_for_delivery` + `fund_remainder` handle
the remainder; `expire_remainder_window` (permissionless, sweeps to
cooperative, immediate-bar buyer default) and `reclaim_on_nondelivery`
(buyer-gated, 3-strike-eligible seller forfeiture) are the two new
terminal, uncontested-by-construction outcomes — deliberately distinct
`Status::Defaulted`/`Status::Forfeited` variants, not a shared one, since
they represent opposite parties' failure. `confirm_delivery` re-gated to
require the remainder funded. `cancel`/`reassign_buyer` extended to reach
`ReadyForDelivery`. 58/58 unit tests (24 new), rebuilt and reuploaded
(deployment 5, new WASM hash), verified live in three separate real-time
scenarios: the full happy path through two-phase funding, a buyer default
triggered by a genuinely unrelated third-party signer (proving the
permissionless design), and a seller-non-delivery reclaim. Reputation/
strikes tracking, the appeals process, and the site's Roles &
Responsibilities disclosure are all `api/`/`site/`-side follow-on work,
not part of this contract.
Prior entry: added `reassign_buyer` (buyer-position
assignability, PRD §4.8): three-signer-consented (outgoing buyer,
cooperative, incoming buyer — one more than the PRD line alone names),
same reachable-state range as `cancel`. 34/34 unit tests passing (4
new), rebuilt and reuploaded to testnet (deployment 4, new WASM hash),
verified live with three genuinely different signers via the `api/`
repo's SDK layer, including a functional proof that reclaim rights
transfer to the new buyer, not just the field. Same session also fixed
a real process gap: bumping `ESCROW_WASM_HASH` locally isn't the same
as the build being on testnet — `stellar contract upload` still has to
happen, or every deploy fails with "Wasm does not exist."
Before that: added `cancel` (mutual unwind, PRD §7):
buyer+cooperative co-signed, reachable Draft through Advance2Released,
balance-based refund to the buyer. 30/30 unit tests passing (6 new),
rebuilt and redeployed to testnet (deployment 3, new WASM hash), and
`cancel` itself verified live with two genuinely different, correctly-
authorized signers via the `api/` repo's SDK layer — see "Verified on
testnet" for what it took to get a real multi-party Soroban auth call
working (two false starts, both look-alikes for the right answer).
Before that: claimable-balance-with-expiry semantics for both
advance tranches, a real fairness bug in `settle` caught and fixed
during self-audit, reentrancy-hardened transfer ordering, 24 passing
tests at the time. Update the date/context here when you next touch
this repo.*
