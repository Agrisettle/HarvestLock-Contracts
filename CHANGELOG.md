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
needed (`docs/storage-ttl.md`; #98). No deployment entry until the next
build goes to testnet.

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
