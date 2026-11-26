# Events reference — shape, emitters, and RPC filters

This is the reference for every event `harvestlock-escrow` publishes. It is
written for indexers and for anyone decoding `getEvents` by hand. It is backed
by a committed snapshot and a `cargo test`, not just prose:

- `contracts/escrow/events.snapshot` pins the serialized XDR shape of one
  instance of every event.
- `contracts/escrow/tests/events_snapshot.rs` fails when any event's serialized
  topics or data drift from that file, and prints the command to re-bless it.
- `tools/event-timeline/` decodes a real contract's stream with the spec
  embedded in the WASM, so these shapes can be checked against testnet without
  hand-written decoders.

Per [docs/versioning.md](./versioning.md) §2.2, **any change to an event's
shape is a MINOR **BREAKING** bump**: renaming a field, reordering a `#[topic]`,
moving a field between topic and data, or changing its type breaks decoders
silently even though the event still decodes.

## Encoding conventions

Every Soroban contract event has:

- **topics** — an array of `ScVal`s, position-sensitive. Topic `0` is always the
  event name as an `ScVal::Symbol` (the lower-case snake-case name below). Each
  Rust field marked `#[topic]` follows, in declaration order.
- **data** — an `ScVal::Map`. For these events the keys are the Rust field names
  as `ScVal::Symbol`s; values are the non-topic fields. **Decode by key, not by
  map position.**

Scalar encodings used here: `u32`/`bool`/`u64`/`i128` are the obvious `ScVal`
variants (`ScVal::U32`, `ScVal::Bool`, `ScVal::U64`, `ScVal::I128`); `Address`
is `ScVal::Address`; `Symbol` is `ScVal::Symbol`. The two fieldless
`#[contracttype]` enums (`Status`, `DisputeOutcome`) encode as a **single-element
`ScVal::Vec` containing the variant name as an `ScVal::Symbol`** — for example
`Status::Locked` is `ScVal::Vec([ScVal::Symbol("Locked")])`. Decode/matching on
the variant name, not on a numeric index: the base64 below is what the RPC
returns, read back from a live testnet event.

| `Status` | wire value (base64) | | `DisputeOutcome` | wire value (base64) |
| --- | --- | --- | --- | --- |
| `Draft` | `AAAAEAAAAAEAAAABAAAADwAAAAVEcmFmdAAAAA==` | | `Resolved` | `AAAAEAAAAAEAAAABAAAADwAAAAhSZXNvbHZlZA==` |
| `Locked` | `AAAAEAAAAAEAAAABAAAADwAAAAZMb2NrZWQAAA==` | | `Expired` | `AAAAEAAAAAEAAAABAAAADwAAAAdFeHBpcmVkAA==` |
| `Advance1Released` | `AAAAEAAAAAEAAAABAAAADwAAABBBZHZhbmNlMVJlbGVhc2Vk` | | | |
| `CheckpointPassed` | `AAAAEAAAAAEAAAABAAAADwAAABBDaGVja3BvaW50UGFzc2Vk` | | | |
| `Advance2Released` | `AAAAEAAAAAEAAAABAAAADwAAABBBZHZhbmNlMlJlbGVhc2Vk` | | | |
| `ReadyForDelivery` | `AAAAEAAAAAEAAAABAAAADwAAABBSZWFkeUZvckRlbGl2ZXJ5` | | | |
| `Delivered` | `AAAAEAAAAAEAAAABAAAADwAAAAlEZWxpdmVyZWQAAAA=` | | | |
| `Settled` | `AAAAEAAAAAEAAAABAAAADwAAAAdTZXR0bGVkAA==` | | | |
| `Cancelled` | `AAAAEAAAAAEAAAABAAAADwAAAAlDYW5jZWxsZWQAAAA=` | | | |
| `Defaulted` | `AAAAEAAAAAEAAAABAAAADwAAAAlEZWZhdWx0ZWQAAAA=` | | | |
| `Forfeited` | `AAAAEAAAAAEAAAABAAAADwAAAAlGb3JmZWl0ZWQAAAA=` | | | |
| `Disputed` | `AAAAEAAAAAEAAAABAAAADwAAAAhEaXNwdXRlZA==` | | | |

A diagnostic consumer can read a topic `0` value as
`ScVal::Symbol("status_changed")`; an RPC filter must send the serialized form
(base64 below).

### RPC `getEvents` filters

