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

The **happy-path state machine is fully implemented and covered by tests**:

```
Draft --lock()--> Locked --release_advance_1()--> Advance1Released
      --mark_checkpoint()--> CheckpointPassed --release_advance_2()--> Advance2Released
      --confirm_delivery()--> Delivered --settle()--> Settled
```

- `initialize` — validates `total_amount > 0` and `advance1_bps + advance2_bps <= 10_000`, requires buyer auth, sets `Draft`.
- `lock` — requires buyer auth, pulls `total_amount` of the token into the contract via `token::Client::transfer`, sets `Locked`.
- `release_advance_1` / `release_advance_2` — pay the cooperative the relevant bps of `total_amount`. **Not auth-gated** — see "Design decisions" below for why that's deliberate, not an oversight.
- `mark_checkpoint` / `confirm_delivery` — **auth-gated to the warehouse operator**, since these are attestations, not mechanical advances.
- `settle` — pays the cooperative whatever remains after both advances.
- `get_status` / `get_commitment` — read-only accessors.

Tests (`src/test.rs`) cover: initialize sets Draft; can't initialize twice;
lock moves the full deposit; advance release pays the correct bps; the full
happy path pays out exactly `total_amount` in total with nothing stuck in
the contract; each state-guarded function rejects being called out of
order; zero-bps advances are valid and pay nothing; bps summing over
10,000 is rejected at `initialize`.

**Run it**: `cd contracts/escrow && cargo test` — **confirmed passing, 9/9, this session**, clean build with zero warnings.

## Verified on testnet — this is not just "it compiles"

The WASM was built (`stellar contract build`, produces
`target/wasm32v1-none/release/harvestlock_escrow.wasm`, 9,925 bytes
optimized, 9 exported functions) and **deployed to Stellar testnet**, then
the full happy path was walked end-to-end with real transactions against
the native XLM Stellar Asset Contract as the token — not a mock, the actual
testnet SAC.

