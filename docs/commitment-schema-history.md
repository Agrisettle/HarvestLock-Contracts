# Commitment schema history — what `get_commitment` returned on every testnet WASM

The escrow contract has **no upgrade path** (threat-model entry 26: no admin,
no owner, no upgrade function). A contract instance keeps running the WASM it
was deployed with forever, so ten different builds are simultaneously live on
testnet, each with a different `Commitment` shape. This is the lookup table for
them: given a WASM hash (or an instance address someone tells you is old), what
does `get_commitment` return, and what does it mean.

Companion documents:

- [CHANGELOG.md](../CHANGELOG.md) — per-deployment dates, byte sizes,
  exported-function counts, and the recorded WASM hash (the index).
- [HANDOFF.md](../HANDOFF.md#verified-on-testnet--twice-now-and-the-second-time-specifically-exercised-the-new-mechanic)
  — the transaction-level record of each deployment (the walk-through).
- [versioning.md](./versioning.md) — the `0.x.y` label minted for each hash.
- [error_discriminants.snapshot](../contracts/escrow/error_discriminants.snapshot)
  — the canonical `Error` number → name mapping.

Ground truth here is `git log -p contracts/escrow/src/lib.rs`. Every shape below
was read out of the source at the commit that produced that deployment, not
reconstructed from prose:

| Deployment | Source commit | Date |
| --- | --- | --- |
| 1 | `1dc820d` | 2026-08-27 |
| 2 | `6683f84` | 2026-08-27 |
| 3 | `5cc000f` | 2026-09-01 |
| 4 | `ad24de8` | 2026-09-02 |
| 5 | `5af5e4d` | 2026-09-02 |
| 6 | `fe10829` | 2026-09-03 |
| 7 | `6f97a47` | 2026-09-04 |
| 8 | `d2a497b` | 2026-09-04 |
| 9 | `f024c73` | 2026-09-06 |
| 10 | `302dbf3` | 2026-09-06 |
| Unreleased | `HEAD` of `main` | — |

Deployment 11 (`e097f1ff…`, 2026-10-20, cost measurements only) uploaded a new
hash but changed no contract code — see §12. Its schema is Deployment 10's.

## 0. How these types encode on the wire

Four rules matter more than any individual table. They are read out of
`soroban-sdk-macros` 29.0.1 (`derive_struct.rs`, `derive_enum.rs`,
`derive_error_enum_int.rs`), not assumed:

1. **A `#[contracttype]` struct is an `ScMap` keyed by field-name symbols,
   sorted ascending by key.** The derive macro sorts fields by their identifier
   string *before* generating the map, and builds it with `ScMap::sorted_from`.
   Declaration order is irrelevant to the wire shape. That is why this doc gives
   every struct's key order alphabetically — it is the order a raw XDR reader
   sees, and a hand-built map in any other order is rejected by the host as
   malformed.
2. **On decode, a missing key becomes `ScVal::Void`, and a non-`Option` field's
   conversion from `Void` fails.** So decoding is not symmetric:
   - bindings at deployment *X* reading a **newer** instance → extra keys are
     ignored, the value decodes fine;
   - bindings at the **latest** build reading an **older** instance → every
     field that instance's struct doesn't have is missing, and decoding fails.
   This is the direction that breaks, and it is why the tables below list the
   fields a latest-bindings decoder still expects from each old instance.
3. **A unit-variant `#[contracttype]` enum encodes as
   `ScVal::Symbol(variant_name)` — not as a numeric discriminant.** `Status` is
   therefore name-encoded: adding or reordering variants never renumbers an
   existing value. A decoder that meets a variant name it doesn't know fails,
   but a variant it already knows always decodes to the same meaning.
4. **`#[contracterror]` errors encode as numeric `ScVal::U32`.** The numbering
   is append-only and CI-pinned (`.github/scripts/check_error_discriminants.sh`
   against `error_discriminants.snapshot`). An old client seeing a new code gets
   an unknown *number*, never a changed meaning for an old one.

All four `Option<T>` occurrences in these shapes are `initialize`/view
arguments (`oracle_config`), not `Commitment` fields; `Option` encodes as
`ScVal::Void` or the inner value.

### Type stability

Across Deployments 1–10 and Unreleased, **every one of the 33 `Commitment`
fields was added once and never renamed or retyped.** The same is true of
`AllocationMember` and of the fields `OracleConfig` has had since introduction.
The change history is purely additive at the field level, which means a client
that already handles a *newer* shape can also read an older instance as long as
it tolerates absent fields — and a client on the oldest shape can read the
newest instance, because unknown keys are ignored. Only the
`latest-bindings → older-instance` direction is systematically broken.

## 1. Deployment 1 — single-shot funding

**WASM:** none recorded (predates hash recording; instance
`CDUWXPAC2AT353J4UF5WWJVW3ZUMATZH7PGNZ2AEXWOGT2USDRR77JSO` ·
[CHANGELOG](../CHANGELOG.md#deployment-1--2026-08-27) · version `0.0.1`,
superseded).

**Commitment — 9 fields.** Declaration order:
`buyer, cooperative, warehouse_operator, token, total_amount, advance1_bps,
advance2_bps, status, created_at`.

ScMap key order:

```
advance1_bps, advance2_bps, buyer, cooperative, created_at, status, token,
total_amount, warehouse_operator
```

**OracleConfig:** does not exist. **AllocationMember:** does not exist.

**Status — 10 variants declared, 7 reachable.** Declared:
`Draft, Locked, Advance1Released, CheckpointPassed, Advance2Released,
Delivered, Settled, Cancelled, Defaulted, Disputed`. Reachable:
`Draft, Locked, Advance1Released, CheckpointPassed, Advance2Released,
Delivered, Settled` — the other three were declared ahead of the code and no
path produced them.

**Error — 5 variants:** `AlreadyInitialized=1`, `NotInitialized=2`,
`InvalidState=3`, `InvalidBps=4`, `ZeroAmount=5`.

**Functions added (9):** `initialize`, `lock`, `release_advance_1`,
`mark_checkpoint`, `release_advance_2`, `confirm_delivery`, `settle`,
`get_status`, `get_commitment`. `lock` escrowed the whole `total_amount`.

## 2. Deployment 2 — claimable-balance tranches

**WASM:** `27b867fb…` (13,472 bytes, 13 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-2--2026-08-27) · version `0.1.0`,
**BREAKING** (replaces single-shot funding).

**Commitment — 16 fields.** Added:

| field | type | set by |
| --- | --- | --- |
| `claim_window_secs` | `u64` | `initialize` |
| `advance1_deadline` | `u64` | `release_advance_1` (0 = not opened) |
| `advance1_claimed` | `bool` | `claim_advance_1` |
| `advance1_expired` | `bool` | `reclaim_advance_1` |
| `advance2_deadline` | `u64` | `release_advance_2` (0 = not opened) |
| `advance2_claimed` | `bool` | `claim_advance_2` |
| `advance2_expired` | `bool` | `reclaim_advance_2` |

ScMap key order:

```
advance1_bps, advance1_claimed, advance1_deadline, advance1_expired,
advance2_bps, advance2_claimed, advance2_deadline, advance2_expired, buyer,
claim_window_secs, cooperative, created_at, status, token, total_amount,
warehouse_operator
```

**OracleConfig / AllocationMember:** still absent.

**Status:** unchanged (same 10 declared, same 7 reachable).
**Error — 7 added:** `InvalidWindow=6`, `NotYetOpened=7`, `AlreadyClaimed=8`,
`AlreadyExpired=9`, `ClaimWindowPassed=10`, `ClaimWindowNotPassed=11`,
`TrancheUnresolved=12`.

**Functions added (4):** `claim_advance_1`, `reclaim_advance_1`,
`claim_advance_2`, `reclaim_advance_2`.

## 3. Deployment 3 — mutual cancellation

**WASM:** `c6b73c5c…` (`c6b73c5c483c5946be73fe68e0cb798c6cd39440ec1430e05cf1f048c1018327`,
14,825 bytes, 14 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-3--2026-09-01) · version `0.2.0`.

**Commitment:** unchanged from Deployment 2 — same 16 fields, same key order.

**OracleConfig / AllocationMember:** unchanged (absent).

**Status:** declared list unchanged; `Cancelled` becomes reachable (first
transition into it is `cancel`). Still symbol-encoded, so no value changes.
**Error:** unchanged (1–12).

**Functions added (1):** `cancel`.

## 4. Deployment 4 — buyer-position assignability

**WASM:** `667e4a50…` (`667e4a50c8ad9af081bf2c8a5be9d34069a45e5d582753cc665346455a1096b5`,
16,130 bytes, 15 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-4--2026-09-02) · version `0.3.0`.

**Commitment:** unchanged from Deployment 3 — same 16 fields, same key order.
`reassign_buyer` rewrites the existing `buyer` field in place; it adds no field.

**OracleConfig / AllocationMember:** unchanged (absent).
**Status:** unchanged. **Error:** unchanged (1–12).

**Functions added (1):** `reassign_buyer`.

## 5. Deployment 5 — two-phase funding and the two uncontested terminal outcomes

**WASM:** `3351e0de…` (`3351e0de0edc918130fb2f88973b90cc717307a789bb0b7ee0802c50f3bd6832`,
22,048 bytes, 19 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-5--2026-09-02) · version `0.4.0`,
**BREAKING** (`lock` escrows only the deposit; `confirm_delivery` re-gated).

**Commitment — 20 fields.** Added:

| field | type | set by |
| --- | --- | --- |
| `remainder_window_secs` | `u64` | `initialize` |
| `delivery_deadline` | `u64` | `initialize` (`created_at + delivery_window_secs`) |
| `remainder_deadline` | `u64` | `ready_for_delivery` (0 = not opened) |
| `remainder_funded` | `bool` | `fund_remainder` |

ScMap key order:

```
advance1_bps, advance1_claimed, advance1_deadline, advance1_expired,
advance2_bps, advance2_claimed, advance2_deadline, advance2_expired, buyer,
claim_window_secs, cooperative, created_at, delivery_deadline,
remainder_deadline, remainder_funded, remainder_window_secs, status, token,
total_amount, warehouse_operator
```

**OracleConfig / AllocationMember:** unchanged (absent).

**Status — 12 variants declared.** `ReadyForDelivery` inserted after
`Advance2Released`, and `Forfeited` inserted before `Disputed`. The insertion
position is invisible on the wire (rule 3 above) — but a client built against
Deployments 1–4 does not know either new symbol and will fail to decode an
instance that reports one. Reachable: the D1 set, plus `Cancelled`,
`ReadyForDelivery`, `Defaulted` (via `expire_remainder_window`) and `Forfeited`
(via `reclaim_on_nondelivery`). `Disputed` remains unreachable.

**Error — 5 added:** `RemainderAlreadyFunded=13`, `RemainderWindowPassed=14`,
`RemainderWindowNotPassed=15`, `RemainderNotFunded=16`,
`DeliveryDeadlineNotPassed=17`.

**Functions added (4):** `ready_for_delivery`, `fund_remainder`,
`expire_remainder_window`, `reclaim_on_nondelivery`.

## 6. Deployment 6 — shortfall/grade schedule

**WASM:** `f935d469…` (`f935d469a6714a0c00075dab1fb26dd4a309d85cc4e321be1256d3be1ff28c8f`,
25,370 bytes, 19 exported functions — no new functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-6--2026-09-03) · version `0.5.0`,
**BREAKING** (`initialize` and `confirm_delivery` signatures; `settle` math).

**Commitment — 25 fields.** Added:

| field | type | set by |
| --- | --- | --- |
| `contracted_quantity` | `u32` | `initialize` |
| `grade_price_bps` | `Vec<u32>` | `initialize` |
| `delivered_quantity` | `u32` | `confirm_delivery` |
| `grade_index` | `u32` | `confirm_delivery` |
| `settlement_bps` | `u32` | `confirm_delivery` |

ScMap key order:

```
advance1_bps, advance1_claimed, advance1_deadline, advance1_expired,
advance2_bps, advance2_claimed, advance2_deadline, advance2_expired, buyer,
claim_window_secs, contracted_quantity, cooperative, created_at,
delivered_quantity, delivery_deadline, grade_index, grade_price_bps,
remainder_deadline, remainder_funded, remainder_window_secs, settlement_bps,
status, token, total_amount, warehouse_operator
```

**Meaning change, not a shape change:** before this deployment `settlement_bps`
existed only from `confirm_delivery` onward and `settle` swept the whole
balance; from here `settle` pays `settlement_bps` of `total_amount` and refunds
the rest. A decoder that only checks the field's presence cannot tell the two
apart — use the WASM hash.

**OracleConfig / AllocationMember:** unchanged (absent).

**Status:** unchanged (12 declared / 10 reachable).
**Error — 3 added:** `InvalidQuantity=18`, `InvalidGradeSchedule=19`,
`InvalidGradeIndex=20`.

**Functions added:** none.

## 7. Deployment 7 — allocation ledger

**WASM:** `9f1843c9…` (`9f1843c9c9c299182d307f7d9a25bfe1b7558ff2f8313daf0aa233aa2adb014e`,
28,767 bytes, 21 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-7--2026-09-04) · version `0.6.0`.

**Commitment:** unchanged from Deployment 6 — same 25 fields, same key order.
The allocation ledger lives under a separate `DataKey::Allocation`, not inside
`Commitment`, so `get_commitment` exposes none of it.

**AllocationMember — 2 fields, first appearance.**
Declaration order: `member_hash, share_bps`. ScMap key order:

```
member_hash, share_bps
```

(`member_hash: BytesN<32>`, `share_bps: u32`.) Never changed after this.

**OracleConfig:** still absent.
**Status:** unchanged. **Error — 3 added:** `AllocationAlreadySet=21`,
`InvalidAllocation=22`, `AllocationNotSet=23`.

**Functions added (2):** `set_allocation`, `get_allocation`.

## 8. Deployment 8 — Reflector oracle read

**WASM:** `5322b601…` (`5322b601fc6ea29e4edfaac260573347d6b47672ecaeb5e13e7cbd75ff0afcab`,
33,488 bytes, 23 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-8--2026-09-04) · version `0.7.0`,
**BREAKING** (`initialize` gains an `oracle_config` argument).

**OracleConfig — 3 fields, first appearance.** Added:

| field | type |
| --- | --- |
| `oracle_contract` | `Address` |
| `price_asset` | `Symbol` |
| `max_age_secs` | `u64` |

ScMap key order:

```
max_age_secs, oracle_contract, price_asset
```

Stored under its own `DataKey::OracleConfig` and read by `get_oracle_config`/
`oracle_rate`; it is **not** a `Commitment` field. An instance configured with
`None` has no `OracleConfig` entry at all.

**Commitment:** unchanged from Deployment 7 — same 25 fields.
**AllocationMember:** unchanged.

**Status:** unchanged. **Error — 4 added:** `InvalidOracleConfig=24`,
`OracleNotConfigured=25`, `OraclePriceUnavailable=26`, `OracleStale=27`.

**Functions added (2):** `get_oracle_config`, `oracle_rate`.

## 9. Deployment 9 — FX shortfall wired into settlement

**WASM:** `7940147d…` (`7940147db99f8222a0710de46566a479d42dbf0626575d17af16e1698aa045f0`,
42,175 bytes, 26 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-9--2026-09-06) · version `0.8.0`,
**BREAKING** (changes `settle`'s gating and payout math).

**Commitment — 30 fields.** Added:

| field | type | set by |
| --- | --- | --- |
| `fx_resolved` | `bool` | `resolve_fx_shortfall` |
| `fx_adjusted_total` | `i128` | `resolve_fx_shortfall` |
| `fx_shortfall_amount` | `i128` | `resolve_fx_shortfall` |
| `fx_shortfall_funded` | `bool` | `fund_fx_shortfall` |
| `fx_shortfall_deadline` | `u64` | `resolve_fx_shortfall` (0 when no shortfall) |

ScMap key order:

```
advance1_bps, advance1_claimed, advance1_deadline, advance1_expired,
advance2_bps, advance2_claimed, advance2_deadline, advance2_expired, buyer,
claim_window_secs, contracted_quantity, cooperative, created_at,
delivered_quantity, delivery_deadline, fx_adjusted_total, fx_resolved,
fx_shortfall_amount, fx_shortfall_deadline, fx_shortfall_funded, grade_index,
grade_price_bps, remainder_deadline, remainder_funded, remainder_window_secs,
settlement_bps, status, token, total_amount, warehouse_operator
```

**OracleConfig — 4 fields.** Added `denominated_amount: i128` (the deal's value
in `price_asset`, 7-decimal units). ScMap key order becomes:

```
denominated_amount, max_age_secs, oracle_contract, price_asset
```

This is a decode-breaking change *within* `OracleConfig`: a decoder expecting
four fields cannot read a Deployment 8 instance's three-field map. It is
unreachable in practice only because Deployment 8 has no `get_oracle_config`
function to call — the stored value is still under the same `DataKey`.

**AllocationMember:** unchanged.

**Status:** unchanged. **Error — 7 added:** `FxAlreadyResolved=28`,
`FxNotResolved=29`, `FxShortfallUnfunded=30`, `NoFxShortfall=31`,
`FxShortfallAlreadyFunded=32`, `FxShortfallWindowPassed=33`,
`FxShortfallWindowNotPassed=34`.

**Functions added (3):** `resolve_fx_shortfall`, `fund_fx_shortfall`,
`expire_fx_shortfall_window`.

## 10. Deployment 10 — dispute flagging

**WASM:** `21a4bb2e…` (`21a4bb2efb129d0252d32b575f2ca595259b1c2fe841d2d0554dc02c44aca580`,
47,171 bytes, 29 exported functions) ·
[CHANGELOG](../CHANGELOG.md#deployment-10--2026-09-06) · version `0.9.0`.

**Commitment — 32 fields.** Added:

| field | type | set by |
| --- | --- | --- |
| `dispute_pre_status` | `Status` | `flag_dispute` (`Draft` = unset) |
| `dispute_deadline` | `u64` | `flag_dispute` (0 = not disputed) |

ScMap key order:

```
advance1_bps, advance1_claimed, advance1_deadline, advance1_expired,
advance2_bps, advance2_claimed, advance2_deadline, advance2_expired, buyer,
claim_window_secs, contracted_quantity, cooperative, created_at,
delivered_quantity, delivery_deadline, dispute_deadline, dispute_pre_status,
fx_adjusted_total, fx_resolved, fx_shortfall_amount, fx_shortfall_deadline,
fx_shortfall_funded, grade_index, grade_price_bps, remainder_deadline,
remainder_funded, remainder_window_secs, settlement_bps, status, token,
total_amount, warehouse_operator
```

**OracleConfig (4) / AllocationMember (2):** unchanged from Deployment 9.

**Status:** declared list unchanged (12); `Disputed` becomes reachable. All 12
variants are now reachable — the first deployment where that is true.
**Error — 2 added:** `NotAParty=35`, `DisputeWindowNotPassed=36`.

**Functions added (3):** `flag_dispute`, `resolve_dispute`,
`expire_dispute_window`.

## 11. Unreleased — stale-quote signal, guard caps, and the #249 bound

**WASM:** not uploaded. When it is, [versioning.md](./versioning.md) §3 calls it
`0.9.1` (patch: same ABI family as Deployment 10 plus additive fields and one
new error). Source: `main` as of this writing (31 exported functions).

**Commitment — 33 fields.** Added:

| field | type | set by |
| --- | --- | --- |
| `fx_quote_stale` | `bool` | `refresh_fx_quote_status` (informational; no guard reads it) |

ScMap key order:

```
advance1_bps, advance1_claimed, advance1_deadline, advance1_expired,
advance2_bps, advance2_claimed, advance2_deadline, advance2_expired, buyer,
claim_window_secs, contracted_quantity, cooperative, created_at,
delivered_quantity, delivery_deadline, dispute_deadline, dispute_pre_status,
fx_adjusted_total, fx_quote_stale, fx_resolved, fx_shortfall_amount,
fx_shortfall_deadline, fx_shortfall_funded, grade_index, grade_price_bps,
remainder_deadline, remainder_funded, remainder_window_secs, settlement_bps,
status, token, total_amount, warehouse_operator
```

**OracleConfig (4) / AllocationMember (2):** unchanged from Deployment 10.

**Status:** unchanged (12 declared, 12 reachable).

**Error — 3 added:** `GradeScheduleTooLong=37`, `AllocationTooLarge=38`,
`AmountTooLarge=39`. The first two cap `grade_price_bps` (16 entries) and
`set_allocation`'s member list (200); `AmountTooLarge` is #249's
`initialize`-time bound on `total_amount` (`<= i128::MAX / 10_000`), added so
`bps_amount`'s `total * bps` can never overflow later in `lock`/`claim_*`/
`settle`. None of the three changes an existing number.

**Functions added (2):** `refresh_fx_quote_status`, `available_actions`.

## 12. Deployment 11 — a hash with Deployment 10's schema

**WASM:** `e097f1ff…` (`e097f1ffe08f878007cd9511d5fc70da0e238ac64491181e158d58f91e4714c5`,
60,043 bytes, **29 exported functions**) ·
[CHANGELOG](../CHANGELOG.md#deployment-11--2026-10-20) · version `0.9.1`-class
patch under [versioning.md](./versioning.md) §2 (no contract code changed; the
entry records cost measurements only).

29 exported functions is Deployment 10's count: the build predates both
`refresh_fx_quote_status` and `available_actions`. It therefore also predates
`fx_quote_stale`, which landed in the same commit as `refresh_fx_quote_status`.
**Treat `e097f1ff…` as exactly Deployment 10's shape (32 fields); not
Unreleased's.** This is an inference from the recorded export count and the
source history, and it is the one row here that cannot be confirmed by a
staleness-free read of the recorded artifact — if you have the instance, read
`get_commitment` once and check for a `fx_quote_stale` key before relying on it.

## 13. Which changes are decode-breaking

Read this as: *a client generated from the latest bindings, pointed at an
instance of deployment N, calling `get_commitment`.*

`get_commitment` fails for **every** deployment from 1 to 10, because the latest
`Commitment` has 33 fields and no earlier build has all of them. What differs is
how much is missing:

| Instance | Fields the latest decoder expects but the instance lacks | `get_commitment` decodes? |
| --- | --- | --- |
| D1 | 24 of 33 — everything added in D2, D5, D6, D9, D10, Unreleased | no |
| D2, D3, D4 | 17 — the D5, D6, D9, D10 and Unreleased fields | no |
| D5 | 13 — the D6, D9, D10 and Unreleased fields | no |
| D6, D7, D8 | 8 — the five D9 fields, two D10 fields, and `fx_quote_stale` | no |
| D9 | 3 — `dispute_pre_status`, `dispute_deadline`, `fx_quote_stale` | no |
| D10 | 1 — `fx_quote_stale` | no |
| D11 (`e097f1ff…`) | 1 — `fx_quote_stale` (see §12) | no |
| Unreleased | none | yes |

The same table read the other way — *bindings from deployment N, pointed at a
newer instance* — is uniformly safe for `get_commitment`: unknown keys are
ignored, so a D10-built client reads a D1 instance and an Unreleased instance
alike. The exceptions are not in the map shape:

- **Unknown `Status` symbol.** A pre-D5 client decoding a D5+ instance that is
  `ReadyForDelivery` or `Forfeited` fails: it doesn't know the symbol. A pre-D10
  client fails on `Disputed`. Values it does know are never reinterpreted (the
  enum is name-encoded, so the D5 insertion renumbers nothing).
- **Unknown `Error` number.** Pre-D2 clients can't name errors 6–12, pre-D5
  clients 13–17, and so on. Numbers are append-only, so no old code changes
  meaning — a client should surface unfamiliar codes as "unknown error N", never
  guess.
- **Functions that didn't exist.** `get_allocation` on a pre-D7 instance,
  `get_oracle_config`/`oracle_rate` on pre-D8, the FX trio on pre-D9, the
  dispute trio on pre-D10, `refresh_fx_quote_status`/`available_actions` on
  everything before the next upload: the host rejects the invocation as an
  unknown function before any decoding happens.
- **`OracleConfig` gained a field in D9.** A four-field decoder cannot read a
  three-field Deployment 8 config. In practice Deployment 8 has no
  `get_oracle_config`, so this is only reachable by reading storage directly.
- **Semantics that a shape check cannot see.** `settlement_bps` exists before
  D6 but only affects the payout from D6; `lock` escrows the full total before
  D5 and the deposit from D5. If a payer amount or a settlement outcome matters,
  branch on the WASM hash, not on the presence of a field.

**Practical rule for an indexer / event timeline / API:** key everything on the
WASM hash, not on a version string. Decode with the struct definition for that
hash (columns above), and treat "field absent" as a normal old-instance state
rather than a corrupt record. `docs/versioning.md` §3 maps hash → version label;
this file maps hash → shape.

## 14. Maintaining this file

A new deployment that changes any of these five shapes needs a section here in
the same PR that adds its CHANGELOG entry: the added fields with types, the new
alphabetical ScMap key order, the `Status`/`Error` additions with discriminants,
the new exported functions, and the hash. A deployment that changes no shape
(Deployment 11) gets a one-line note pointing at the shape it reuses. If a field
is ever *renamed*, *retyped* or removed, that is not an addition — say so
explicitly in its own subsection, because it breaks the "purely additive"
property §0 relies on.