`getEvents` takes a positional `topics` array. Each element is either `"*"`
(match any value at that position) or a base64-encoded `ScVal`. A filter with
fewer segments than the event has topics matches the prefix. Topic `0` symbols:

| Event | Topic `0` base64 (`ScVal::Symbol`) |
| --- | --- |
| `status_changed` | `AAAADwAAAA5zdGF0dXNfY2hhbmdlZAAA` |
| `payout` | `AAAADwAAAAZwYXlvdXQAAA==` |
| `tranche_claimed` | `AAAADwAAAA90cmFuY2hlX2NsYWltZWQA` |
| `tranche_reclaimed` | `AAAADwAAABF0cmFuY2hlX3JlY2xhaW1lZAAAAA==` |
| `remainder_funded` | `AAAADwAAABByZW1haW5kZXJfZnVuZGVk` |
| `fx_shortfall_funded` | `AAAADwAAABNmeF9zaG9ydGZhbGxfZnVuZGVkAA==` |
| `buyer_reassigned` | `AAAADwAAABBidXllcl9yZWFzc2lnbmVk` |
| `dispute_flagged` | `AAAADwAAAA9kaXNwdXRlX2ZsYWdnZWQA` |
| `dispute_ended` | `AAAADwAAAA1kaXNwdXRlX2VuZGVkAAAA` |
| `fx_resolved` | `AAAADwAAAAtmeF9yZXNvbHZlZAA=` |
| `fx_quote_stale_changed` | `AAAADwAAABZmeF9xdW90ZV9zdGFsZV9jaGFuZ2VkAAA=` |
| `delivery_confirmed` | `AAAADwAAABJkZWxpdmVyeV9jb25maXJtZWQAAA==` |
| `allocation_recorded` | `AAAADwAAABNhbGxvY2F0aW9uX3JlY29yZGVkAA==` |
| `oracle_config_replaced` | `AAAADwAAABZvcmFjbGVfY29uZmlnX3JlcGxhY2VkAAA=` |

