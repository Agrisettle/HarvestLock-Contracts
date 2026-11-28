import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Buffer } from "node:buffer";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { Client } from "../dist/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURE_DIR = resolve(__dirname, "../../../contracts/escrow/tests/fixtures/xdr");

const client = new Client({
  contractId: "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM",
  networkPassphrase: "Test SDF Network ; September 2015",
  rpcUrl: "http://localhost:8000",
  allowHttp: true,
});
const spec = client.spec;

function load(name) {
  return JSON.parse(readFileSync(resolve(FIXTURE_DIR, name), "utf8"));
}

const BUYER = "GD5RFZ4IADL7KS3J2AMCXNUDT3G7V7U4KMTX54UWXO7J3MTICWBS2LSI";
const COOPERATIVE = "GA6BHLUH7BC65ZOBZ2OPAXGTNVM7ZFPTACJ5I6FTEZ2P7FCAKUJFD3SC";
const WAREHOUSE = "GA3FPIF3JAI5AH2PHBYO3AI4QXOJEA5NP6Q7N7QLIX2YFWDUR7SXLWQ3";
const TOKEN = "CAVWBS5USXL5WRIB2HBGKBEABQZ6AT6YH7P4JO452WWEMWPWER5T6RDF";
const ORACLE_CONTRACT = "CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W";

test("initialize args (with oracle_config) round-trip byte equality", () => {
  const rust = load("initialize_with_oracle.json");
  const args = {
    buyer: BUYER,
    cooperative: COOPERATIVE,
    warehouse_operator: WAREHOUSE,
    token: TOKEN,
    total_amount: 100000000n,
    advance1_bps: 1500,
    advance2_bps: 1500,
    claim_window_secs: 604800n,
    remainder_window_secs: 604800n,
    delivery_window_secs: 10368000n,
    contracted_quantity: 1000,
    grade_price_bps: [10000, 9000, 7500],
    oracle_config: {
      oracle_contract: ORACLE_CONTRACT,
      price_asset: "GBP",
      max_age_secs: 300n,
      denominated_amount: 500000n,
    },
  };
  const tsScVals = spec.funcArgsToScVals("initialize", args);
  const tsB64 = tsScVals.map((v) => v.toXDR("base64"));
  assert.deepEqual(tsB64, rust.args);

  // Decode Rust-produced XDR and re-encode: byte equality both ways.
  const fn = spec.getFunc("initialize");
  rust.args.forEach((b64, i) => {
    const ty = fn.inputs()[i].type();
    const native = spec.scValStrToNative(b64, ty);
    const reencoded = spec.nativeToScVal(native, ty).toXDR("base64");
    assert.equal(reencoded, b64);
  });
});

test("initialize args (without oracle_config) round-trip byte equality", () => {
  const rust = load("initialize_without_oracle.json");
  const args = {
    buyer: BUYER,
    cooperative: COOPERATIVE,
    warehouse_operator: WAREHOUSE,
    token: TOKEN,
    total_amount: 100000000n,
    advance1_bps: 1500,
    advance2_bps: 1500,
    claim_window_secs: 604800n,
    remainder_window_secs: 604800n,
    delivery_window_secs: 10368000n,
    contracted_quantity: 1000,
    grade_price_bps: [10000, 9000, 7500],
    oracle_config: null,
  };
  const tsScVals = spec.funcArgsToScVals("initialize", args);
  const tsB64 = tsScVals.map((v) => v.toXDR("base64"));
  assert.deepEqual(tsB64, rust.args);

  const fn = spec.getFunc("initialize");
  rust.args.forEach((b64, i) => {
    const ty = fn.inputs()[i].type();
    const native = spec.scValStrToNative(b64, ty);
    const reencoded = spec.nativeToScVal(native, ty).toXDR("base64");
    assert.equal(reencoded, b64);
  });
});

test("Vec<AllocationMember> round-trip byte equality", () => {
  const rust = load("allocation_members.json");
  const members = [
    { member_hash: Buffer.alloc(32, 0x11), share_bps: 6000 },
    { member_hash: Buffer.alloc(32, 0x22), share_bps: 4000 },
  ];
  const tsScVal = spec.funcArgsToScVals("set_allocation", { members })[0];
  const tsB64 = tsScVal.toXDR("base64");
  assert.equal(tsB64, rust.xdr);

  const ty = spec.getFunc("set_allocation").inputs()[0].type();
  const native = spec.scValStrToNative(rust.xdr, ty);
  const reencoded = spec.nativeToScVal(native, ty).toXDR("base64");
  assert.equal(reencoded, rust.xdr);
});

test("Commitment mid-lifecycle round-trip byte equality", () => {
  const rust = load("commitment_mid_lifecycle.json");
  const commitment = {
    buyer: BUYER,
    cooperative: COOPERATIVE,
    warehouse_operator: WAREHOUSE,
    token: TOKEN,
    total_amount: 100000000n,
    advance1_bps: 1500,
    advance2_bps: 1500,
    claim_window_secs: 604800n,
    remainder_window_secs: 604800n,
    status: { tag: "Delivered" },
    created_at: 1700000000n,
    delivery_deadline: 1700000000n + 10368000n,
    advance1_deadline: 1700000600n,
    advance1_claimed: true,
    advance1_expired: false,
    advance2_deadline: 1700001200n,
    advance2_claimed: true,
    advance2_expired: false,
    remainder_deadline: 1700001800n,
    remainder_funded: true,
    contracted_quantity: 1000,
    grade_price_bps: [10000, 9000, 7500],
    delivered_quantity: 950,
    grade_index: 1,
    settlement_bps: 9000,
    fx_resolved: true,
    fx_adjusted_total: 90000000n,
    fx_shortfall_amount: 5000000n,
    fx_shortfall_funded: true,
    fx_shortfall_deadline: 1700002400n,
    fx_quote_stale: false,
    dispute_pre_status: { tag: "Draft" },
    dispute_deadline: 0n,
  };

  // Encode through spec using get_commitment output type.
  const out = spec.getFunc("get_commitment").outputs()[0];
  const ty = out.switch().name === "scSpecTypeResult"
    ? out.result().okType()
    : out;
  const tsB64 = spec.nativeToScVal(commitment, ty).toXDR("base64");
  assert.equal(tsB64, rust.xdr);

  // Decode Rust-produced XDR and re-encode.
  const native = spec.scValStrToNative(rust.xdr, ty);
  const reencoded = spec.nativeToScVal(native, ty).toXDR("base64");
  assert.equal(reencoded, rust.xdr);
});
