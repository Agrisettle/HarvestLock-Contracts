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

Not started. First tasks — toolchain setup through the initial state-machine
skeleton — are in the main repo's roadmap:
[`ROADMAP.md`, Phase 0 Track B](https://github.com/agrisettle/harvestlock/blob/main/ROADMAP.md#track-b--build-the-contract-weeks-110-in-parallel).

Dependencies (`soroban-sdk` and its pinned version) are deliberately not
declared yet — they get pinned fresh at the point of actually starting,
not guessed in advance.

## Reference

- **PRD** (living document): https://claude.ai/code/artifact/c9a2f2a6-b9f2-4218-b4e8-60651ddfbb5d — see §4.8 (contract design), §4.5 (why this can't hold custody keys), §16.3 (oracle staleness and depeg handling the contract must account for)
- **Org**: [agrisettle](https://github.com/agrisettle)

## License

Apache-2.0 — see [`LICENSE`](./LICENSE).
