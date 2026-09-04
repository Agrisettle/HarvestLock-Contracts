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
- `soroban-sdk = "27.0.6"` — resolved via `cargo add`, not hand-picked.
  Don't downgrade without a reason; don't upgrade without checking the
  changelog against this contract's API usage first.
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

Tests (`src/test.rs`, **58 tests, all passing, zero clippy warnings** beyond
the inherent, unavoidable `too_many_arguments` on `initialize` — one field
per commitment property) cover, beyond last session's set: `lock` escrows
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

**All seven deployed/uploaded instances are validation artifacts, not
infrastructure.** Redeploy fresh for future testing; don't build anything
that depends on any of these addresses continuing to exist or hold correct
state.

## What's deliberately NOT implemented yet

Don't assume these are oversights — each one is a scoping decision, listed
so nobody "fixes" them without knowing what they're trading off.

1. **No dispute path.** `Status::Disputed` exists in the enum (so
   downstream code can match on it) but no function transitions into it —
   arbitrating a *contested* fault claim needs a mechanism this contract
   doesn't have an answer for. **Buyer-default forfeiture and seller-
   non-delivery forfeiture are now both built** (`expire_remainder_window`
   and `reclaim_on_nondelivery`, see above) — both are *uncontested*,
   deadline-triggered cases only, which is why they didn't need to wait on
   a dispute mechanism. **Mutual cancellation is also built** (`cancel`,
   see above) — this item used to cover all three, it no longer does.
2. ~~No shortfall/grade adjustment at delivery.~~ **Built** (Deployment 6,
   3 Sept 2026, see above): `confirm_delivery` takes `delivered_quantity`/
   `grade_index`, computes `settlement_bps` against the pre-agreed
   `grade_price_bps` table, and `settle` pays out against it. What's
   still not here: the warehouse operator's attestation is trusted
   as-is — there's no separate receipt-hash/signature artifact beyond
   the `require_auth()` call itself, and grade/quantity disputes are
   explicitly off-chain (the operator's own appeals process, per the
   PRD's edge-case table) — this contract doesn't arbitrate them.
3. **No NGN/oracle conversion.** `total_amount` is treated as already being
   in the settlement asset. PRD §4.2's NGN-denomination-with-stablecoin-
   settlement design, and §16.3's oracle staleness bound, aren't here.
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
   "Design decisions" — this is deliberate, not new this session, but
   still worth a security reviewer's attention on sight).

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
5. **Decide the `claim_window_secs` minimum question** (see "What's
   deliberately NOT implemented," item 5) — doesn't have to block the
   items above, but shouldn't be forgotten either.
6. **NGN/oracle conversion** (PRD §4.2/§16.3) — the one "must have" v1
   feature from the PRD's feature list with genuinely nothing built yet.
   Now the top remaining settlement-logic item, alongside the allocation
   ledger.

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
*Last updated: 2 Sept 2026 (later same day) — two-phase funding plus the
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
