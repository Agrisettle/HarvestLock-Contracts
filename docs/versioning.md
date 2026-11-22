# Contract versioning — a semver scheme for the escrow WASM, applied to the deployment history

`harvestlock-escrow` has shipped ten testnet deployments and, as of this
writing, still has no version numbers on them — [CHANGELOG.md](../CHANGELOG.md)
says it plainly: "This contract has no versioned releases yet … so the
deployments are the versions." That is workable up to ten and stops being
workable around twenty. This doc defines a real versioning scheme for the
contract build and applies it, retroactively, to Deployments 1–10.

Ground truth for the numbers below is the WASM hashes, byte sizes, exported-
function counts and dates in [CHANGELOG.md](../CHANGELOG.md) (the index), cross-
checked against the transaction-level record in [HANDOFF.md](../HANDOFF.md)
(the record). Where this doc and those two disagree, HANDOFF wins on *what a
deployment did* and the on-chain WASM hash wins on *what a build is*. This file
only adds the version *labels*; it changes no behavior.

## TL;DR

- A version is an ordered human label pinned to one **immutable WASM hash**. The
  hash is canonical; the version is a name for it. A version is minted only when
  a build is uploaded to testnet — a source change that never shipped stays
  `[Unreleased]`.
- Format is [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html):
  `MAJOR.MINOR.PATCH`. Because the contract is pre-pilot (nothing on mainnet),
  **`MAJOR` is pinned at `0`** until the v1 feature set is frozen and cut; the
  first mainnet build is `1.0.0`. Per semver §4, while the major version is `0`
  the public API is considered unstable and **backwards-incompatible changes are
  a `MINOR` bump, marked `**BREAKING**`** rather than a `MAJOR` bump.
- Field meanings for a contract (see §2 for the decision procedure):
  - **MINOR (`y`)** — a new deployed build that changes the exported ABI or the
    state machine / settlement semantics. Covers both purely-additive
    deployments (a new function) and, flagged `**BREAKING**`, the incompatible
    ones.
  - **PATCH (`z`)** — a new WASM hash whose **exported ABI and state-machine
    semantics are identical** to the previous version: a docs/test/toolchain/CI
    rebuild, an internal refactor, or a correctness fix that restores documented
    behavior without touching the ABI. "Same interface, different bytes."
  - **MAJOR (`X`)** — reserved: stays `0` throughout the pilot; becomes `1` at
    the first mainnet / pilot-complete cut.
- Tags are namespaced **`escrow-vX.Y.Z`**. These numbers are *the contract's* and
  have nothing to do with the `api/` or frontend repos' own release tags — see
  §5.

## 1. What a version labels

Every testnet deployment produced a distinct WASM hash. Uploading a build is
the moment a version exists:

| artifact | role |
| --- | --- |
| **WASM hash** | canonical, immutable identity of a build; the thing the `api/` repo's `ESCROW_WASM_HASH` points at. Never changes for a given build. |
| **version (`X.Y.Z`)** | ordered label for that hash, minted at upload. Encodes *how this build relates to the previous one* (additive / breaking / ABI-identical). |
| **git tag `escrow-vX.Y.Z`** | applied to the source commit that produced the hash, so `source ↔ version ↔ hash` are all recoverable. |

