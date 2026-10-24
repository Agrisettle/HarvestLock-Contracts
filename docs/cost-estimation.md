# Cost estimation — `stellar-cli` built-ins vs. the external `soroban-cost-estimator`

Issue [#92] asked whether `stellar-cli` now reports simulation resource costs
natively enough to make the external
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
tool redundant for this project, and for a recommendation on which to
standardize on. Short answer: **the CLI already produces the numbers; the
external tool mostly re-formats them and adds a fee/trending layer. Standardize
on the CLI as the source of record and treat the estimator as an optional
reporting add-on, not a dependency.**

## TL;DR

- Both tools get their raw metrics from the *same* place: the RPC
  `simulateTransaction` response (CPU instructions, memory bytes, ledger
  read/write footprint, transaction size). Neither measures anything the network
  doesn't already return from a simulation.
- `stellar-cli` is already this repo's toolchain and CI dependency (see
  [`.github/workflows/test.yml`](../.github/workflows/test.yml) and HANDOFF's
  toolchain section). It exposes the cost of the exact build you ship, with no
  extra install.
- `soroban-cost-estimator`'s value is **on top of** those metrics, not in
  measuring them: fee-in-stroops/XLM conversion, contextual optimization advice,
  a `--json` pipeline mode, and persisted `config snapshot` baselines for
  cost-over-time regression trending.
- Recommendation: CLI for the measurement/source-of-truth; the estimator only
  if/when the `api/` side needs XLM-denominated budgets or turnkey historical
  cost snapshots. Do not add it as a required CI dependency (§4).

## 1. What the CLI gives you natively (verified)

Against the `stellar-cli` available in this environment:

- `stellar contract invoke … --cost` — "Output the cost execution to stderr"
  (help text). Simulates the call and prints the resource cost alongside the
  result.
- `stellar contract invoke … --send=no` (the `--sim-only` path) — simulates and
  returns the result without submitting.
- `stellar tx simulate` — simulates a transaction envelope read from stdin and
  returns the full `simulateTransaction` response, i.e. cost + footprint +
  diagnostics, for an arbitrary prebuilt transaction (not only contract calls).
- `--filter-logs 'stellar_cli::log::footprint=debug'` (or `RUST_LOG`) — emits the
  per-call ledger-footprint detail the CLI uses internally.

There is **no `stellar contract simulate` subcommand** in this version — cost
reporting lives on `invoke --cost` and `tx simulate`, both of which drive an RPC
simulation and therefore need a reachable network and (for `invoke`) a deployed
`--id` and a source account.

## 2. What the external tool adds

