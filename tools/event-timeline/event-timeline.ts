#!/usr/bin/env -S npx tsx
/**
 * Reference event reader for HarvestLock escrow commitments.
 *
 * Fetches a contract's events from Soroban RPC, decodes each one with the
 * contract spec embedded in the deployed WASM (SEP-48), and prints a
 * human-readable timeline. There are no hand-written decoders here: the same
 * `contract.Spec` the generated bindings use parses every event.
 *
 *   npm install
 *   npx tsx event-timeline.ts --contract-id C... [--rpc-url URL]
 */

import { contract, rpc } from "@stellar/stellar-sdk";

const DEFAULT_RPC_URL = "https://soroban-testnet.stellar.org";
const DEFAULT_LOOKBACK = 20_000;
const DEFAULT_LIMIT = 100;

type Options = {
  contractId: string;
  rpcUrl: string;
  startLedger?: number;
  lookback: number;
  limit: number;
  json: boolean;
};

type TimelineEntry = {
  id: string;
  ledger: number;
  closedAt: string;
  txHash: string;
  event: string;
  data: Record<string, unknown>;
};

function usage(): string {
  return `Usage: event-timeline --contract-id CONTRACT_ID [options]

Prints every event emitted by a HarvestLock escrow contract in ledger order.
Topics and data are decoded with the contract spec embedded in the on-chain
WASM, so newly added events work without changing this script.

Options:
  --contract-id ID      Contract to read (required; C...)
  --rpc-url URL         Soroban RPC endpoint (default: $STELLAR_RPC_URL or
                        ${DEFAULT_RPC_URL})
  --start-ledger N      First ledger to scan (default: latest - --lookback,
                        clamped to the RPC's oldest retained ledger)
  --lookback N          How far back to scan when --start-ledger is omitted
                        (default: ${DEFAULT_LOOKBACK})
  --limit N             Events per getEvents page (default: ${DEFAULT_LIMIT})
  --json                Print a JSON array instead of the timeline
  --help, -h            Show this help

Environment:
  STELLAR_RPC_URL       Overrides the default RPC endpoint
`;
}

function requiredValue(args: string[], index: number, flag: string): string {
  const value = args[index + 1];
  if (!value || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

function nonNegativeInteger(value: string, flag: string): number {
  if (!/^\d+$/.test(value)) {
    throw new Error(`${flag} must be a non-negative integer`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) {
    throw new Error(`${flag} is too large`);
  }
  return parsed;
}

function parseArgs(args: string[], env = process.env): Options | { help: true } {
  let contractId: string | undefined;
  let rpcUrl = env.STELLAR_RPC_URL ?? DEFAULT_RPC_URL;
  let startLedger: number | undefined;
  let lookback = DEFAULT_LOOKBACK;
  let limit = DEFAULT_LIMIT;
  let json = false;

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    switch (arg) {
      case "--help":
      case "-h":
        return { help: true };
      case "--contract-id":
        contractId = requiredValue(args, index, arg);
        index += 1;
        break;
      case "--rpc-url":
        rpcUrl = requiredValue(args, index, arg);
        index += 1;
        break;
      case "--start-ledger":
        startLedger = nonNegativeInteger(requiredValue(args, index, arg), arg);
        index += 1;
        break;
      case "--lookback":
        lookback = nonNegativeInteger(requiredValue(args, index, arg), arg);
        index += 1;
        break;
      case "--limit":
        limit = nonNegativeInteger(requiredValue(args, index, arg), arg);
        index += 1;
        break;
      case "--json":
        json = true;
        break;
      default:
        if (arg.startsWith("-")) {
          throw new Error(`unknown option: ${arg}`);
        }
        if (contractId) {
          throw new Error("only one contract ID may be supplied");
        }
        contractId = arg;
    }
  }

  if (!contractId) {
    throw new Error("--contract-id is required");
  }
  if (limit === 0) {
    throw new Error("--limit must be greater than zero");
  }
  return { contractId, rpcUrl, startLedger, lookback, limit, json };
}

/** JSON.stringify with bigint support (i128 fields decode as bigint). */
function stringify(value: unknown): string {
  return JSON.stringify(
    value,
    (_key, inner) => (typeof inner === "bigint" ? inner.toString() : inner),
    2,
  );
}

function formatData(data: Record<string, unknown>): string {
  return stringify(data).replace(/\n\s*/g, " ").replace(/^\{ /, "").replace(/ \}$/, "");
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2));
  if ("help" in options) {
    console.log(usage());
    return;
  }

  const server = new rpc.Server(options.rpcUrl, {
    allowHttp: options.rpcUrl.startsWith("http://"),
  });

  // The spec lives in the deployed WASM. A Stellar Asset Contract has no WASM,
  // so this fails for a SAC with a clear message of its own.
  let spec: contract.Spec;
  try {
    const wasm = await server.getContractWasmByContractId(options.contractId);
    spec = contract.Spec.fromWasm(wasm);
  } catch (error) {
    throw new Error(
      `could not load the contract spec for ${options.contractId}: ${
        error instanceof Error ? error.message : String(error)
      }\n` +
        "A Stellar Asset Contract has no embedded spec; point this tool at a " +
        "HarvestLock escrow instance.",
    );
  }

  const health = await server.getHealth();
  const startLedger =
    options.startLedger ??
    Math.max(health.oldestLedger, health.latestLedger - options.lookback);
  if (startLedger < health.oldestLedger) {
    throw new Error(
      `--start-ledger ${startLedger} predates the RPC's retained history ` +
        `(oldest retained: ${health.oldestLedger})`,
    );
  }

  const filters: rpc.Api.EventFilter[] = [
    { type: "contract", contractIds: [options.contractId] },
  ];

  const entries: TimelineEntry[] = [];
  const seen = new Set<string>();
  let cursor: string | undefined;
  let pages = 0;

  for (;;) {
    const page = cursor
      ? await server.getEvents({ filters, cursor, limit: options.limit })
      : await server.getEvents({ filters, startLedger, limit: options.limit });
    pages += 1;

    for (const event of page.events) {
      if (seen.has(event.id)) {
        continue;
      }
      seen.add(event.id);

      const parsed = spec.parseEvent(event.topic, event.value);
      entries.push({
        id: event.id,
        ledger: event.ledger,
        closedAt: event.ledgerClosedAt,
        txHash: event.txHash,
        event: parsed?.name ?? "<unrecognised>",
        data: parsed?.data ?? {
          topics: event.topic.map((topic) => topic.toXDR("base64")),
          value: event.value.toXDR("base64"),
        },
      });
    }

    if (page.events.length === 0 || page.cursor === cursor) {
      break;
    }
    cursor = page.cursor;
    if (pages > 10_000) {
      throw new Error("pagination did not terminate");
    }
  }

  if (options.json) {
    console.log(stringify(entries));
    return;
  }

  if (entries.length === 0) {
    console.log(
      `no events for ${options.contractId} in ledgers ${startLedger}..${health.latestLedger}`,
    );
    return;
  }

  console.log(
    `${entries.length} event(s) for ${options.contractId} ` +
      `(ledgers ${entries[0].ledger}..${entries[entries.length - 1].ledger})\n`,
  );
  for (const entry of entries) {
    console.log(
      `ledger ${entry.ledger}  ${entry.closedAt}  ${entry.txHash.slice(0, 8)}  ` +
        `${entry.event}  ${formatData(entry.data)}`,
    );
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
