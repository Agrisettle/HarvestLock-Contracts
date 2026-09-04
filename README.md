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
stated default). 76 tests passing, deployed and exercised on Stellar
testnet seven times. A contested dispute path (`Status::Disputed`) and
NGN/oracle conversion are not yet built — all
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

## Reference

- **PRD**: [`docs/PRD.md`](https://github.com/Agrisettle/HarvestLock/blob/main/docs/PRD.md) in the `HarvestLock` repo — see §4.8 (contract design), §4.5 (why this can't hold custody keys), §16.3 (oracle staleness and depeg handling the contract must account for)
- **Contributing**: see [`HarvestLock`](https://github.com/Agrisettle/HarvestLock)'s [`CONTRIBUTING.md`](https://github.com/Agrisettle/HarvestLock/blob/main/CONTRIBUTING.md) — one contribution guide for every repo under this org, not a separate one per repo
- **Security**: [`SECURITY.md`](./SECURITY.md) — report vulnerabilities privately, especially anything in this contract
- **Org**: [Agrisettle](https://github.com/Agrisettle)

## License

Apache-2.0 — see [`LICENSE`](./LICENSE).