Per [`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)'s
own README:

- Tracks the same "real resource consumption (CPU instructions, memory,
  read/write entries/bytes, tx size, rent)" and **translates them into a
  transaction fee in stroops/XLM.**
- Runs as a standalone CLI that "require[s] no test harness and no local
  container," routing simulations to live `simulateTransaction` RPC on Testnet or
  Mainnet — the same RPC the CLI's `--cost`/`tx simulate` use.
- Renders a table **plus contextual optimization advice**, with a `--json` switch
  for pipelines.
- Persists `config snapshot` files for **historical cost baselining** — the one
  capability the CLI has no direct equivalent for.
- Inputs: a compiled `--wasm` path, or `--id`/`--fn`/`--arg` for a deployed
  contract.

Note the tool frames its differentiation against "reporting scripts that depend
on local Docker or instrumented JS/TS test suites" — not against `stellar-cli`'s
`--cost`. For a project that already uses `stellar-cli` (this one), the overlap is
large and the non-overlap is narrow: fee conversion, advice text, and snapshot
trending.

## 3. Comparison

| dimension | `stellar-cli` (`invoke --cost` / `tx simulate`) | `soroban-cost-estimator` |
| --- | --- | --- |
| Raw metrics (CPU, mem, footprint, tx size) | ✅ direct from `simulateTransaction` | ✅ same RPC response |
| Rent | ✅ (part of footprint/response) | ✅ |
| Fee in stroops / XLM | ⚠️ you compute from response + network fee schema | ✅ built in |
| Optimization advice | ❌ | ✅ |
| Machine-readable (`--json`) | ⚠️ `tx simulate` returns structured XDR/JSON; `--cost` is stderr prose | ✅ `--json` |
| Historical cost baselining | ❌ (DIY: diff committed outputs) | ✅ `config snapshot` |
| Extra dependency / supply chain | none (already in toolchain + CI) | adds one, plus the same live-RPC dependency |
| Measures the exact shipped build | ✅ it *is* the deploy/invoke tool | ✅ (given the same wasm/instance) |

The honest reading: for **measurement**, the two are interchangeable — both read
one RPC simulation response, and the CLI is already present. The estimator earns
its place only on the **presentation and trending** axes (fee conversion, advice,
snapshots).

## 4. Recommendation

1. **Source of record: `stellar-cli`.** Use `stellar contract invoke … --cost`
   for per-function dispatch cost during development, and `stellar tx simulate`
   when you need the footprint/cost of a fully-built transaction (multi-op,
   auth-heavy) rather than a single function. This is free — it's the tool the
   build and CI already run.
2. **Keep offline regression coverage in `cargo test`.** The committed
   `contracts/escrow/test_snapshots/**` already capture each call's ledger
   read/write footprint deterministically against the in-process test ledger,
   with no network, no funded account and no deployed instance. That is the
   right place for cheap, reproducible footprint regression; neither external
   tool nor CLI `--cost` can match its hermeticity because both require live RPC.
3. **Adopt the estimator opportunistically, not as a dependency.** Reach for it
   when you specifically want (a) an XLM/stroops fee figure to hand the `api/`
   team for pre-transaction `resourceConfig`/fee bidding, or (b) `config snapshot`
   trending across deployments. If we ever want that in CI, prefer parsing the
   CLI's `tx simulate` JSON response over adding the estimator as a new required
   tool — it avoids a fresh supply-chain dependency for what is largely the same
   RPC payload.

   **Update (20 Oct 2026), after using it for real:** a second, concrete
   reason not to depend on it. The estimator's `--arg` infers an argument's
   type from the literal alone (`ScVal::I64` for any integer), ignoring the
   contract spec it has already parsed — so v0.1.0 **cannot invoke any
   contract with a `u32`/`i32`/`u64` parameter at all**, and upstream `main`
   has the same defect. `confirm_delivery` takes two `u32`s, so the stock
   binary aborts with `Error(WasmVm, InvalidAction)` before the contract
   body runs. The measurement in [`gas-costs.md`](./gas-costs.md) therefore
   needed a small source patch to the tool, which is a poor thing to have to
   repeat for every future contract we measure. The CLI's `--cost` has no
   such limitation. See
   [`gas-costs.md`'s tooling finding](./gas-costs.md#tooling-finding--soroban-cost-estimator-v010-cannot-encode-a-u32-argument).

Net: the CLI makes the external tool **partially** redundant (its measurement
role), not wholly. Its fee-conversion and snapshot-trending layers remain
genuinely additive and worth keeping available for the budgeting/trending use
cases.

## 5. Caveats on this comparison

- **CLI version.** Verified against the `stellar-cli` present in this
  environment (**22.0.0**). [HANDOFF.md](../HANDOFF.md)'s toolchain section
  records the project having used **v28.0.0**. `invoke --cost` and `tx simulate`
  exist in both; re-confirm their exact output shape against the version your CI
  pins before finalizing anything that scripts on them.
- **No live cost run is quoted here.** Producing actual numbers needs a funded
  source account and a deployed instance on a reachable network; none was run
  from this environment. The capability claims come from `--help` output (CLI)
  and the tool's README (estimator), not from fresh executions.

[#92]: https://github.com/Agrisettle/HarvestLock-Contracts/issues/92
