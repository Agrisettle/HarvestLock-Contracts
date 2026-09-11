## What this does

<!-- One or two sentences. Link the issue this closes, if there is one. -->

## Why

<!-- The reasoning, not just the change -- see HarvestLock's CONTRIBUTING.md's "Commit messages" section (this repo shares it, not a separate copy). -->

## Checklist

- [ ] **Scoped to one thing** — no adjacent refactors or renames.
- [ ] **`cargo test` passes**, and new behavior has new test coverage, not just a manual `stellar contract invoke` check that isn't written down.
- [ ] **Doc comments updated** — if this changes what `lib.rs`'s own doc comments claim about a function's behavior, they're updated in this PR.
- [ ] **`HANDOFF.md` updated in this PR** if this changes the current-state summary it maintains — not a follow-up that may never happen.

## How you verified it

<!-- `cargo test` output at minimum. If you also live-verified against testnet, say what you deployed/called and link the transaction. -->
