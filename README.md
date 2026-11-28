```
██╗  ██╗ █████╗ ██████╗ ██╗   ██╗███████╗███████╗████████╗██╗      ██████╗  ██████╗██╗  ██╗
██║  ██║██╔══██╗██╔══██╗██║   ██║██╔════╝██╔════╝╚══██╔══╝██║     ██╔═══██╗██╔════╝██║ ██╔╝
███████║███████║██████╔╝██║   ██║█████╗  ███████╗   ██║   ██║     ██║   ██║██║     █████╔╝
██╔══██║██╔══██║██╔══██╗╚██╗ ██╔╝██╔══╝  ╚════██║   ██║   ██║     ██║   ██║██║     ██╔═██╗
██║  ██║██║  ██║██║  ██║ ╚████╔╝ ███████╗███████║   ██║   ███████╗╚██████╔╝╚██████╗██║  ██╗
╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝  ╚═══╝  ╚══════╝╚══════╝   ╚═╝   ╚══════╝ ╚═════╝  ╚═════╝╚═╝  ╚═╝
                                        — Contracts —
```

Soroban escrow contract (Rust) for [HarvestLock](https://github.com/Agrisettle/HarvestLock)
— one instance per commitment. Implements the state machine from the PRD, §4.8:

```
Draft → Locked → Advance1_Released → Checkpoint_Passed → Advance2_Released → ReadyForDelivery → Delivered → Settled
                 ↓                    ↓                    ↓                  ↓
             Cancelled            Defaulted            Disputed          Forfeited
```

Split into its own repo so the contract has its own audit trail, its own
release/versioning discipline, and doesn't share history with application
code that changes on a different cadence.

[![contracts](https://github.com/Agrisettle/HarvestLock-Contracts/actions/workflows/test.yml/badge.svg)](https://github.com/Agrisettle/HarvestLock-Contracts/actions/workflows/test.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)

## Status: pre-pilot, work in progress

**This is not audited, and nothing here is deployed to mainnet.** No
cooperative or buyer has ever used this contract with real funds — every
address, every deployment, every transaction referenced below is
Stellar *testnet*, thrown away after it served its purpose. "67/67
tests passing" and "verified live on testnet" are real, checkable
claims; "ready for real money" is a different claim this repo doesn't
make yet. See [`../HarvestLock`](https://github.com/Agrisettle/HarvestLock)'s
own Status section for what has to happen with a real counterparty
before that changes.

**Read [`HANDOFF.md`](./HANDOFF.md) first — always.** It's the current,
maintained source of truth for exactly what's implemented, what's
deliberately not, and why. This README won't be kept as precisely in sync;
treat it as orientation, HANDOFF.md as ground truth.

**Then read [`docs/architecture.md`](./docs/architecture.md)** — one diagram
of a single commitment's world (escrow instance, WASM, settlement token and
issuer, Reflector, the three parties' wallets, the HarvestLock API) with the
threat model's trust boundaries marked and every arrow linked to the doc or
code that covers it. It's the index into everything under Reference below.

Short version as of this writing: the state machine (`Draft` through
`Settled`) is implemented in `contracts/escrow`, including real
claim/reclaim-with-expiry semantics for both advance tranches, mutual
cancellation (`cancel`, buyer- and cooperative-co-signed), buyer-position
assignability (`reassign_buyer`, three-signer-consented), two-phase
funding with buyer-default / seller-non-delivery forfeiture (`lock`
escrows only the deposit, `fund_remainder` escrows the rest, and two
deadline-triggered terminal paths — `expire_remainder_window` and
`reclaim_on_nondelivery` — cover the uncontested failure cases), and the
PRD §7 shortfall/grade adjustment schedule at settlement
(`confirm_delivery` takes a delivered quantity and grade, `settle` pays
the cooperative only what's still owed against it and refunds the rest
to the buyer as a shortfall credit — already-claimed advances are never
clawed back), and the PRD §4.8/§16.1 allocation ledger (`set_allocation`
records each member farmer's entitlement as a per-member salted hash
plus a share, cooperative-gated, record-only for v1 per PRD §4.9's own
stated default), and PRD §4.2/§16.3's oracle-based FX handling against a
live [Reflector](https://reflector.network) oracle: `oracle_rate`
enforces a configurable max quote age (the staleness bound), and
`resolve_fx_shortfall`/`fund_fx_shortfall`/`expire_fx_shortfall_window`
wire option (b) — buyer tops up or is refunded at settlement — into
`settle`'s actual payout math, live-verified with a genuine cross-rate
top-up on real testnet (see HANDOFF.md's Deployment 9). PRD's must-have
"dispute flagging with defined escalation" is also built:
`flag_dispute`/`resolve_dispute`/`expire_dispute_window` (Deployment 10)
let any one of the three named parties freeze a commitment and either
unanimous consent or a timeout resume it — still not arbitration of
who's at fault, a bounded pause only, by explicit design (see
HANDOFF.md). 213 tests passing, deployed and exercised on Stellar
testnet eleven times. Reflector's real testnet oracle doesn't quote NGN
yet (confirmed live, not assumed — GBP proves the mechanism instead) —
tracked in HANDOFF.md's "next steps," matching
[`ROADMAP.md`, Phase 0 Track B](https://github.com/Agrisettle/HarvestLock/blob/main/ROADMAP.md#track-b--build-the-contract-weeks-110-in-parallel)
in the main repo (this repo doesn't keep its own separate roadmap —
one plan, one place, to avoid the two drifting apart).

## Building and testing

```bash
cd contracts/escrow
cargo test              # runs on host target, no Stellar CLI needed
stellar contract build  # produces the deployable .wasm (needs stellar-cli)
```

CI (`.github/workflows/test.yml`) runs both on every push and PR: `cargo
test` on the host target, and a separate `stellar contract build` job
that also installs `stellar-cli` from source (which needs the system
`libdbus-1-dev`/`libudev-dev` dev headers — already handled in the
workflow; see its comments if that job ever breaks again on a fresh
runner image). A red badge above means something on `main` is actually
broken.

### Calling the escrow from another contract

**Import the built WASM with `soroban_sdk::contractimport!`; do not add
`harvestlock-escrow` as a normal Rust dependency.** The crate is
`crate-type = ["cdylib", "rlib"]`, so the rlib *is* linkable — but a
contract built that way compiles the escrow's whole implementation (state
machine, storage keys, error enum) into the caller's WASM, and reads the
caller's own instance storage rather than the escrow's, because Soroban
storage is per contract address. Importing the WASM instead generates the
client and interface types from the deployed artifact's `contractspecv0`, so
a factory or a pooled-buyer contract can only agree with the escrow about the
wire format and pays a thin `invoke_contract` wrapper rather than a second
copy of the contract.

[`examples/escrow-reader/`](./examples/escrow-reader/) is the worked example
(a contract that reads `get_status` and `get_commitment` across the
boundary, plus its test), and its README covers the trade-off in full. It
needs the escrow WASM first, and is deliberately its own workspace so that
prerequisite does not apply to the root workspace's builds:

```bash
stellar contract build --package harvestlock-escrow
cargo test --manifest-path examples/escrow-reader/Cargo.toml
```

Setting this up for the first time and something won't build?
[`docs/dev-setup-troubleshooting.md`](./docs/dev-setup-troubleshooting.md)
collects the toolchain failures this repo has already hit and how they were
fixed — including why `cargo install stellar-cli` fails on Windows and which
libraries a fresh Linux runner is missing.

## Reference

- **Architecture**: [`docs/architecture.md`](./docs/architecture.md) — start here after HANDOFF.md: what talks to what around one commitment, trust boundaries marked, each arrow linked to its deep-dive doc
- **PRD**: [`docs/PRD.md`](https://github.com/Agrisettle/HarvestLock/blob/main/docs/PRD.md) in the `HarvestLock` repo — see §4.8 (contract design), §4.5 (why this can't hold custody keys), §16.3 (oracle staleness and depeg handling the contract must account for)
- **Authorization model**: [`docs/authorization-matrix.md`](./docs/authorization-matrix.md) — every `require_auth()` site in the contract mapped to the PRD's model, including the two places the code is deliberately stricter than a plain PRD reading
- **Sequence diagrams**: [`docs/sequences.md`](./docs/sequences.md) — who signs what and which contracts get called for `lock`, the multi-party `cancel`/`reassign_buyer`/`resolve_dispute` (including `require_once_each` dedup), `resolve_fx_shortfall` and `settle`
- **Dispute scope**: [`docs/dispute-scope.md`](./docs/dispute-scope.md) — what `flag_dispute`/`resolve_dispute` cover (a bounded pause) and deliberately don't (arbitration)
- **Threat model**: [`docs/threat-model.md`](./docs/threat-model.md) — the actor table and the attack register, with a defended/accepted verdict and the test that holds each defended one
- **Terminal outcomes**: [`docs/terminal-outcomes.md`](./docs/terminal-outcomes.md) — what the buyer, cooperative, and operator actually walk away with in `Settled`/`Defaulted`/`Forfeited`/`Cancelled`, cross-referenced against the off-chain reputation consequences
- **Events**: [`docs/events.md`](./docs/events.md) — every event's topic layout, data fields, emitting calls, the RPC `getEvents` filter for each, and how `contracts/escrow/events.snapshot` pins the serialized shape in CI
- **Dispute stalling**: [`docs/dispute-stalling.md`](./docs/dispute-stalling.md) — whether a losing party gains by flagging a spurious dispute, and why unanimous resolution is not a deterrent
- **Race exposure**: [`docs/dispute-frontrunning.md`](./docs/dispute-frontrunning.md) — the deadline races around `flag_dispute`, `confirm_delivery`, the expiry sweeps, and one-shot FX resolution
- **Griefing by inaction**: [`docs/griefing-and-inaction.md`](./docs/griefing-and-inaction.md) — who can unstick each required action, and the one state where funds are genuinely stranded
- **Downstream contract example**: [`examples/escrow-reader/`](./examples/escrow-reader/) — a minimal contract that imports the escrow's built WASM with `soroban_sdk::contractimport!` and reads `get_status`/`get_commitment` across the boundary, with a test that registers both; its README explains why the WASM, not the rlib, is the supported integration pattern
- **TypeScript bindings**: [`bindings/typescript/`](./bindings/typescript/) — generated by [`scripts/gen-bindings.sh`](./scripts/gen-bindings.sh) from the current WASM (`.wasm-sha256` inside records which build); see [`docs/ts-bindings-comparison.md`](./docs/ts-bindings-comparison.md) for the byte-level comparison against the `HarvestLock` app's hand-built ScVal encoders and the adoption recommendation
- **Test assets & trustlines**: [`scripts/test-asset.sh`](./scripts/test-asset.sh) issues a test classic asset, deploys its SAC, sets up trustlines, and mints initial tokens; see [`docs/trustline-prerequisites.md`](./docs/trustline-prerequisites.md) for the per-role trustline prerequisites when settling in an issued asset
- **Contributing**: [`CONTRIBUTING.md`](./CONTRIBUTING.md) here is a pointer, not a separate guide — see [`HarvestLock`](https://github.com/Agrisettle/HarvestLock)'s [`CONTRIBUTING.md`](https://github.com/Agrisettle/HarvestLock/blob/main/CONTRIBUTING.md) for the real one, shared across every repo under this org
- **Security**: [`SECURITY.md`](./SECURITY.md) — report vulnerabilities privately, especially anything in this contract
- **Org**: [Agrisettle](https://github.com/Agrisettle) ([LinkedIn](https://www.linkedin.com/company/agrisettle))

<a href="https://github.com/Agrisettle"><img src="./docs/wordmark-agrisettle.png" alt="Agrisettle" width="360" /></a>

## License

Apache-2.0 — see [`LICENSE`](./LICENSE).
