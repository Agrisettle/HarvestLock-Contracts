# HarvestLock-Contracts

Soroban escrow contract (Rust) for [HarvestLock](https://github.com/agrisettle/harvestlock)
— one instance per commitment. Implements the state machine from the PRD, §4.8:

```
Draft → Locked → Advance1_Released → Checkpoint_Passed → Advance2_Released → Delivered → Settled
                 ↓                    ↓                    ↓
             Cancelled            Defaulted            Disputed
```

Split into its own repo so the contract has its own audit trail, its own
release/versioning discipline, and doesn't share history with application
code that changes on a different cadence.

## Status

**Read [`HANDOFF.md`](./HANDOFF.md) first — always.** It's the current,
maintained source of truth for exactly what's implemented, what's
deliberately not, and why. This README won't be kept as precisely in sync;
treat it as orientation, HANDOFF.md as ground truth.

Short version as of this writing: the happy-path state machine
(`Draft` through `Settled`) is implemented in `contracts/escrow` with
passing tests. Claimable-balance expiry semantics, cancellation/dispute
paths, the allocation ledger, and NGN/oracle conversion are not yet built —
all tracked in HANDOFF.md's "next steps," matching
[`ROADMAP.md`, Phase 0 Track B](https://github.com/agrisettle/harvestlock/blob/main/ROADMAP.md#track-b--build-the-contract-weeks-110-in-parallel)
in the main repo.

## Building and testing

```bash
cd contracts/escrow
cargo test              # runs on host target, no Stellar CLI needed
stellar contract build  # produces the deployable .wasm (needs stellar-cli)
```

## Reference

- **PRD** (living document): https://claude.ai/code/artifact/c9a2f2a6-b9f2-4218-b4e8-60651ddfbb5d — see §4.8 (contract design), §4.5 (why this can't hold custody keys), §16.3 (oracle staleness and depeg handling the contract must account for)
- **Org**: [agrisettle](https://github.com/agrisettle)

## License

Apache-2.0 — see [`LICENSE`](./LICENSE).