The `Status` values used by `status_changed` filters are the base64 encodings
in the table above (`Draft`, `Locked`, `Defaulted`, ... are the values at
topics[1]/topics[2]). For example, every transition **into** `Defaulted`, in
JSON-RPC shape:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "getEvents",
  "params": {
    "startLedger": 123456,
    "filters": [
      {
        "type": "contract",
        "contractIds": ["C..."],
        "topics": [
          ["AAAADwAAAA5zdGF0dXNfY2hhbmdlZAAA"],
          ["*"],
          ["AAAAEAAAAAEAAAABAAAADwAAAAlEZWZhdWx0ZWQAAAA="]
        ]
      }
    ],
    "pagination": { "limit": 100 }
  }
}
```

Every other event's section below gives its base filter (`topic 0` only, which
matches all instances); append `"*"` or a base64 value for each additional
indexed topic you want to narrow.

## Event catalogue

| Event | Emitted by | When |
| --- | --- | --- |
| `status_changed` | every status transition via `set_status` | `lock`, `release_advance_1/2`, `mark_checkpoint`, `ready_for_delivery`, `confirm_delivery`, `settle`, `cancel`, `expire_remainder_window`, `expire_fx_shortfall_window`, `reclaim_on_nondelivery`, `expire_delivery_window`, `flag_dispute`, `resolve_dispute`, `expire_dispute_window` |
| `payout` | `settle`, `cancel`, `expire_remainder_window`, `expire_fx_shortfall_window`, `reclaim_on_nondelivery`, `expire_delivery_window` | after each non-zero transfer to a party |
| `tranche_claimed` | `claim_advance_1`, `claim_advance_2` | after the tranche transfer |
| `tranche_reclaimed` | `reclaim_advance_1`, `reclaim_advance_2` | after the refund transfer |
| `remainder_funded` | `fund_remainder` | after the second deposit |
| `fx_shortfall_funded` | `fund_fx_shortfall` | after the FX top-up |
| `buyer_reassigned` | `reassign_buyer` | after the buyer address changes |
| `dispute_flagged` | `flag_dispute` | after entering `Disputed` |
| `dispute_ended` | `resolve_dispute`, `expire_dispute_window` | after restoring the pre-dispute status |
| `fx_resolved` | `resolve_fx_shortfall` | once, after the oracle conversion is recorded |
| `fx_quote_stale_changed` | `refresh_fx_quote_status` | only when the stale flag actually flips |
| `delivery_confirmed` | `confirm_delivery` | after the operator's attestation |
| `allocation_recorded` | `set_allocation` | after the ledger is stored |
| `oracle_config_replaced` | `replace_oracle_config` | after the consented swap |

`initialize` deliberately emits nothing: a commitment created in `Draft` is an
initial state, not a transition into one. A transfer that moves zero tokens
(the #275 fully-prepaid remainder case) also emits no payout/funding event —
there was no funding.

---

### `status_changed`

Every real status transition. Because it is published inside the one function
that assigns `status`, a future transition gets the event by construction.

- Topics: `[symbol("status_changed"), from: Status @topic, to: Status @topic]`
- Data: empty map
- RPC filter (all): `topics: [["AAAADwAAAA5zdGF0dXNfY2hhbmdlZAAA"]]`
- RPC filter (into `Locked`): `topics: [["AAAADwAAAA5zdGF0dXNfY2hhbmdlZAAA"], ["*"], ["AAAAEAAAAAEAAAABAAAADwAAAAZMb2NrZWQAAA=="]]`

`Disputed` appears as a `to` value from `flag_dispute` and as the restored
status only through `dispute_ended.restored`, never as a `status_changed.from`
on a resume: `resolve_dispute`/`expire_dispute_window` transition
`Disputed -> pre_dispute_status`.

### `payout`

A balance-moving transfer to a party in a terminal or payout path. One event per
non-zero leg; `settle` can emit two in one call (cooperative payment and buyer
shortfall refund).

- Topics: `[symbol("payout"), reason: Symbol @topic]`
- Data: `{ to: Address, amount: i128 }`
- `reason` values:

  | `reason` | Emitted by | Recipient |
  | --- | --- | --- |
  | `coop_pay` | `settle` | cooperative |
  | `refund` | `settle` | buyer |
  | `cancel` | `cancel` | buyer |
  | `exp_rem` | `expire_remainder_window` | cooperative |
  | `exp_fx` | `expire_fx_shortfall_window` | cooperative |
  | `forfeit` | `reclaim_on_nondelivery` | buyer |
  | `exp_del` | `expire_delivery_window` | buyer |

- RPC filter (all): `topics: [["AAAADwAAAAZwYXlvdXQAAA=="]]`

### `tranche_claimed`

- Topics: `[symbol("tranche_claimed"), tranche: u32 @topic]` (`1` or `2`)
- Data: `{ to: Address, amount: i128 }`
- Emitted by `claim_advance_1` / `claim_advance_2` after the transfer.
- RPC filter (all): `topics: [["AAAADwAAAA90cmFuY2hlX2NsYWltZWQA"]]`

### `tranche_reclaimed`

- Topics: `[symbol("tranche_reclaimed"), tranche: u32 @topic]` (`1` or `2`)
- Data: `{ to: Address, amount: i128 }`
- Emitted by `reclaim_advance_1` / `reclaim_advance_2` after the refund.
- RPC filter (all): `topics: [["AAAADwAAABF0cmFuY2hlX3JlY2xhaW1lZAAAAA=="]]`

### `remainder_funded`

- Topics: `[symbol("remainder_funded")]`
- Data: `{ from: Address, amount: i128 }`
- Emitted by `fund_remainder`. Not emitted for a zero remainder (#275), since
  no transfer happened.
- RPC filter (all): `topics: [["AAAADwAAABByZW1haW5kZXJfZnVuZGVk"]]`

### `fx_shortfall_funded`

- Topics: `[symbol("fx_shortfall_funded")]`
- Data: `{ from: Address, amount: i128 }`
- Emitted by `fund_fx_shortfall` after the top-up transfer.
- RPC filter (all): `topics: [["AAAADwAAABNmeF9zaG9ydGZhbGxfZnVuZGVkAA=="]]`

### `buyer_reassigned`

- Topics: `[symbol("buyer_reassigned"), previous: Address @topic, new_buyer: Address @topic]`
- Data: empty map
- Emitted by `reassign_buyer` after the write. No funds move.
- RPC filter (all): `topics: [["AAAADwAAABBidXllcl9yZWFzc2lnbmVk"]]`

### `dispute_flagged`

- Topics: `[symbol("dispute_flagged"), flagger: Address @topic]`
- Data: `{ pre_status: Status, deadline: u64 }`
- Emitted by `flag_dispute`. `deadline` is the dispute window's end, computed
  from `remainder_window_secs`.
- RPC filter (all): `topics: [["AAAADwAAAA9kaXNwdXRlX2ZsYWdnZWQA"]]`

### `dispute_ended`

- Topics: `[symbol("dispute_ended"), outcome: DisputeOutcome @topic]`
- Data: `{ restored: Status }`
- Emitted by `resolve_dispute` (`Resolved`) and `expire_dispute_window`
  (`Expired`).
- RPC filter (all): `topics: [["AAAADwAAAA1kaXNwdXRlX2VuZGVkAAAA"]]`
- RPC filter (expired only): `topics: [["AAAADwAAAA1kaXNwdXRlX2VuZGVkAAAA"], ["AAAAEAAAAAEAAAABAAAADwAAAAdFeHBpcmVkAA=="]]`

### `fx_resolved`

- Topics: `[symbol("fx_resolved")]`
- Data: `{ rate: i128, rate_timestamp: u64, fx_adjusted_total: i128, shortfall: i128, deadline: u64 }`
- Emitted once by `resolve_fx_shortfall`. `deadline` is `0` when `shortfall` is
  `0` (no top-up window needed).
- RPC filter (all): `topics: [["AAAADwAAAAtmeF9yZXNvbHZlZAA="]]`

### `fx_quote_stale_changed`

- Topics: `[symbol("fx_quote_stale_changed")]`
- Data: `{ stale: bool }`
- Emitted by `refresh_fx_quote_status` **only when the flag changes**; a probe
  that re-observes the same freshness state writes storage but emits nothing.
- RPC filter (all): `topics: [["AAAADwAAABZmeF9xdW90ZV9zdGFsZV9jaGFuZ2VkAAA="]]`

### `delivery_confirmed`

- Topics: `[symbol("delivery_confirmed"), grade_index: u32 @topic]`
- Data: `{ delivered_quantity: u32, settlement_bps: u32 }`
- Emitted by `confirm_delivery`. These three values are the trust anchor for
  every downstream payout calculation; per-member allocation hashes stay
  off-chain (see `docs/allocation-salt-analysis.md`).
- RPC filter (all): `topics: [["AAAADwAAABJkZWxpdmVyeV9jb25maXJtZWQAAA=="]]`

### `allocation_recorded`

- Topics: `[symbol("allocation_recorded")]`
- Data: `{ member_count: u32, total_share_bps: u32 }`
- Emitted by `set_allocation`. Aggregate shape only — individual member hashes
  are not in the event.
- RPC filter (all): `topics: [["AAAADwAAABNhbGxvY2F0aW9uX3JlY29yZGVkAA=="]]`

### `oracle_config_replaced`

- Topics: `[symbol("oracle_config_replaced")]`
- Data: `{ new_oracle_contract: Address }`
- Emitted by `replace_oracle_config` after the consented swap. The old config
  is not retained; read it from the previous `initialize` transaction's
  arguments if needed.
- RPC filter (all): `topics: [["AAAADwAAABZvcmFjbGVfY29uZmlnX3JlcGxhY2VkAAA="]]`

---

## Reading the stream

`tools/event-timeline/` pages `getEvents`, decodes every event against the
contract spec embedded in the WASM, and prints a ledger/order/time timeline.
That is the reference implementation of the topic layout above and the fastest
way to check a testnet deployment:

```sh
cd tools/event-timeline
npm install
npx tsx event-timeline.ts --contract-id C... --rpc-url https://soroban-testnet.stellar.org
```

Two notes for indexers:

- Soroban's RPC only retains events for a bounded ledger window. Start from a
  ledger at or after the contract's deployment and page forward; `getEvents`
  does not go backwards indefinitely.
- A `StatusChanged` event with `from == to` is not published: every emitter is
  a real transition through `set_status`. Payout/funding events are conditional
  on a non-zero amount, so reconstruct balances from `payout`,
  `tranche_claimed`, `tranche_reclaimed`, `remainder_funded` and
  `fx_shortfall_funded`, not from the mere presence of a call.

## Changing an event

1. Change the Rust event type or its `#[topic]` placement.
2. Run `BLESS_EVENTS_SNAPSHOT=1 cargo test -p harvestlock-escrow --test events_snapshot`
   and review the diff — every changed line is a consumer-visible change.
3. Update this document's section for that event and, per
   [versioning.md](./versioning.md) §2.2, label the deployment MINOR
   **BREAKING**.
4. Commit the snapshot change in the same PR as the Rust change.
