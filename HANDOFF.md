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

The **happy-path state machine is fully implemented and covered by tests**,
and advance tranches now use **real claimable-balance-equivalent semantics**
(claim within a window, reclaim after it lapses) — this was the top item in
last session's "next steps" and is no longer a gap:

```
Draft --lock()--> Locked --release_advance_1()--> Advance1Released
      --mark_checkpoint()--> CheckpointPassed --release_advance_2()--> Advance2Released
      --confirm_delivery()--> Delivered --settle()--> Settled
```

- `initialize` — validates `total_amount > 0`, `advance1_bps + advance2_bps <= 10_000`, and `claim_window_secs > 0`; requires buyer auth; sets `Draft`.
- `lock` — requires buyer auth, pulls `total_amount` of the token into the contract, sets `Locked`.
- `release_advance_1` / `release_advance_2` — **open** a tranche's claim window (set a deadline, advance the status). Move **no funds**. Not auth-gated — see "Design decisions."
- `claim_advance_1` / `claim_advance_2` — **cooperative-auth-gated.** Pays the tranche's bps amount to the cooperative, only if called within the window and not already claimed or expired.
- `reclaim_advance_1` / `reclaim_advance_2` — **buyer-auth-gated.** Returns the tranche's bps amount to the buyer, only if called after the window has passed and not already claimed or expired.
- `mark_checkpoint` / `confirm_delivery` — warehouse-operator-auth-gated attestations, unchanged from before.
- `settle` — **requires both tranches already resolved** (each claimed or expired) or returns `Error::TrancheUnresolved`. Once that holds, pays the cooperative the contract's entire remaining balance. See "Design decisions" for why the resolve-first requirement exists — it's the result of catching a real fairness bug during this session's audit, not an arbitrary constraint.
- `cancel` — **mutual unwind** (PRD §7): requires **both** buyer and cooperative auth in the same call. Reachable from `Draft` through `Advance2Released`, not from `Delivered` onward. Pays the contract's current balance back to the buyer (whatever's already been claimed stays with the cooperative — no clawback), sets `Cancelled`.
- `reassign_buyer` — **buyer-position assignability** (PRD §4.8): requires **three** signatures — outgoing buyer, cooperative, incoming buyer. Same reachable-state range as `cancel`. Rewrites `buyer`; moves no funds, a novation not a trade. The third signature (the incoming buyer's) isn't literally named in the PRD line this implements — added anyway so the current buyer and cooperative can't saddle a third party with the position without that party agreeing to take it on.
- `get_status` / `get_commitment` — read-only accessors; `get_commitment` now also exposes each tranche's deadline/claimed/expired fields.

Tests (`src/test.rs`, **34 tests, all passing, zero warnings**) cover, beyond
the prior session's happy-path/ordering/bps-validation set: opening a
tranche moves no funds; claiming within the window pays the right amount;
claiming before opening, twice, or after the window fails with the specific
right error each time; the exact-boundary case (claiming in the same second
as the deadline still succeeds — the guard is `>`, not `>=`); reclaiming
before the window passes fails; reclaiming after it passes returns funds to
the buyer; reclaiming twice fails; claiming after the buyer already
reclaimed fails, and vice versa; `settle` is blocked with
`TrancheUnresolved` if either tranche is untouched, verified for both
tranches independently; a full happy path where one tranche is claimed and
the other is deliberately left to expire and get reclaimed still sums
`buyer_balance + cooperative_balance == total_amount` exactly; a zero-bps
tranche still requires explicit resolution before `settle` (documented as
intentional, not a bug — see below); `cancel` from `Draft` (nothing to
return), from `Locked` (full refund), and after a partial claim (claimed
share stays with the cooperative, remainder returns to the buyer); `cancel`
rejected after delivery is confirmed and rejected a second time once
already cancelled; an auth-trace check confirming `cancel` genuinely
requires *both* parties, not just one; `reassign_buyer` actually updates
the buyer field, rejected after delivery, and — the functional proof, not
just a field check — reclaim rights genuinely transfer to the new buyer
(the old buyer gets nothing on a subsequent reclaim); an auth-trace check
confirming all *three* parties are required on `reassign_buyer`.

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

**All four deployed/uploaded instances are validation artifacts, not
infrastructure.** Redeploy fresh for future testing; don't build anything
that depends on any of these addresses continuing to exist or hold correct
state.

## What's deliberately NOT implemented yet

Don't assume these are oversights — each one is a scoping decision, listed
so nobody "fixes" them without knowing what they're trading off.

1. **No dispute or default paths.** `Status::Defaulted` and
   `Status::Disputed` exist in the enum (so downstream code can match on
   them) but no function transitions into either. Buyer-default forfeiture
   and side-selling forfeiture (PRD §7) are unbuilt. **Mutual cancellation
   is now built** (`cancel`, see above) — this item used to cover all three,
   it no longer does.
2. **No shortfall/grade adjustment at delivery.** `confirm_delivery` is a
   boolean gate. It doesn't read a warehouse receipt's quantity or grade,
   and doesn't apply the PRD §7 adjustment schedule to the settlement
   amount.
3. **No NGN/oracle conversion.** `total_amount` is treated as already being
   in the settlement asset. PRD §4.2's NGN-denomination-with-stablecoin-
   settlement design, and §16.3's oracle staleness bound, aren't here.
4. **No allocation ledger.** PRD §4.8's per-member salted-hash allocation
   (and the NDPA-driven off-chain identity map from §16.1) don't exist in
   this contract at all yet. This is a separate, substantial piece of work,
   and is now the **top priority** in "Next steps."
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

## Next steps, in priority order

Matches `ROADMAP.md` (main repo) Phase 0 Track B — the claimable-balance
item that used to be #1 here is done; everything shifts up by one.

1. **Allocation ledger** (Week 4-5) — salted-hash member entries, decided
   per-contract-instance. Genuinely new surface, not an extension of what
   exists. This is also the piece most directly tied to the NDPA
   compliance finding in PRD §16.1 — get the salting scheme right the
   first time (per-contract salt, never a bare hash of a phone number),
   since retrofitting it after real data exists would be a migration, not
   a refactor.
2. **Settlement logic against a real attestation** (Week 5-6) — replace
   the boolean `confirm_delivery` with something that takes delivered
   quantity/grade and applies PRD §7's adjustment schedule, plus the
   oracle staleness bound from §16.3.
3. ~~**Assignability** (Week 6-7) — buyer position transfer with cooperative
   consent.~~ **Done** — `reassign_buyer`, see above. Went one signer
   further than "with cooperative consent" alone implies; see its doc
   comment for why.
4. **Regression tests for the remaining edge cases** (Week 7-8) — partial
   delivery, over-delivery, buyer default, side-selling forfeiture. Mutual
   cancellation is now done (see `cancel`, above) — this item used to
   include it, it no longer does. None of the rest have corresponding
   contract logic yet, so this step includes writing the logic, not just
   the tests.
5. **Decide the `claim_window_secs` minimum question** (see "What's
   deliberately NOT implemented," item 5) — doesn't have to block the
   items above, but shouldn't be forgotten either.

~~Deploy to testnet, exercise the happy path end-to-end.~~ **Done, four
times now.** ~~Claimable-balance-with-expiry for the advance tranches.~~
**Done, a previous session** — see "Verified on testnet" above for the
live proof, including the negative case (`settle` correctly rejected
on-chain before resolution). ~~Mutual cancellation (`cancel`).~~ **Done**
— 30/30 unit tests at the time, plus a genuine two-signer live-testnet
run via the `api/` repo's SDK layer. ~~Assignability (`reassign_buyer`).~~
**Done** — 34/34 unit tests, plus a genuine three-signer live-testnet
run, including a functional proof (reclaim rights actually transfer, not
just the field). See "Verified on testnet" above for both.

## If you're an AI agent picking this up cold

Read, in this order: this file, then `contracts/escrow/src/lib.rs` (it's
short, read the whole thing, the doc comments carry real design intent),
then `contracts/escrow/src/test.rs`. Run `cargo test` before changing
anything, to confirm your starting point actually matches this document —
if it doesn't, trust the code and the test output over this file, and fix
this file to match before doing anything else.

---
*Last updated: 2 Sept 2026 — added `reassign_buyer` (buyer-position
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
Prior entry: added `cancel` (mutual unwind, PRD §7):
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
