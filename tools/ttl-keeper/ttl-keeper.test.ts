import test from "node:test";
import assert from "node:assert/strict";

import { xdr } from "@stellar/stellar-sdk";

import {
  needsExtension,
  parseArgs,
  parseContractFile,
  readContractTtl,
  remainingLedgers,
} from "./ttl-keeper.ts";

const ttl = (instanceRemaining: number, codeRemaining: number) => ({
  id: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
  latestLedger: 100,
  instanceLiveUntil: 100 + instanceRemaining,
  codeLiveUntil: 100 + codeRemaining,
  instanceRemaining,
  codeRemaining,
});

test("extends when either the instance or code entry is below the threshold", () => {
  assert.equal(needsExtension(ttl(99, 10_000), 100), true);
  assert.equal(needsExtension(ttl(100, 100), 100), false);
  assert.equal(needsExtension(ttl(10_000, 99), 100), true);
});

test("remaining ledgers never goes negative for an archived entry", () => {
  assert.equal(remainingLedgers(0, 123), 0);
  assert.equal(remainingLedgers(150, 123), 27);
});

test("parses contract lists while ignoring comments and blank lines", () => {
  assert.deepEqual(parseContractFile("# keeper list\nCA1\n\nCA2 # second\r\n"), ["CA1", "CA2"]);
});

test("requires a source identity for real runs", () => {
  assert.throws(
    () => parseArgs(["CA1"], {}),
    /real runs require .*STELLAR_SOURCE/,
  );
  const options = parseArgs(["--dry-run", "CA1"], {});
  assert.equal("help" in options, false);
  if (!("help" in options)) assert.equal(options.dryRun, true);
});

test("reads instance and Wasm-code TTLs from the RPC response", async () => {
  const wasmHash = new xdr.Hash(new Uint8Array(32).fill(7));
  const calls: unknown[][] = [];
  const server = {
    async getLedgerEntries(...keys: unknown[]) {
      calls.push(keys);
      if (keys.length === 1) {
        return {
          latestLedger: 100,
          entries: [{
            key: { type: "contractData" },
            liveUntilLedgerSeq: 140,
            val: {
              type: "contractData",
              value: {
                val: {
                  type: "scvContractInstance",
                  value: { executable: { type: "contractExecutableWasm", wasmHash } },
                },
              },
            },
          }],
        };
      }
      return {
        latestLedger: 105,
        entries: [
          { key: { type: "contractData" }, liveUntilLedgerSeq: 140 },
          { key: { type: "contractCode" }, liveUntilLedgerSeq: 205 },
        ],
      };
    },
  };

  const result = await readContractTtl(
    server,
    "CDL3RTC6A52B4XJTD7YQ4SWL6RVLZQ44NMOSYC2ENIR3DPKEINB7RC2Q",
  );
  assert.equal(calls.length, 2);
  assert.equal(result.instanceRemaining, 35);
  assert.equal(result.codeRemaining, 100);
});
