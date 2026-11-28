# Migration runbook — what happens to in-flight commitments when a new WASM is released

This describes the design **as it exists today**. It does not argue for or
against an upgrade path; whether one should exist is a separate open question
(#47) and nothing here pre-empts its answer.

The premise, from [threat-model.md](./threat-model.md) entry 26: there is no
admin key, no proxy, and no upgrade function. A deployed instance runs the WASM
it was deployed with **forever**. Uploading a new build creates a new hash; it
does not change any instance that already exists.

## 1. What a new WASM does and does not affect

| Artifact | Changes at a WASM release? |
| --- | --- |
| The new build's hash | Yes — that is the release |
| Instances created **after** the API is repointed | Yes — they run the new hash |
| Instances created **before** it | **No** — same code, same storage, forever |
| `ESCROW_WASM_HASH` in the API environment | Yes — it is the pointer new deals read |
| An open commitment's `Commitment` struct | No — nothing rewrites it in place |

`ESCROW_WASM_HASH` is read when the API creates a contract instance for a new
deal. The sibling `api/` repo deploys one instance per commitment, so "the
contract version" is a per-deal property, not a global one: two live deals can
legitimately be on different hashes at the same time. The hash a deal was
created with is the hash it will always have.

There is no function that migrates state, and no off-chain job that can
re-create a commitment without the parties re-signing. The only way a *live*
commitment gets onto new code is to end it and start a new one.

## 2. Operator checklist at a release

1. **Mint the version and record the hash.** [versioning.md](./versioning.md) §2
   and §3 for the label, [CHANGELOG.md](../CHANGELOG.md) for the entry, and
   [commitment-schema-history.md](./commitment-schema-history.md) for the shape
   the new hash produces.
2. **Upload before repointing.** `stellar contract upload` the build, then set
   `ESCROW_WASM_HASH` to the returned hash. Reversing these two steps is the
   process gap HANDOFF Deployment 4 records: instance creation references a hash
   that must already exist on-chain, and fails otherwise. Uploading is not the
   same as deploying an instance.
3. **Do not touch existing commitments.** There is nothing to run against them.
   Their rows in the API keep their own contract ID; the new
   `ESCROW_WASM_HASH` is irrelevant to them.
4. **Keep the old decoder reachable until the old deals are terminal.** An
   indexer or API that decodes `get_commitment` must use the shape for the
   instance's hash, not the latest bindings — see
   [commitment-schema-history.md](./commitment-schema-history.md) §13. This is
   the one concrete engineering task a release creates.
5. **Tell the parties what the release does to them:** nothing, unless they
   choose to move a live deal (§3–§4).

## 3. Per-status: what a party can do about a live deal

`cancel` is the **only** route from a live commitment to a new one. It requires
the buyer **and** the cooperative in the same invocation (the warehouse operator
does not sign), and is accepted from any status **before `Delivered`**:
`Draft`, `Locked`, `Advance1Released`, `CheckpointPassed`, `Advance2Released`,
`ReadyForDelivery`.

`cancel` refunds the contract's **entire current token balance** to the buyer
and sets `Status::Cancelled`. It does not claw back anything already paid out.
Terminal outcomes and their payouts are in
[terminal-outcomes.md](./terminal-outcomes.md); this table only covers the
migration decision.

| Status at release | `cancel` available? | Buyer gets refunded | Cooperative keeps | What is lost by cancelling | If cancel is not what you want |
| --- | --- | --- | --- | --- | --- |
| `Draft` | Yes | Nothing (0 escrowed) | — | Only gas | Nothing to migrate; abandon it or proceed |
| `Locked` | Yes | The whole deposit | Nothing yet | Gas; the deal is gone | Finish on the old code |
| `Advance1Released` | Yes | Deposit **minus any claimed advance 1** | Advance 1, if claimed | Any claim already made is not recovered | Finish on the old code |
| `CheckpointPassed` | Yes | Deposit minus claimed advance 1 | Advance 1, if claimed | As above | Finish on the old code |
| `Advance2Released` | Yes | Deposit minus claimed advances 1 and 2 | Both claimed advances | Both claims are not recovered | Finish on the old code |
| `ReadyForDelivery` | Yes | The whole remaining balance (deposit + funded remainder, minus claims) | Any claimed advances | Remainder funded is returned to the buyer; claims stay | Finish on the old code, or let the deadline run |
| `Delivered` | **No** — past the cancel range | — | — | — | `settle` on the old code (permissionless) |
| `Disputed` | **No** | — | — | — | `resolve_dispute` (all three parties) or `expire_dispute_window` (permissionless) first; then see the restored status |
| `Settled`, `Cancelled`, `Defaulted`, `Forfeited` | No — absorbing | — | — | — | Nothing left to do; the deal is over |

Notes the table compresses:

- **Cancelling from `Disputed` is a two-step.** A dispute freezes every other
  transition. Resolve it first; it restores the exact pre-dispute status. If
  that status is `Delivered`, `cancel` is still unavailable (the dispute could
  have been flagged from `Delivered`), and the deal must settle on the old code.
- **An unclaimed tranche is still refundable.** If an advance was released but
  never claimed, its money is still in the contract, so `cancel` returns it with
  the rest of the balance — the cooperative only keeps what it actually claimed.
- **Cancellation is not a default.** It records no reputation consequence
  (`terminal-outcomes.md` §7): neither party is marked as having failed.
- **Nothing forces a decision at release time.** The old deal keeps working
  under the old WASM. Cancelling is a convenience for parties who want the new
  code now, not a maintenance obligation.

## 4. Exact steps for moving a live commitment

This is a **replace**, not a migrate: a new contract instance with a new ID and
a fresh `Commitment`. There is no atomic swap between the two instances, so the
parties have to sequence it themselves.

1. **Agree the unwind and the new terms off-chain.** `cancel` needs both
   signatures, and the replacement is a brand-new deal whose terms
   (`initialize`'s arguments) can differ from the old one. Agree both before
   anyone signs anything.
2. **Confirm the replacement build is uploaded** and `ESCROW_WASM_HASH` points
   at it (§2, steps 2–3), or the instance creation for the replacement will
   fail after the old deal has already been cancelled.
3. **Buyer and cooperative co-sign `cancel` on the old instance.** One
   invocation with two auth entries. In the `api/` repo this is
   `submitMultiPartyCall` (`api/test/helpers.ts`); signing the same transaction
   object twice with the SDK is **not** equivalent and fails with
   `TxBadAuthExtra` — the per-entry `SorobanAuthorizationEntry` signatures are
   what the host checks (HANDOFF Deployment 3 records the two false starts).
4. **Verify, don't assume.** Read back `get_status` → `Cancelled`, and the
   token balance → the buyer's before/after difference equal to the contract's
   previous balance. A submitted transaction is not evidence the refund landed.
5. **Create the replacement.** The new instance's `initialize` is buyer-signed;
   it takes the oracle config (if any), windows, quantity and grade schedule
   afresh. If the buyer is re-funding from the refund, `cancel` must land first.
6. **Re-record the allocation, if there is one.** The ledger lives in the old
   instance's storage; nothing carries across. The cooperative calls
   `set_allocation` on the new instance from `Draft`, generating a **fresh salt
   per member** — reusing the old `member_hash` values would defeat the erasure
   property the ledger is built around (see
   [allocation-salt-analysis.md](./allocation-salt-analysis.md)).
7. **Re-lock and continue.** `lock` on the new instance starts the new deal at
   `Draft → Locked`. All deadlines are computed from the new `created_at`; the
   old deadlines are irrelevant and are not carried over.

What is **not** recoverable through any of this: advances already claimed (they
are the cooperative's money at that point), gas already spent, and the old
instance's allocation ledger as a *live* record — it remains on-chain for audit
but no function on the new instance reads it.

## 5. What changes in the API

Only one setting matters for this decision:

- **`ESCROW_WASM_HASH`** — the build new instances are created from. Repointing
  it affects future deals only. It is not read by, and cannot affect, a deal
  that already has a contract ID.
- **The decoder for old instances** — not a setting, but the other half of a
  release: read each commitment's shape from its own hash
  ([commitment-schema-history.md](./commitment-schema-history.md)). A released
  upgrade to the latest bindings will fail on old instances' `get_commitment`,
  because fields added after that instance was deployed are absent.

## 6. Relationship to the versioning scheme

[versioning.md](./versioning.md) §1 says it plainly: there is no in-place
upgrade, and "compatibility" means *callers and ABI*, not live-state migration.
This runbook is the operational corollary — a version bump is a new hash for new
deals, and an old deal moves only by being ended and replaced. Neither document
answers whether an upgrade path *should* exist; that is #47.
