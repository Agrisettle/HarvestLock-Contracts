# Estimated fees by party

This view combines the per-method measurements in
[`gas-costs.md`](./gas-costs.md) with the authorization and lifecycle records
in [`authorization-matrix.md`](./authorization-matrix.md) and
[`0003-lifecycle-narrative.md`](./0003-lifecycle-narrative.md). It is a budget,
not a quote: a transaction's **source account** pays its fees and rent, which
may be a relayer rather than the address supplying contract authorization.
The tables assume each party submits and pays for its own calls; the
permissionless column assumes a separate keeper pays for permissionless calls.

## Assumptions and cost model

Network observations were read on **23 Nov 2026** from Stellar testnet
(Protocol 29). The recorded invocation inclusion fee was **100 stroops per
instruction**. For consistency, the estimates below add that observed
inclusion fee to each method's non-refundable resource fee. They do not model
congestion bidding, asset trustline setup, or exchange-rate conversion.

Most public methods do not yet have a live per-method measurement. For those
calls, the table uses the closest measured call as a proxy; this is explicitly
an estimate, not a claim that two methods have identical costs. The proxies
are:

| Cost class | Proxy and measurement | Non-refundable + inclusion | Simulated refundable rent |
|---|---|---:|---:|
| Authenticated token transfer | `claim_advance_1`, session 2, 20 Oct 2026 | 7,223 | 902,922 |
| Authenticated state update | `mark_checkpoint`, session 2, 20 Oct 2026 | 4,679 | 902,337 |
| Delivery attestation | `confirm_delivery`, session 2, 20 Oct 2026 | 4,684 | 902,392 |
| Permissionless deadline open | `release_advance_1` / `_2`, session 1, 17 Oct 2026 | 2,441 / 2,442 | 13,233 / 6,456 |

The rent column is the **gross refundable amount proposed by simulation**, not
the expected net loss: Soroban refunds most of it after execution. For example,
the real `mark_checkpoint` transaction charged 4,181 stroops including its
100-stroop inclusion fee, after the simulated rent was reconciled. The large
gross figures show the per-call simulated rent footprint; they are not
additive liquidity requirements across sequential calls and should not be
added to the non-refundable estimate as an invoice. Actual rent depends on the
entry's remaining TTL and the live network schedule.

## Contract creation (separate from a lifecycle)

The creator or deployment sponsor pays the one-time upload and the per-instance
deployment/initialization costs. The 3-grade, no-oracle pilot shape is the
default; the worst-case grade/oracle shape is included for planning.

| Payer (assumed) | Calls | Estimated net testnet cost |
|---|---|---:|
| Deployment sponsor | Upload this WASM hash once | 26,442,735 stroops (2.6442735 XLM) |
| Deployment sponsor | Deploy + initialize, 3 grades, no oracle | 3,487,643 stroops (0.3487643 XLM) per commitment |
| Deployment sponsor | Deploy + initialize, 16 grades + oracle | 3,724,839 stroops (0.3724839 XLM) per commitment |

These are real submitted-transaction costs recorded in `gas-costs.md` on
23 Nov 2026, not proxy estimates. `initialize` itself does not require a
party's contract authorization, so its fee payer is whoever submits it.

## Happy path: full delivery and settlement

The path assumes both advances are claimed, the remainder is funded, delivery
is confirmed, and `settle` is called. Calls whose exact method cost is not in
`gas-costs.md` use the proxy class above (`lock`, funding calls, and `settle`
use the authenticated-transfer proxy; `ready_for_delivery` uses the
authenticated-state proxy; `settle` is charged to its permissionless caller).

| Fee payer | Calls submitted | Estimated non-refundable + inclusion | Gross simulated rent |
|---|---|---:|---:|
| Buyer | `lock`, `fund_remainder` | 14,446 stroops | 1,805,844 stroops |
| Cooperative | `claim_advance_1`, `claim_advance_2`, `ready_for_delivery` | 19,125 stroops | 2,708,181 stroops |
| Warehouse operator | `mark_checkpoint`, `confirm_delivery` | 9,363 stroops | 1,804,729 stroops |
| Permissionless caller | `release_advance_1`, `release_advance_2`, `settle` | 12,106 stroops | 922,611 stroops |

`initialize`, deploy, and upload are excluded from these lifecycle totals and
shown separately above. If the buyer, cooperative, or warehouse is sponsored,
move that call's fee and TTL rent to the transaction source/sponsor.

## Terminal paths

