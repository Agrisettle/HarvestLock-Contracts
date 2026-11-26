# HarvestLock event timeline

`event-timeline.ts` reads a HarvestLock escrow contract's event stream from
Soroban RPC and prints a human-readable timeline: ledger, close time,
transaction, event name, and decoded fields.

It exists to answer one question cheaply — *are the event shapes actually
usable by a consumer?* — before the API builds on them. It is also a debugging
tool for testnet walkthroughs, where reconstructing what happened from
`stellar.expert` by hand is slow and error-prone.

There are **no hand-written decoders** in this tool. It fetches the WASM
deployed at the contract ID, builds a `contract.Spec` from the SEP-48 event
declarations embedded in it, and calls `spec.parseEvent(topics, data)`. A new
event added to the contract works here as soon as the new WASM is deployed.

## Install

From this directory:

```sh
npm install
```

Node.js 18 or newer is required (the SDK's TypeScript build targets a modern
runtime; `tsx` runs the script without a separate compile step).

## Run

```sh
npx tsx event-timeline.ts --contract-id C... --rpc-url https://soroban-testnet.stellar.org
```

or, with the package script:

```sh
npm start -- --contract-id C...
```

The default RPC endpoint is Stellar testnet. Override it with `--rpc-url` or
`STELLAR_RPC_URL`.

By default the reader scans the last 20,000 ledgers (clamped to the RPC's
retained window — Soroban RPC keeps roughly seven days of events). For a
deployment older than that, pass `--start-ledger` explicitly:

```sh
npx tsx event-timeline.ts \
  --contract-id C... \
  --start-ledger 5880000 \
  --limit 200
```

`--json` emits the parsed entries as a JSON array (big integers become
decimal strings) for piping into another tool.

## Sample output

Real output from Deployment 12's testnet instance,
`CAB6NIVIIGZ47WM3Y3DFMFJCDAXPRMWBTNJBL74JC2LMC4CDBPBR4TI2` (23 Nov 2026;
initialize → lock → release/claim tranche 1):

```text
3 event(s) for CAB6NIVIIGZ47WM3Y3DFMFJCDAXPRMWBTNJBL74JC2LMC4CDBPBR4TI2 (ledgers 5882214..5882217)

ledger 5882214  2026-11-23T11:51:52Z  658f4bfb  status_changed  { from: "Draft", to: "Locked" }
ledger 5882215  2026-11-23T11:51:57Z  217a25a4  status_changed  { from: "Locked", to: "Advance1Released" }
ledger 5882217  2026-11-23T11:52:07Z  5c2366fc  tranche_claimed  { tranche: 1, to: "GAAAAAAA73EZNVGCIGZX344MLZ7T5R3JANBQNBI6JVTGVXFFZEH7P3AQ", amount: "15000000" }
```

Event retention is bounded, so those ledgers will age out of the public RPC.
Redeploy and re-run for a fresh trace; see the deployment notes in
[`HANDOFF.md`](../../HANDOFF.md).

An event the deployed spec does not declare (for example, a stream from a
newer build with an event this tool's SDK cannot match) is printed as
`<unrecognised>` with its raw base64 topics and data rather than dropped.

## Event shapes

[`docs/events.md`](../../docs/events.md) is the reference for every event's
topic layout, data fields, emitting calls, and `getEvents` filters.
[`contracts/escrow/events.snapshot`](../../contracts/escrow/events.snapshot)
pins the serialized shape, and
`contracts/escrow/tests/events_snapshot.rs` fails CI when it drifts.
