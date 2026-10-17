# Mutation testing with cargo-mutants (#62)

Mutation testing builds the crate, deliberately breaks one line at a time, and
re-runs the test suite. A mutant that survives — tests still green — is a line
the suite only *looks* like it is checking. `cargo test` counts tests; this
counts **killed** mutants.

This document records the spike: one full run against the escrow contract, the
triage of everything that survived, and what CI now does with it.

## The run

| | |
| --- | --- |
| Date | 2026-10-17 |
| Commit | `0f201c3` (= `main` at the time; contract source unchanged since) |
| Tool | `cargo-mutants 27.1.0` |
| Toolchain | `rustc 1.99.1`, target `wasm32v1-none` not involved (host build) |
| Machine | 2 vCPU Linux, 8 GB RAM |
| Command | `cargo mutants --manifest-path contracts/escrow/Cargo.toml --jobs 2 --timeout 180` |
| Wall time | **41 min** (baseline: 139 s build + 6 s test; then ~10 s build + ~11 s test per mutant) |

Result, as printed by cargo-mutants:

```
219 mutants tested in 41m: 14 missed, 197 caught, 8 unviable
```

| Outcome | Count | Meaning |
| --- | --- | --- |
| Caught | 197 | the suite failed on the mutant — the assertion is doing its job |
| Missed | 14 | **survivors**, triaged below |
| Unviable | 8 | the replacement did not compile; skipped, not counted as failures |
| Timeout | 0 | nothing hung |

Catch rate: **197 / 211 = 93.4%** of the mutants that could run.

### The 8 unviable mutants

All eight are view fns replaced with `Ok(Default::default())` /
`Ok(vec![])` — `get_oracle_config`, `oracle_rate`, `read_oracle_rate`,
`get_allocation`, `get_status`, `get_commitment`, `load`. They never compile
because the contract's `#[contracttype]` structs (`OracleRate`,
`OracleConfig`, `Commitment`, `Status`) do not implement `Default`, so
cargo-mutants discards them before running a single test. Nothing to triage
there; it is a property of the mutant model, not of the suite.

## What survived

Fourteen mutants, in three classes. Class 1 and class 2 are **equivalent
mutants** (no observable behaviour change); class 3 is a **real gap**.

### Class 1 — `amount > 0` guards around token transfers (10 survivors)

`lib.rs` `lock` (821), `fund_remainder` (958), `expire_remainder_window`
(1002), `reclaim_on_nondelivery` (1042), `settle` (1183, 1186),
`expire_fx_shortfall_window` (1349), `cancel` (1396), `claim_tranche`
(1724), `reclaim_tranche` (1755).

Every one is the same shape:

```rust
if balance > 0 {
    token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
}
```

Mutated to `balance >= 0` the contract performs a zero-amount transfer
instead of skipping the call. Balances, commitment fields and status are
byte-identical, so the suite — and any client — sees nothing different. The
guard exists to skip a pointless host call, not to change a result, which is
exactly why cargo-mutants cannot kill it.

**Verdict: equivalent.** Not a bug, and not fixable by a test that asserts
state. If the project ever decides the zero-amount *event* matters (a SEP-41
`transfer` event with amount 0 would appear in the transaction metadata), the
fix is an assertion on the emitted events, not on balances. Until then these
ten are expected noise.

### Class 2 — `settle`'s FX-shortfall guard (1 survivor)

`lib.rs:1153`:

```rust
if c.fx_shortfall_amount > 0 && !c.fx_shortfall_funded {
    return Err(Error::FxShortfallUnfunded);
}
```

