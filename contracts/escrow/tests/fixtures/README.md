# `tests/fixtures/` — recorded deployment snapshots (#125)

Every `*.snap` file here is a call/args/expected-outcome transcript of one
scenario that `HANDOFF.md` ("Verified on testnet") records as having actually
run on testnet. `contracts/escrow/tests/snapshot_replay.rs` parses the whole
directory and replays each transcript against the current build.

**Provenance rule, and the limit of it.** No transaction envelope XDR, auth
entry XDR, or ledger export was captured for the early deployments (Deployments 1-10),
and `HANDOFF.md` calls the instances "validation artifacts, not
infrastructure". So those fixtures replay *recorded chain outcomes* — the
amounts and rejections HANDOFF.md states were read back off the ledger — not
chain bytes. Starting with Deployment 13 (Issue #286), scripted end-to-end runs
(`scripts/e2e.sh`) record full transaction hashes and return values into JSON transcripts
(`transcripts/e2e-testnet-latest.json`), and the corresponding fixtures
(`deployment-13-real-hashes-*.snap`) cite these verified real hashes in their
provenance. Each fixture must say which deployment it came from and how
faithful it is in its `provenance =` line; the loader rejects a fixture with no
provenance, and `replay_all_recorded_snapshots` requires the string `HANDOFF.md`
to appear in it. If real XDRs are ever captured, `Step`/`Snapshot` in
`snapshot_replay.rs` are the seam to swap: the expectations stay, the transport
changes.

## Format

Line-oriented `key = value`, one per line. Blank lines and `#` comments are
ignored; a line indented with a space or tab continues the previous
`provenance =` value (provenance notes are long). Whitespace around keys,
values and tokens is insignificant. A parse failure panics with
`file:line` — a malformed transcript fails the job rather than being skipped.

### Metadata and terms

| Key | Meaning |
| --- | --- |
| `snapshot` | name; must equal the file stem (`foo.snap` → `snapshot = foo`) |
| `provenance` | required; which deployment this came from and what was read off the ledger |
| `note` | free-form commentary for reviewers; not parsed |
| `total_amount` | commitment size in the token's smallest unit (default 1000000) |
| `advance1_bps`, `advance2_bps` | tranche splits in basis points (default 1500 / 1500) |
| `claim_window_secs` | per-tranche claim window (default 604800) |
| `remainder_window_secs`, `delivery_window_secs` | two-phase funding / delivery windows |
| `contracted_quantity` | quantity the cooperative must deliver (default 1000) |
| `grade_price_bps` | comma-separated grade schedule, e.g. `10000,9000,7500` |
| `buyer_mint` | balance given to the buyer before `lock` (default 3000000) |
| `oracle` | `symbol,denominated_amount,max_age_secs,price,decimals`; registers a fixed-quote fixture oracle and passes an `OracleConfig`. The `price` is the raw 14-decimal value; for Deployment 9 it is the live GBP/USD quote HANDOFF.md recorded. |

### Steps

```text
step = <call> [args...] => ok
step = <call> [args...] => err(<ErrorVariant>)
step = advance <secs>
step = set_time <absolute_timestamp>
```

`advance`/`set_time` move the simulated ledger clock; they are engine actions,
not contract calls, so they carry no expectation (every other step must state
one). `=> ok` means "the call must succeed"; `=> err(Name)` means "the contract
must return exactly that `Error` variant" — a host-level abort or a different
variant is a failure. Steps run in file order against one freshly built
instance, so a fixture can never depend on another fixture's state.

Call names are the public method names, with these extra argument conventions:

- party names (`buyer`, `cooperative`, `warehouse`, `stranger`) stand in for
  addresses — `step = flag_dispute cooperative => ok`
- `advance`/`set_time` take seconds / a timestamp
- `set_allocation` takes one or more `<64 hex chars>:<share_bps>` members

`check` steps are replayed in the same order as the calls around them, because
a recorded balance belongs to a point in the transcript, not to its end:

```text
check = <key> <value>
```

Keys: `status`, `dispute_pre_status` (Status names), `buyer_party`,
`coop_balance`, `buyer_balance`, `contract_balance`, `stranger_balance`,
`settlement_bps`, `fx_adjusted_total`, `fx_shortfall_amount`,
`delivered_quantity`, `allocation_count` (integers), `fx_resolved`,
`fx_shortfall_funded`, `remainder_funded`, `advance1_claimed`,
`advance1_expired`, `advance2_claimed`, `advance2_expired` (`true`/`false`).
Adding a method or a field means adding one match arm in `run_step` or
`check_value`; the engine panics with the fixture's `file:line` when it meets a
name it does not know, so nothing is silently ignored.

### Declared divergences

Where the current build does *not* reproduce a recorded outcome, the step gets
an annotation naming what today actually does:

```text
known_divergence = settle => err(InvalidState)
step = settle => err(TrancheUnresolved)
```

The annotation must immediately precede a step that repeats the same call and
arguments, and the replay asserts **both** directions:

1. today's build must still produce the annotated outcome — if the behavior
   shifts again, the job fails and says so;
2. today's build must still *differ* from the recorded outcome — if the
   recorded behavior is restored, the job fails and says to delete the
   annotation.

So a divergence is a live, documented finding rather than either a red build or
a silence. A step with no annotation that does not match its expectation is an
undeclared regression and fails immediately. `deployment-2-…snap` currently
carries the only three: the pre-two-phase-funding WASM let `confirm_delivery`
run from `Advance2Released`, let an out-of-order `settle` reach the tranche
gate (error #12 `TrancheUnresolved`) instead of the status gate
(`InvalidState`), and settled a fully escrowed commitment.

## Adding a snapshot

1. Copy the closest existing file; keep the `#` header and set `snapshot` to the
   new file stem.
2. Transcribe the deployment from `HANDOFF.md` into `provenance` (what ran,
   which amounts were read back, and explicitly that no XDR exists).
3. Write the calls in the order the deployment made them, then the `check`
   lines for the recorded numbers at the point those numbers were recorded.
4. Run `cargo test --test snapshot_replay`. Green means the current build
   reproduces the deployment. A failure means it does not: either the fixture
   misread HANDOFF.md, or you have found a real regression — and if it is the
   latter, annotate it with `known_divergence`, list it in
   `the_recorded_divergences_are_still_divergences`, and say what you found in
   the PR.