Each row gives the whole path from `lock` through the named outcome. Shared
calls use the same estimates as the happy-path table. Unmeasured terminal
methods use the authenticated-transfer proxy unless they are permissionless,
in which case the same transfer proxy is used with the permissionless caller
as payer. This deliberately conservative proxy does not replace a live
measurement of the terminal method.

### Buyer default: remainder window expires unfunded

| Fee payer | Calls submitted | Estimated non-refundable + inclusion | Gross simulated rent |
|---|---|---:|---:|
| Buyer | `lock` | 7,223 stroops | 902,922 stroops |
| Cooperative | `claim_advance_1`, `claim_advance_2`, `ready_for_delivery` | 19,125 stroops | 2,708,181 stroops |
| Warehouse operator | `mark_checkpoint` | 4,679 stroops | 902,337 stroops |
| Permissionless caller | `release_advance_1`, `release_advance_2`, `expire_remainder_window` | 12,106 stroops | 922,611 stroops |

The buyer does not call `fund_remainder`. If a tranche is reclaimed rather
than claimed, substitute the same authenticated-transfer proxy and move the
fee to the buyer.

### FX shortfall default

This path completes delivery, resolves the oracle amount, then lets the FX
shortfall window expire without a buyer top-up. `resolve_fx_shortfall` and
`expire_fx_shortfall_window` are permissionless; their costs use the
permissionless deadline-open and token-transfer proxies, respectively.

| Fee payer | Calls submitted | Estimated non-refundable + inclusion | Gross simulated rent |
|---|---|---:|---:|
| Buyer | `lock`, `fund_remainder` | 14,446 stroops | 1,805,844 stroops |
| Cooperative | `claim_advance_1`, `claim_advance_2`, `ready_for_delivery` | 19,125 stroops | 2,708,181 stroops |
| Warehouse operator | `mark_checkpoint`, `confirm_delivery` | 9,363 stroops | 1,804,729 stroops |
| Permissionless caller | `release_advance_1`, `release_advance_2`, `resolve_fx_shortfall`, `expire_fx_shortfall_window` | 14,547 stroops | 935,844 stroops |

### Seller non-delivery: buyer reclaims after the delivery deadline

| Fee payer | Calls submitted | Estimated non-refundable + inclusion | Gross simulated rent |
|---|---|---:|---:|
| Buyer | `lock`, `fund_remainder`, `reclaim_on_nondelivery` | 21,669 stroops | 2,708,766 stroops |
| Cooperative | `claim_advance_1`, `claim_advance_2`, `ready_for_delivery` | 19,125 stroops | 2,708,181 stroops |
| Warehouse operator | `mark_checkpoint` | 4,679 stroops | 902,337 stroops |
| Permissionless caller | `release_advance_1`, `release_advance_2` | 4,883 stroops | 19,689 stroops |

If nobody submits `reclaim_on_nondelivery`, the permissionless
`expire_delivery_window` fallback has the same estimated proxy cost, paid by
the keeper instead of the buyer.

### Mutual cancellation

`cancel` requires buyer and cooperative authorization in one transaction.
Whichever account is the transaction source pays its fee and rent; a separate
sponsor can pay without becoming an authorizer. The method is not yet
measured, so budget one authenticated state-update proxy (4,679 stroops plus
902,337 gross simulated rent) for the submitting account. Calls made before
`lock` have no escrow to refund and omit the funding/advance calls above.

| Fee payer | Calls submitted | Estimated non-refundable + inclusion | Gross simulated rent |
|---|---|---:|---:|
| Buyer + cooperative | Both authorize `cancel`; assumed buyer submits/pays | 4,679 stroops (buyer/source) | 902,337 stroops (buyer/source) |
| Warehouse operator | None | 0 | 0 |
| Permissionless caller | None | 0 | 0 |

## Who bears TTL re-rent?

Every ordinary public method loads the instance and calls
`bump_instance_ttl`; `initialize` does so directly. The transaction source
account therefore bears the rent charge triggered by that transaction, even
when another party supplied the required `require_auth()` signature. There is
no fixed “buyer pays TTL” rule in the contract. In production, a relayer or
protocol sponsor can submit and fund the re-rent, while the named buyer,
cooperative, or warehouse still authorizes the action. Read-only polling can
also trigger the bump, so API polling should be budgeted to the API's own
source account rather than charged to a deal party.

The instance TTL is extended to the network maximum, and code-entry rent may
be included the first time a newly uploaded WASM is extended. See
[`gas-costs.md`](./gas-costs.md#wasm-upload-vs-instantiation) and
[`storage-ttl.md`](./storage-ttl.md) for the measured first-extension charge,
refunds, and remaining TTL caveats.
