# Changelog

Notable changes to `harvestlock-escrow`, one entry per testnet deployment.

This contract has no versioned releases yet — it's pre-pilot, and nothing is
on mainnet — so **the deployments are the versions**. Each entry is the WASM
build uploaded and exercised on testnet on that date. Source lives in
[`contracts/escrow`](./contracts/escrow); the transaction-level record of each
walk-through is in [`HANDOFF.md`](./HANDOFF.md), which this file links to
rather than repeating. `HANDOFF.md` remains the source of truth when the two
disagree.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Dates here are the ledger's, not the write-up's: each is the day that
deployment's instance was created on Stellar testnet, read back from
stellar.expert, with the instance's WASM hash cross-checked against the hash
HANDOFF.md records for that deployment. Every contract address below is a
testnet throwaway — a validation artifact, not infrastructure. Don't build
anything that depends on one still existing.

## [Unreleased]

Changes since Deployment 10, none of which uploaded a new WASM: auth-model
scrutiny of the staged multi-party flow (#138–#141, which surfaced and fixed a
real overlapping-role bug in `cancel`/`reassign_buyer`/`resolve_dispute`),
edge-case coverage (#130–#133), exact-boundary coverage at the deadline second
itself for all four absolute deadlines (`remainder_deadline`,
`delivery_deadline`, `fx_shortfall_deadline`, `dispute_deadline`; #112–#115,
with the inclusive/exclusive rule now stated in the doc comments rather than
left implicit), the `Commitment`-footprint audit (#143), dispatch-cost
benchmarks, two CI guard scripts (Error-discriminant stability,
public-method auth doc completeness), and the #95–#98 review set: the
`confirm_delivery` bounds-check ordering recorded as analyzed (no gap —
check runs before any mutation; #95), the abandoned-at-`Draft` funds
answer documented (nothing ever moved at that stage; #96), a runnable
minimal-commitment walkthrough in `docs/minimal-example.md` (#97), and
a storage-TTL audit with a real fix — `bump_instance_ttl` re-rents the
contract instance (and code) entry from `load`/`initialize` when its
lifetime has decayed below half the network max, closing the gap where
a dormant `Locked` escrow's own state could be archived while still
needed (`docs/storage-ttl.md`; #98).

Security review of the permissionless tranche-opening calls, on the
record (#2): `docs/permissionless-release-review.md` answers the open
"is this actually safe?" question with a full reader audit of
`advance1_deadline`/`advance2_deadline` and the adversarial-timing
scenarios the issue named, pinned by a dedicated regression suite
(`contracts/escrow/tests/permissionless_release_review.rs`, its own CI
job). Verdict: the permissionless shape holds — recorded confirmation,
no auth gate added; the `release_advance_1/2` doc comments now state the
conclusion instead of only the defensive one-liner.

A distinct "waiting on a fresh quote" signal for stale-oracle
settlement blocks (#1): new permissionless `refresh_fx_quote_status`
records a stale oracle quote into the new `Commitment::fx_quote_stale`
flag, which `get_commitment` surfaces, so callers can see settlement is
waiting on a fresh Reflector quote instead of inferring it from a
failed `resolve_fx_shortfall`. Deliberately a boolean flag alongside
`fx_resolved`, not a new `Status` variant; informational only — no
guard reads it (`resolve_fx_shortfall` still enforces staleness
itself, and its `OracleStale` refusal still can't persist a marker of
its own, which is exactly why the probe is a separate call). No
deployment entry until the next build goes to testnet.

## [Deployment 11] — 2026-10-20

### Added

- A measured resource-cost baseline for the delivery group. `mark_checkpoint`
  and `confirm_delivery` had only ever been correctness-tested; this
  deployment measured them against a live testnet instance with
  [`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
  and recorded the numbers in [`docs/gas-costs.md`](./docs/gas-costs.md) —
  CPU instructions, memory, read/write entries and bytes, tx size, and the
  fee split into its non-refundable and refundable halves.

### Measured

- `mark_checkpoint` — 1,523,347 CPU instructions, 2 read / 2 write entries,
  4,579 non-refundable stroops. `confirm_delivery` — 1,530,217 CPU
  instructions, 2 read / 2 write entries, 4,584 non-refundable stroops.
  Both were also really submitted, and were charged 4,181 and 4,242 stroops
  in total: 99.5% of the estimator's headline "total" is simulated storage
  rent, not execution.
- The auth-gated calls are the expensive ones, not the money-moving ones:
  both of these move zero stroops and still cost ~53% more CPU than the
  permissionless `release_advance_1` measured on the same build and the same
  instance — the extra read entry, write entry and ~527k CPU instructions
  appear only on the auth-gated path.
- A real upstream limitation, recorded in full in `docs/gas-costs.md`: the
  estimator's `--arg` infers an argument's type from the literal alone, so
  v0.1.0 cannot invoke any contract with a `u32`/`i32`/`u64` parameter at
  all — including this contract's `confirm_delivery`. Upstream `main` has
  the same defect.

No contract code changed. WASM `e097f1ff…` · 60,043 bytes · 29 exported
functions · instances `CCTQTECYATFFE3GJWYSND6CJOGB6KLGSF3JKK5KW3HRBNTKLFBFJEUWJ`
(walk-through), `CDG2V5HIJ5KJS6D3QK4VI4NRCRVUZYHHZQWYSONULUKQYJMOPFDBWAQ7`
and `CBTFFKL24HYNUUG4K26FBB3B47T5KROWR3FTMBH4SST5NA3NCT5IFATV` (controls),
`CDKP2KUIDUJ4MS3G4GDS372BK3HMRZZEMJGM4C5GULTGAISI2FFGZAXK` (independent
reproduction) · [walk-through][handoff-deployments]

## [Deployment 10] — 2026-09-06

### Added

- Dispute flagging with defined escalation (PRD's must-have): `flag_dispute`
  lets any one named party freeze a commitment from `Locked` through
  `Delivered`; `resolve_dispute` (unanimous three-party auth) or
  `expire_dispute_window` (permissionless, once `dispute_deadline` passes)
  restores the exact pre-dispute status. A bounded pause, not arbitration of
  who was right — by explicit design.

WASM `21a4bb2e…` · 47,171 bytes optimized · 29 exported functions · instance
`CCHQLA3LZXWXWJJR75TWFM4POBPA673VIWJX4YWSAHUZEWUSXSF6HJBO` ·
[walk-through][handoff-deployments]

## [Deployment 9] — 2026-09-06

### Added

- `resolve_fx_shortfall` / `fund_fx_shortfall` / `expire_fx_shortfall_window`:
  PRD §4.2 option (b) — buyer tops up or is refunded at settlement — actually
  wired into `settle`'s payout math, using a fresh oracle rate.

### Changed

- `OracleConfig` gained `denominated_amount`, the deal's true value in
  `price_asset`.

WASM `7940147d…` · 42,175 bytes optimized · 26 exported functions · instance
`CBOZLCNHWFHTK24WO7RCDBEWUFV6LWQKZFHJUIOZZRVBMNRTGLXRMKKH` ·
[walk-through][handoff-deployments]

## [Deployment 8] — 2026-09-04

### Added

- `oracle_rate` and `get_oracle_config`: a staleness-bounded FX read against a
  live Reflector SEP-40 oracle (PRD §16.3), with a stale quote a hard error
  rather than a silently-old number. Read primitive only — `settle`'s math is
  unchanged at this point.

### Changed

- `initialize` gained an optional `oracle_config` (oracle address, price
  asset, max quote age). Reflector's live testnet fiat oracle does not quote
  NGN — confirmed by calling it, not assumed — so the live walk used GBP.

WASM `5322b601…` · 33,488 bytes optimized · 23 exported functions · instance
`CCUDE3MMW5JGK4QM3CA5PGW23KCYEIXFJ46754LKIWNJR4OKACU6HMA4` ·
[walk-through][handoff-deployments]

## [Deployment 7] — 2026-09-04

### Added

- `set_allocation` / `get_allocation`: the PRD §4.8/§16.1 allocation ledger.
  Each member farmer's entitlement is recorded as a per-member salted hash plus
  a `share_bps`, cooperative-gated, one-time, callable only from `Draft`.
  Record-only in v1 — `settle` still pays the cooperative wallet as a lump sum.

WASM `9f1843c9…` · 28,767 bytes optimized · 21 exported functions · instance
`CBRZPZPXBYW2QHGU5KOFWG27AS4YM7EQLULOALYAE3VGJ7BMZTZLKARZ` ·
[walk-through][handoff-deployments]

## [Deployment 6] — 2026-09-03

### Added

- The PRD §7 shortfall/grade adjustment schedule: `initialize` takes
  `contracted_quantity` and a pre-agreed `grade_price_bps` table;
  `confirm_delivery` takes `delivered_quantity`/`grade_index` and computes
  `settlement_bps`.

### Changed

- `settle` pays the cooperative only what's still owed against
  `settlement_bps` and refunds the rest of the remainder to the buyer as a
  shortfall credit. Already-claimed advances are never clawed back. A real
  behavior change from Deployment 5's "sweep the whole balance to the
  cooperative", not an additive feature.

WASM `f935d469…` · 25,370 bytes optimized · 19 exported functions · instance
`CBDDWE4CKHAAFZPNNUVZRCHKAJQ3F2PMQA547LMPPM6ULRZJ6AKAHXAQ` ·
[walk-through][handoff-deployments]

## [Deployment 5] — 2026-09-02

### Added

- Two-phase funding and both uncontested terminal outcomes: `ready_for_delivery`
  and `fund_remainder` escrow the remainder later; `expire_remainder_window`
  (permissionless, buyer default) and `reclaim_on_nondelivery` (buyer-gated,
  seller non-delivery) close the deal out when the other side doesn't show.
  Three separate instances proved the happy path, the buyer-default path, and
  the seller-non-delivery path.

### Changed

- `lock` now escrows only the deposit (`advance1_bps + advance2_bps`), not the
  whole `total_amount`.
- `cancel` and `reassign_buyer` extended to reach `ReadyForDelivery`;
  `confirm_delivery` re-gated on the remainder being funded.

WASM `3351e0de…` · 22,048 bytes optimized · 19 exported functions · instances
`CB5MR3IYQ6R4QTFMTXTBFG4WSCZTJ3QTND6GMBSASGORPQSD3VKYCTCW` (happy path),
`CA3AB4QXJ5ZZ4SIIH3VHI7C37MDWBUQSZECT64XBVHPDQI473BJK4G52` (buyer default),
`CCOVWDEWTQPI57OCUUHGSIFSMAUQWJOX5SVZI2J5KFYDRVGNDVTFXSNQ` (non-delivery) ·
[walk-through][handoff-deployments]

## [Deployment 4] — 2026-09-02

### Added

- `reassign_buyer`: buyer-position assignability (PRD §4.8), consented by the
  outgoing buyer, the cooperative, and the incoming buyer.

WASM `667e4a50…` · 16,130 bytes optimized · 15 exported functions · uploaded
only, no standalone instance recorded (the `api/` repo's tests deploy fresh
instances from the hash) · [walk-through][handoff-deployments]

## [Deployment 3] — 2026-09-01

### Added

- `cancel`: the PRD §7 mutual-consent unwind, buyer- and cooperative-co-signed,
  reachable from `Draft` through `Advance2Released`, refunding the contract's
  current balance to the buyer.

WASM `c6b73c5c…` · 14,825 bytes optimized · 14 exported functions · instance
`CAVWBS5USXL5WRIB2HBGKBEABQZ6AT6YH7P4JO452WWEMWPWER5T6RDF` ·
[walk-through][handoff-deployments]

## [Deployment 2] — 2026-08-27

### Added

- Claimable-balance-with-expiry semantics for both advance tranches, built
  natively in the contract rather than via classic Stellar
  `ClaimableBalanceEntry` interop: opening a tranche starts a claim window, the
  cooperative can claim within it, the buyer can reclaim once it passes. Both
  tranches must be resolved before `settle` will run.

WASM `27b867fb…` · 13,472 bytes optimized · 13 exported functions · instance
`CDVF6UVJOLF3OHCFSYSJ72RMG2T6DUQ42VRJ6IHL6MVEFDYEBZ3KTFK4` ·
[walk-through][handoff-deployments]

## [Deployment 1] — 2026-08-27

### Added

- The escrow happy path: the `Draft` → `Locked` → … → `Settled` state machine
  with a single-shot funding model.

Superseded — this build predates the claimable-balance tranche semantics that
replaced its funding model, so don't use it as a reference for current
behavior. Kept as an entry because it was a real deployment. Instance
`CDUWXPAC2AT353J4UF5WWJVW3ZUMATZH7PGNZ2AEXWOGT2USDRR77JSO` ·
[walk-through][handoff-deployments]

[handoff-deployments]: ./HANDOFF.md#verified-on-testnet--twice-now-and-the-second-time-specifically-exercised-the-new-mechanic