`>= 0` makes the first clause always true, so the guard would also reject the
`(amount == 0, funded == false)` combination. That combination is unreachable:
`resolve_fx_shortfall` writes `fx_shortfall_funded = true` whenever the amount
comes out zero, and says so in its own comment ("Nothing owed — trivially
`funded` so `settle()`'s guard reads naturally").

**Verdict: equivalent.** The invariant is real and documented; the suite is
right not to need a test for an unreachable state. This mutant is a useful
reminder that the two fields are coupled by construction.

### Class 3 — real gaps (3 survivors, 2 independent findings)

#### 3a. `initialize` never tests the exactly-10_000 boundary (1 survivor)

`lib.rs:607`:

```rust
if advance1_bps.saturating_add(advance2_bps) > 10_000 {
    return Err(Error::InvalidBps);
}
```

Mutated to `>= 10_000`, a commitment whose advances total *exactly* 100% is
rejected. The suite keeps passing, because the only bps test uses 6_000 +
5_000 = 11_000 (`advance_bps_over_10000_rejected_at_initialize`) — the
reject side of the boundary is covered, the accept side at the cap is not.
`> 10_000` vs `>= 10_000` is a one-character contract change that would break
a legitimate 100%-advance deal and CI would not notice.

**Verdict: real gap — fixed in this PR** by
`advance_bps_summing_to_exactly_10000_is_accepted_at_initialize`.

#### 3b. `resolve_fx_shortfall`'s two tranche guards mask each other (2 survivors)

`lib.rs:1226` and `lib.rs:1229`:

```rust
if !c.advance1_claimed && !c.advance1_expired {
    return Err(Error::TrancheUnresolved);
}
if !c.advance2_claimed && !c.advance2_expired {
    return Err(Error::TrancheUnresolved);
}
```

Deleting the `!` before `advance1_expired` (and likewise for tranche 2) —
i.e. turning the guard into `!advance1_claimed && advance1_expired` — survived.
The reason is the *shape* of the suite, not the strength of it: the only
negative test, `cannot_resolve_fx_shortfall_before_tranches_are_resolved`,
leaves **both** tranches open, so the second guard still throws
`TrancheUnresolved` and the test passes even with the first guard broken.
Each guard is therefore only ever observed through its sibling. The same
overlap exists in `settle`'s copy of these guards, which is why those two
mutants *were* caught: `settle` has a test that leaves only one tranche
unresolved.

**Verdict: real gap — fixed in this PR** by
`resolve_fx_shortfall_rejects_while_the_first_tranche_is_still_open`, which
claims tranche 2 and deliberately leaves tranche 1 open.

#### Evidence

Each fix was checked against the mutant, not just against `main`:

| Test | On `main` | Under the mutant |
| --- | --- | --- |
| `advance_bps_summing_to_exactly_10000_is_accepted_at_initialize` | pass | **fail** (contract rejects the 10_000 cap) |
| `resolve_fx_shortfall_rejects_while_the_first_tranche_is_still_open` | pass | **fail** (`TrancheUnresolved` no longer thrown) |

## What CI now does

`test.yml` gains a `mutation testing (changed lines)` job. Full-crate
mutation testing cannot gate a PR — 41 minutes on two cores here, and that is
after the baseline build — so the job diffs the PR against its base branch and
hands cargo-mutants only the hunks it changed (`--in-diff`). A PR that does not
touch Rust code exits green with "nothing to mutate"; a PR that changes a line
must have tests strong enough to kill the mutants on that line.

Deliberately **not** in this PR:

- **A scheduled full-crate run.** Worth adding once the equivalent-mutant
  noise in class 1 has a policy (baseline file, or event-level assertions).
- **Adding the job to `REQUIRED_CHECKS`.** That list is maintainer-managed in
  `auto-merge.yml`; the PR template forbids contributors from touching it.

## Reproducing

```bash
cargo install --locked cargo-mutants --version 27.1.0
cargo mutants --manifest-path contracts/escrow/Cargo.toml --jobs 2 --timeout 180
```

Outputs land in `mutants.out/`: `missed.txt` (the survivors), `caught.txt`,
`unviable.txt`, `timeout.txt`, plus a per-mutant patch under `mutants.out/diff/`
and the test logs under `mutants.out/log/`. Re-run a single survivor with
`cargo mutants --line 607:54` to see it for yourself.

To make the run deterministic enough to compare across runs, pin the tool
version: a different cargo-mutants version generates a different mutant set.

## Known limitations

- **One point in time.** This is a snapshot of `0f201c3`. Contract code that
  changes materially should re-run it; the CI job only covers changed lines.
- **Mutants, not theorems.** A caught mutant proves an assertion observed
  *that* change; it says nothing about changes no mutant generator models
  (e.g. reordering two independent statements).
- **The unviable eight** are skipped rather than assessed.
- **Runtime is machine-bound.** 41 min on 2 vCPU; a 4-core runner is roughly
  twice as fast, which is why the CI job caps itself to PR hunks.
