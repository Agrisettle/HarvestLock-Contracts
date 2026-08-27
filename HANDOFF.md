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
- `get_status` / `get_commitment` — read-only accessors; `get_commitment` now also exposes each tranche's deadline/claimed/expired fields.

Tests (`src/test.rs`, **24 tests, all passing, zero warnings**) cover, beyond
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
intentional, not a bug — see below).

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

**Both deployed instances are validation artifacts, not infrastructure.**
Redeploy fresh for future testing; don't build anything that depends on
either address continuing to exist or hold correct state.

## What's deliberately NOT implemented yet

Don't assume these are oversights — each one is a scoping decision, listed
so nobody "fixes" them without knowing what they're trading off.

1. **No cancellation, dispute, or default paths.** `Status::Cancelled`,
   `Status::Defaulted`, and `Status::Disputed` exist in the enum (so
   downstream code can match on them) but no function transitions into
   them. PRD §7's mutual-cancellation unwind, buyer-default forfeiture, and
   side-selling forfeiture are all unbuilt.
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
5. **No minimum (or maximum) enforced on `claim_window_secs`.** The buyer
   sets it at `initialize` with no floor. A careless or adversarial buyer
   could set it to something absurdly short (as this session's testnet
   demo deliberately did, for demo purposes), making it practically
   impossible for the cooperative to claim in time and near-guaranteeing
   a reclaim. Whether the contract should enforce a floor (and what a
   reasonable one is) is a genuine open question — it's as much a business
   decision as a technical one, so it's flagged here rather than answered
   with an arbitrary constant. If nothing else changes this, at minimum
   whatever calls `initialize` in a real deployment (the API layer, not
   yet built) should validate this before submitting the transaction.
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
3. **Assignability** (Week 6-7) — buyer position transfer with cooperative
   consent.
4. **Regression tests for the remaining edge cases** (Week 7-8) — partial
   delivery, over-delivery, buyer default, side-selling forfeiture, mutual
   cancellation unwind. None of these have corresponding contract logic
   yet, so this step includes writing the logic, not just the tests.
5. **Decide the `claim_window_secs` minimum question** (see "What's
   deliberately NOT implemented," item 5) — doesn't have to block the
   items above, but shouldn't be forgotten either.

~~Deploy to testnet, exercise the happy path end-to-end.~~ **Done, twice.**
~~Claimable-balance-with-expiry for the advance tranches.~~ **Done this
session** — see "Verified on testnet" above for the live proof, including
the negative case (`settle` correctly rejected on-chain before resolution).

## If you're an AI agent picking this up cold

Read, in this order: this file, then `contracts/escrow/src/lib.rs` (it's
short, read the whole thing, the doc comments carry real design intent),
then `contracts/escrow/src/test.rs`. Run `cargo test` before changing
anything, to confirm your starting point actually matches this document —
if it doesn't, trust the code and the test output over this file, and fix
this file to match before doing anything else.

---
*Last updated: 27 Aug 2026 (second session same day) — added
claimable-balance-with-expiry semantics for both advance tranches, caught
and fixed a real fairness bug in `settle` during self-audit before it ever
shipped, hardened transfer ordering against reentrancy, expanded to 24
passing tests, redeployed and re-verified end-to-end on Stellar testnet
including the negative `TrancheUnresolved` guard live on-chain. Update the
date/context here when you next touch this repo.*
