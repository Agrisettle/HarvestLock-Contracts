#!/usr/bin/env -S npx tsx

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

import { Contract, StrKey, rpc, xdr } from "@stellar/stellar-sdk";

const execFileAsync = promisify(execFile);
const DEFAULT_RPC_URL = "https://soroban-testnet.stellar.org";
const DEFAULT_NETWORK = "testnet";
const DEFAULT_THRESHOLD = 100_000;
const DEFAULT_EXTENSION = 500_000;

export type ContractTtl = {
  id: string;
  latestLedger: number;
  instanceLiveUntil: number;
  codeLiveUntil: number;
  instanceRemaining: number;
  codeRemaining: number;
};

export type Options = {
  contractIds: string[];
  file?: string;
  dryRun: boolean;
  threshold: number;
  ledgersToExtend: number;
  rpcUrl: string;
  network: string;
  source?: string;
  stellarBin: string;
};

type ParsedArgs = Options | { help: true };

function usage(): string {
  return `Usage: ttl-keeper [options] CONTRACT_ID...

Reads the persistent instance and Wasm-code TTL for each contract. Contracts
with either entry below --threshold are extended unless --dry-run is set.

Options:
  --file FILE                  Read one contract ID per line (comments allowed)
  --threshold LEDGERS          Extend below this many ledgers (default: ${DEFAULT_THRESHOLD})
  --ledgers-to-extend LEDGERS  Add this many ledgers when extending (default: ${DEFAULT_EXTENSION})
  --rpc-url URL                Soroban RPC endpoint (default: $STELLAR_RPC_URL or ${DEFAULT_RPC_URL})
  --network NAME               Stellar CLI network name (default: $STELLAR_NETWORK or ${DEFAULT_NETWORK})
  --source IDENTITY            Funded Stellar CLI identity (required unless --dry-run)
  --stellar-bin PATH           Stellar CLI executable (default: stellar)
  --dry-run                    Report what would be extended; never submit a transaction
  --help                       Show this help

Environment:
  STELLAR_SOURCE                Source identity used in real runs
  STELLAR_RPC_URL               Overrides the default RPC endpoint
  STELLAR_NETWORK               Overrides the default CLI network name
`;
}