- **Contract**: `CDUWXPAC2AT353J4UF5WWJVW3ZUMATZH7PGNZ2AEXWOGT2USDRR77JSO`
  ([stellar.expert](https://stellar.expert/explorer/testnet/contract/CDUWXPAC2AT353J4UF5WWJVW3ZUMATZH7PGNZ2AEXWOGT2USDRR77JSO))
- **Token used**: native XLM SAC, `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`
  (this is the deterministic per-network wrapper — get it via
  `stellar contract id asset --asset native --network testnet`, don't try
  to `deploy` it, it already exists and deploying will error
  `contract already exists`, which is expected, not a failure)
- **Commitment**: 1,000,000,000 stroops (100 XLM), 15% / 20% advance split
- **Result**: advance 1 paid 150,000,000 to the cooperative, advance 2 paid
  200,000,000, settlement paid the remaining 650,000,000 — **sums to
  exactly the total, contract balance ends at 0**, final status `Settled`.
  Cooperative's on-chain balance reflects the full amount received.

This is a real, reproducible validation, not a demo built to look
convincing — every number above was read back from the ledger via
`stellar contract invoke ... get_status` / `... balance`, not asserted.
Re-run it yourself with the identities and commands above if you want to
confirm independently rather than trust this document.

**This deployed instance is a validation artifact, not infrastructure.**
Don't build anything that depends on this specific contract address
continuing to exist or hold correct state — redeploy fresh for any future
testing rather than reusing it, since nothing about the deployer or funding
here is meant to be durable.

## What's deliberately NOT implemented yet

Don't assume these are oversights — each one is a scoping decision, listed
so nobody "fixes" them without knowing what they're trading off.

1. **No claimable-balance-with-expiry mechanic.** PRD §4.3/§4.8 describes
   the advance tranches as native claimable balances that revert to the
   buyer if unclaimed within a window. What's built instead is a direct,
   immediate transfer once the state-guard permits it. Implementing real
   expiry-and-reclaim semantics is the next real piece of work on this
   contract — see "Next steps."
2. **No cancellation, dispute, or default paths.** `Status::Cancelled`,
   `Status::Defaulted`, and `Status::Disputed` exist in the enum (so
   downstream code can match on them) but no function transitions into
   them. PRD §7's mutual-cancellation unwind, buyer-default forfeiture, and
   side-selling forfeiture are all unbuilt.
3. **No shortfall/grade adjustment at delivery.** `confirm_delivery` is a
   boolean gate. It doesn't read a warehouse receipt's quantity or grade,
   and doesn't apply the PRD §7 adjustment schedule to the settlement
   amount.
4. **No NGN/oracle conversion.** `total_amount` is treated as already being
   in the settlement asset. PRD §4.2's NGN-denomination-with-stablecoin-
   settlement design, and §16.3's oracle staleness bound, aren't here.
5. **No allocation ledger.** PRD §4.8's per-member salted-hash allocation
   (and the NDPA-driven off-chain identity map from §16.1) don't exist in
   this contract at all yet. This is a separate, substantial piece of work.
6. **`release_advance_1`/`release_advance_2` are not auth-gated.** This is
   explained in "Design decisions," not listed as a gap — but flagging it
   here too because it's the kind of thing a security-minded reviewer will
   flag on sight, and the reasoning needs to travel with the code.

## Design decisions worth knowing before you change anything

- **Why `release_advance_1`/`release_advance_2` have no `require_auth`
  call**: the recipient (cooperative) and amount (a fixed bps of the
  already-locked total) are both fixed at `initialize` and can't be
  redirected by whoever calls the function. Calling it early does nothing
  (state guard blocks it); calling it once eligible just executes the
  already-agreed transfer. There's no privilege to gate. If this
  assumption ever stops holding — e.g., if amounts become
  caller-influenced — this needs to change to require the buyer's or
  cooperative's auth immediately.
- **Why `mark_checkpoint`/`confirm_delivery` *are* auth-gated to the
  warehouse operator**: these represent a judgment call by a specific
  trusted party (PRD §4.1 — the warehouse operator is the enforcement
  backstop), not a mechanical consequence of contract state. Anyone should
  be able to "unlock what's already been earned"; nobody but the operator
  should be able to assert that a checkpoint or delivery happened.
- **Why one contract instance per commitment, not one contract managing
  many commitments**: matches PRD §4.8 directly. It also means there's no
  need for a caller to pass a commitment ID anywhere — every function
  operates on "the" commitment, which simplifies the state model
  considerably. Don't refactor to a multi-commitment registry without
  updating the PRD first — that's an architecture change, not a code change.
- **`saturating_add` for the bps check in `initialize`**: deliberate,
  not a copy-paste habit — with `u32` inputs that could theoretically be
  passed as very large by a malicious caller, a normal `+` would panic on
  overflow (which Soroban would just turn into a trap, not a graceful
  `Error::InvalidBps`), while `saturating_add` guarantees the comparison
  against 10,000 always executes and rejects cleanly.

## Next steps, in priority order

Matches `ROADMAP.md` (main repo) Phase 0 Track B, Week 3 onward — Weeks 1-2
are essentially what's described above.

1. **Claimable-balance-with-expiry for the advance tranches** (Week 3-4 in
   the roadmap, pulled forward conceptually since it changes
   `release_advance_1`/`release_advance_2`'s shape). Needs a design
   decision first: does this use Soroban-native logic (a stored deadline +
   a `reclaim()` function the buyer can call after expiry, entirely inside
   this contract) or actually construct a classic Stellar
   `ClaimableBalanceEntry` via cross-VM interop? Recommend the former —
   it's what's actually buildable cleanly inside one Soroban contract
   without juggling classic-operation interop, and it delivers the same
   user-facing behavior (cooperative claims within a window, buyer
   reclaims after). If you pick the latter, document why here.
2. **Allocation ledger** (Week 4-5) — salted-hash member entries, decided
   per-contract-instance. This is genuinely new surface, not an extension
   of what exists.
3. **Settlement logic against a real attestation** (Week 5-6) — replace
   the boolean `confirm_delivery` with something that takes delivered
   quantity/grade and applies PRD §7's adjustment schedule, plus the
   oracle staleness bound from §16.3.
4. **Assignability** (Week 6-7) — buyer position transfer with cooperative
   consent.
5. **Regression tests for the edge cases** (Week 7-8) — partial delivery,
   over-delivery, buyer default, side-selling forfeiture, mutual
   cancellation unwind. None of these have corresponding contract logic
   yet (see "What's deliberately NOT implemented"), so this step includes
   writing the logic, not just the tests.

~~6. Deploy to testnet, exercise the happy path end-to-end.~~ **Done this
session** — see "Verified on testnet" above. Re-deploy fresh rather than
reusing that instance when you next need a live one.

## If you're an AI agent picking this up cold

Read, in this order: this file, then `contracts/escrow/src/lib.rs` (it's
short, read the whole thing, the doc comments carry real design intent),
then `contracts/escrow/src/test.rs`. Run `cargo test` before changing
anything, to confirm your starting point actually matches this document —
if it doesn't, trust the code and the test output over this file, and fix
this file to match before doing anything else.

---
*Last updated: 27 Aug 2026 — initial contract implementation, 9/9 tests
passing, deployed and exercised end-to-end on Stellar testnet. Update the
date/context here when you next touch this repo.*