There is no in-place "upgrade" of a live commitment in this model: the `api/`
repo deploys a fresh instance per deal from whatever `ESCROW_WASM_HASH` is set
(see HANDOFF Deployment 4's process note). So "compatibility" here is about
**callers and the ABI** — does a client written against the previous build have
to change — not about migrating live instances.

Two companion documents carry the consequences of that immutability, and both
key off the WASM hash rather than the version label:

- [commitment-schema-history.md](./commitment-schema-history.md) — the exact
  `Commitment`/`OracleConfig`/`AllocationMember`/`Status`/`Error` shape each
  deployment below produced, its ScMap key order, and which changes break a
  decoder built against the latest bindings. Use it to read an old instance.
- [migration-runbook.md](./migration-runbook.md) — what a release does to
  in-flight commitments, the per-status cost of unwinding one with `cancel`,
  and the exact signing sequence for replacing it.

## 2. Bump rules (decision procedure for the next upload)

Take the diff between the build you're about to upload and the last uploaded
build, then walk these in order:

1. **Is the exported ABI unchanged *and* is every state transition and
   `settle`/claim outcome byte-for-byte equivalent in behavior to the last
   build** (only hashes differ: rebuilt from docs/test-only/CI-guard/toolchain
   changes, an internal refactor, or a bug fix that merely restores already-
   documented behavior)?
   → **PATCH** (`z += 1`). New hash, same interface, no caller changes.
2. **Otherwise** — the ABI or the observable state machine changed (a function
   added/removed/renamed, a parameter list or return shape changed, a new or
   altered status transition, a change to what `settle`/`claim`/`lock` pay or
   gate on):
   - If the change is **purely additive** — existing callers and existing
     transitions behave exactly as before, a capability is only *added* (e.g. a
     brand-new function no current caller invokes) — → **MINOR** (`y += 1`,
     `z = 0`).
   - If the change is **backwards-incompatible** — a caller written against the
     previous build would break, or a value/param that used to be absent is now
     required, or a payout/transition meaning moved — → **MINOR** (`y += 1`,
     `z = 0`) marked **`**BREAKING**`**. Under semver this would be a MAJOR bump
     once `X ≥ 1`; during the pilot `X` stays `0` and the `**BREAKING**` flag is
     how the incompatibility is recorded.
3. Reaching a **MAJOR** (`0 → 1`) happens once, at the first mainnet /
   pilot-complete release. Until then no build is `1.x`.

Two corollaries:

- A `**BREAKING**` label is *descriptive, not optional*. Any build whose CHANGELOG
  entry has a `### Changed` section that a client must act on gets `**BREAKING**`,
  even pre-1.0.
- "No new WASM" means **no new version**. Post-Deployment-10 work (the #138–#141
  auth fix, the guard scripts) is `[Unreleased]` and only becomes `0.9.1` (see §3)
  the day it is uploaded.

### 2.1 The read view's own `schema_version` (#256)

§2 versions the *WASM*. One thing inside it has a second, faster-moving
version: the client-facing commitment shape. `get_commitment` returns the
storage struct `Commitment` itself, so adding a field for a storage reason is
silently an ABI change for every indexer. `get_commitment_view` returns
`CommitmentView` instead, with:

- an explicit `schema_version` (currently `1`, mirroring
  `COMMITMENT_VIEW_SCHEMA_VERSION` in `lib.rs`);
- semantic sub-structs — `parties`, `terms`, `tranches`, `funding`,
  `delivery`, `fx`, `dispute` — instead of storage order;
- a documented stability promise: fields are only added, removed, renamed, or
  re-meaninged with a `schema_version` bump. **Storage-only** changes to
  `Commitment` never bump it. That is the whole point of the separate view.

`get_commitment` stays for backward compatibility and remains the canonical
on-chain record. New clients should read the view and reject a
`schema_version` they don't understand.

The view is a projection of the same single instance-storage read, not a
second source of truth. Measured on the test host, `get_commitment` costs
218,476 CPU instructions and `get_commitment_view` 223,471 (≈2.3% more,
dominated by the shared storage read); the measurement is pinned by
`get_commitment_view_costs_the_same_order_as_the_storage_struct`, which
fails if the view ever starts re-reading storage. Host metering undercounts
the WASM path, so treat those numbers as an order-of-magnitude check rather
than a network fee estimate.

## 3. Retroactive mapping of Deployments 1–10

Applying §2 to the recorded history. The "class" column is the input to the
bump rules; the version column is the output. WASM short-hashes are from
CHANGELOG; full hashes and per-deployment detail are in HANDOFF.

| Deployment | Date (testnet) | WASM | Exported fns | Class | Version |
| --- | --- | --- | --- | --- | --- |
| 1 | 2026-08-27 | *(none recorded)* | — | single-shot-funding happy path; superseded by D2's incompatible tranche model | `0.0.1` *(superseded)* |
| 2 | 2026-08-27 | `27b867fb…` | 13 | **BREAKING** — replaces single-shot funding with claimable-balance tranches; state machine differs from D1 | `0.1.0` |
| 3 | 2026-09-01 | `c6b73c5c…` | 14 | additive — new `cancel` | `0.2.0` |
| 4 | 2026-09-02 | `667e4a50…` | 15 | additive — new `reassign_buyer` | `0.3.0` |
| 5 | 2026-09-02 | `3351e0de…` | 19 | **BREAKING** — `lock` escrows only the deposit; `confirm_delivery` re-gated; adds the four remainder functions | `0.4.0` |
| 6 | 2026-09-03 | `f935d469…` | 19 | **BREAKING** — `initialize`/`confirm_delivery` signatures change; `settle`'s payout math changes (grade/shortfall schedule) | `0.5.0` |
| 7 | 2026-09-04 | `9f1843c9…` | 21 | additive — record-only allocation ledger (`set_allocation`/`get_allocation`), `settle` unchanged | `0.6.0` |
| 8 | 2026-09-04 | `5322b601…` | 23 | **BREAKING** — `initialize` gains a new `oracle_config` argument every caller must now pass (its value may be `None`); additive oracle read (`oracle_rate`/`get_oracle_config`) | `0.7.0` |
| 9 | 2026-09-06 | `7940147d…` | 26 | **BREAKING** — FX shortfall wired into `settle`'s gating; `OracleConfig` gains `denominated_amount` | `0.8.0` |
| 10 | 2026-09-06 | `21a4bb2e…` | 29 | additive — dispute path (`flag_dispute`/`resolve_dispute`/`expire_dispute_window`), new `Disputed` status | `0.9.0` |
| *(unreleased)* | — | *(not uploaded)* | — | ABI-identical: #138–#141 overlapping-role auth correctness fix + two CI guard scripts | next upload ⇒ **`0.9.1`** (patch) |

Per-deployment struct shapes and decode compatibility for every hash in that
table are in [commitment-schema-history.md](./commitment-schema-history.md); the
operational meaning of a bump for deals that are already open is in
[migration-runbook.md](./migration-runbook.md).

Notes on the two judgement calls the history forces:

- **Deployment 1 → `0.0.1`, marked superseded.** CHANGELOG already says D1
  predates the tranche model and must not be referenced. Under §2 rule 2 it and
  D2 are the same incompatible boundary, so D2 takes `0.1.0` and D1 keeps a
  pre-line `0.0.1` purely so the ten real deployments all have a label.
- **The pending `0.9.1`.** The #138–#141 fix narrows which principals may co-sign
  `cancel`/`reassign_buyer`/`resolve_dispute` — it changes accepted *behavior*,
  not the exported ABI or any status transition a well-behaved client depended
  on. Per §2 rule 1 (a fix restoring documented intent, same interface) that is a
  **PATCH**, and it becomes `0.9.1` the moment a build carrying it is uploaded —
  not before, per §2's "no new WASM means no new version."

## 4. Pre-release and build-metadata suffixes

Standard semver extensions, defined so pilot milestones are addressable without
burning a MINOR early:

- `escrow-v0.10.0-rc.1` — a release candidate uploaded to testnet ahead of the
  `0.10.0` cut. RCs share the eventual version's `X.Y.Z` with a `-rc.N` suffix;
  the first non-RC upload retires the suffix.
- `escrow-v0.10.0+<12-hex>` — optional `+build` metadata carrying the leading 12
  hex of the WASM hash, for unambiguous machine reads. Per semver this metadata
  is ignored in version *precedence*, which is correct: the hash is already
  recorded in CHANGELOG and is the real identity.

## 5. Distinct from the application repos' release tags

The `api/` repo and the frontend each have their own release tags with their own
numbers. **An `escrow-vX.Y.Z` tag is neither of those and the three never share
a number line.** Concretely:

- `escrow-v…` versions track the **WASM build only**, and change when the
  contract's ABI/state machine does.
- An `api/` deployment that merely repoints `ESCROW_WASM_HASH` at an existing
  `escrow-vX.Y.Z` is an **`api/` release**, not a new contract version — the
  contract didn't change, only which hash the app pins.
- Conversely a new `escrow-vX.Y.Z` may require *no* `api/` tag change if the ABI
  is backward-compatible.

The `escrow-` prefix on the git tag is what keeps `git tag -l` in the contract
repo from ever being mistaken for an application release, and is the string
integrators match on when subscribing to contract-build releases.

### Relationship to the Cargo package version

`contracts/escrow/Cargo.toml` carries a crate `version` (currently `0.1.0`).
That field feeds `cargo`, and rustdoc output like that #90 proposes renders it in
page titles — so it must not silently disagree with the deployment line. It
already lines up by coincidence: the committed `0.1.0` is exactly this scheme's
Deployment 2, the first reference build. Keep it that way — treat the Cargo
package `version` as the *machine-readable* rendering of the current deployment
version and bump it in step with §2 (a MINOR or PATCH deployment bumps the Cargo
`version` the same way before the build is uploaded). The Cargo `version` has no
semantics of its own beyond mirroring the deployment line; if they ever diverge,
the deployment line (§2, tied to the WASM hash) is authoritative and the Cargo
field is the thing to correct.

## 6. Where this is enforced going forward

This doc defines the scheme; it does not add CI to enforce it (repo policy:
contributors don't add or reconfigure CI checks — see
[PULL_REQUEST_TEMPLATE.md](../.github/PULL_REQUEST_TEMPLATE.md)). The practical
tie-backs, each owned by a human step at upload time:

1. Mint the version per §2 as part of the `stellar contract upload` step.
2. Add the version to the CHANGELOG entry's heading (e.g.
   `## [0.9.0] — Deployment 10 — 2026-09-06`) alongside the WASM short-hash it
   already records.
3. `git tag escrow-vX.Y.Z` the source commit and note the tag in HANDOFF's
   per-deployment paragraph.

CHANGELOG.md remains the index of hashes; this file is the index of *versions*.
The version table in §3 and the CHANGELOG headings are kept in sync by hand, the
same way CHANGELOG and HANDOFF already are.