function positiveInteger(value: string, flag: string): number {
  if (!/^\d+$/.test(value)) {
    throw new Error(`${flag} must be a non-negative integer`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) {
    throw new Error(`${flag} is too large`);
  }
  return parsed;
}

function requiredValue(args: string[], index: number, flag: string): string {
  const value = args[index + 1];
  if (!value || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

export function parseContractFile(contents: string): string[] {
  return contents
    .split(/\r?\n/)
    .map((line) => line.replace(/#.*/, "").trim())
    .filter(Boolean);
}

export function needsExtension(ttl: ContractTtl, threshold: number): boolean {
  return ttl.instanceRemaining < threshold || ttl.codeRemaining < threshold;
}

export function remainingLedgers(liveUntilLedgerSeq: number, latestLedger: number): number {
  return Math.max(0, liveUntilLedgerSeq - latestLedger);
}

export function parseArgs(args: string[], env = process.env): ParsedArgs {
  const contractIds: string[] = [];
  let file: string | undefined;
  let dryRun = false;
  let threshold = DEFAULT_THRESHOLD;
  let ledgersToExtend = DEFAULT_EXTENSION;
  let rpcUrl: string | undefined;
  let network: string | undefined;
  let sourceArgument: string | undefined;
  let stellarBin = "stellar";

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    switch (arg) {
      case "--help":
      case "-h":
        return { help: true };
      case "--dry-run":
        dryRun = true;
        break;
      case "--file":
        file = requiredValue(args, index, arg);
        index += 1;
        break;
      case "--threshold":
        threshold = positiveInteger(requiredValue(args, index, arg), arg);
        index += 1;
        break;
      case "--ledgers-to-extend":
        ledgersToExtend = positiveInteger(requiredValue(args, index, arg), arg);
        index += 1;
        break;
      case "--rpc-url":
        rpcUrl = requiredValue(args, index, arg);
        index += 1;
        break;
      case "--network":
        network = requiredValue(args, index, arg);
        index += 1;
        break;
      case "--source":
        sourceArgument = requiredValue(args, index, arg);
        index += 1;
        break;
      case "--stellar-bin":
        stellarBin = requiredValue(args, index, arg);
        index += 1;
        break;
      default:
        if (arg.startsWith("--")) {
          throw new Error(`unknown option: ${arg}`);
        }
        contractIds.push(arg);
    }
  }

  const source = sourceArgument || env.STELLAR_SOURCE;
  if (!dryRun && !source) {
    throw new Error("real runs require --source or STELLAR_SOURCE (a funded stellar CLI identity); use --dry-run to avoid it");
  }

  return {
    contractIds,
    file,
    dryRun,
    threshold,
    ledgersToExtend,
    rpcUrl: rpcUrl || env.STELLAR_RPC_URL || DEFAULT_RPC_URL,
    network: network || env.STELLAR_NETWORK || DEFAULT_NETWORK,
    source,
    stellarBin,
  };
}

function liveUntil(entry: any, label: string): number {
  const value = entry?.liveUntilLedgerSeq;
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} entry did not include a valid liveUntilLedgerSeq`);
  }
  return value;
}

function entryOfType(entries: any[], type: string): any {
  return entries.find((entry) => entry.key?.type === type);
}

export async function readContractTtl(server: { getLedgerEntries: (...keys: any[]) => Promise<any> }, id: string): Promise<ContractTtl> {
  if (!StrKey.isValidContract(id)) {
    throw new Error(`invalid contract ID: ${id}`);
  }

  const instanceKey = new Contract(id).getFootprint();
  const instanceResponse = await server.getLedgerEntries(instanceKey);
  const initialInstance = entryOfType(instanceResponse.entries || [], "contractData");
  if (!initialInstance?.val || initialInstance.val.type !== "contractData") {
    throw new Error(`contract instance not found: ${id}`);
  }

  const instanceValue = initialInstance.val.value?.val?.value;
  const executable = instanceValue?.executable;
  if (!executable || executable.type !== "contractExecutableWasm") {
    throw new Error(`contract is not a Wasm contract: ${id}`);
  }

  const codeKey = xdr.LedgerKey.contractCode(
    new xdr.LedgerKeyContractCode({ hash: executable.wasmHash }),
  );
  const response = await server.getLedgerEntries(instanceKey, codeKey);
  const instance = entryOfType(response.entries || [], "contractData");
  const code = entryOfType(response.entries || [], "contractCode");
  if (!instance || !code) {
    throw new Error(`instance or Wasm-code entry not found: ${id}`);
  }

  const latestLedger = response.latestLedger;
  if (!Number.isSafeInteger(latestLedger)) {
    throw new Error(`RPC response did not include a valid latest ledger: ${id}`);
  }
  const instanceLiveUntil = liveUntil(instance, "instance");
  const codeLiveUntil = liveUntil(code, "code");

  return {
    id,
    latestLedger,
    instanceLiveUntil,
    codeLiveUntil,
    instanceRemaining: remainingLedgers(instanceLiveUntil, latestLedger),
    codeRemaining: remainingLedgers(codeLiveUntil, latestLedger),
  };
}

function formatTtl(ttl: ContractTtl): string {
  return `${ttl.id}: ledger ${ttl.latestLedger}; instance ${ttl.instanceLiveUntil} (${ttl.instanceRemaining} remaining); code ${ttl.codeLiveUntil} (${ttl.codeRemaining} remaining)`;
}

async function extendContract(options: Options, id: string): Promise<void> {
  const args = [
    "contract",
    "extend",
    "--id",
    id,
    "--durability",
    "persistent",
    "--ledgers-to-extend",
    String(options.ledgersToExtend),
    "--source",
    options.source!,
    "--network",
    options.network,
  ];
  try {
    const result = await execFileAsync(options.stellarBin, args, { windowsHide: true });
    if (result.stdout.trim()) process.stdout.write(result.stdout);
    if (result.stderr.trim()) process.stderr.write(result.stderr);
  } catch (error: any) {
    const detail = [error?.stdout, error?.stderr, error?.message].filter(Boolean).join("\n");
    throw new Error(`stellar contract extend failed for ${id}: ${detail}`);
  }
}

export async function run(options: Options, server = new rpc.Server(options.rpcUrl)): Promise<number> {
  if (options.contractIds.length === 0) {
    throw new Error("provide at least one contract ID, or use --file");
  }

  let failures = 0;
  for (const id of options.contractIds) {
    try {
      const before = await readContractTtl(server, id);
      const extend = needsExtension(before, options.threshold);
      console.log(`${formatTtl(before)} -> ${extend ? (options.dryRun ? "would extend" : "extend") : "ok"}`);
      if (!extend || options.dryRun) continue;

      await extendContract(options, id);
      const after = await readContractTtl(server, id);
      const instanceIncreased = after.instanceLiveUntil > before.instanceLiveUntil;
      const codeIncreased = after.codeLiveUntil > before.codeLiveUntil;
      if (!instanceIncreased || !codeIncreased) {
        throw new Error("extension was not confirmed for both persistent entries");
      }
      console.log(`confirmed: ${formatTtl(after)}`);
    } catch (error: any) {
      failures += 1;
      console.error(`${id}: ${error?.message || error}`);
    }
  }
  return failures === 0 ? 0 : 1;
}

async function main(): Promise<void> {
  try {
    const options = parseArgs(process.argv.slice(2));
    if ("help" in options) {
      process.stdout.write(usage());
      return;
    }
    if (options.file) {
      const { readFile } = await import("node:fs/promises");
      options.contractIds.push(...parseContractFile(await readFile(options.file, "utf8")));
    }
    process.exitCode = await run(options);
  } catch (error: any) {
    console.error(`error: ${error?.message || error}`);
    console.error("Run with --help for usage.");
    process.exitCode = 2;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  void main();
}
