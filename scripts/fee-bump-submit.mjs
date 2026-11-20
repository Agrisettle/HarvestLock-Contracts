#!/usr/bin/env node

import {
  Address,
  authorizeEntry,
  BASE_FEE,
  Contract,
  Keypair,
  Networks,
  Operation,
  TransactionBuilder,
  rpc,
} from "@stellar/stellar-sdk";

const [contractId, method] = process.argv.slice(2);
const allowedMethods = new Set(["claim_advance_1", "cancel"]);

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`Missing required environment variable: ${name}`);
  }
  return value;
}

function authAddress(entry) {
  const credentials = entry.credentials;
  switch (credentials.type) {
    case "sorobanCredentialsAddress":
      return Address.fromScAddress(credentials.address.address).toString();
    case "sorobanCredentialsAddressV2":
      return Address.fromScAddress(credentials.addressV2.address).toString();
    default:
      throw new Error(
        `Unsupported Soroban authorization credential: ${credentials.type}`,
      );
  }
}

async function main() {
  if (!contractId || !method || process.argv.length !== 4) {
    throw new Error(
      "Usage: node scripts/fee-bump-submit.mjs <contract-id> <claim_advance_1|cancel>",
    );
  }
  if (!allowedMethods.has(method)) {
    throw new Error(`Unsupported demo method: ${method}`);
  }

  const source = Keypair.fromSecret(requiredEnv("INNER_SOURCE_SECRET"));
  const sponsor = Keypair.fromSecret(requiredEnv("SPONSOR_SECRET"));
  if (source.publicKey() === sponsor.publicKey()) {
    throw new Error("The fee-bump sponsor must be a different account from the inner source");
  }

  const signers = new Map([[source.publicKey(), source]]);
  if (method === "cancel") {
    const otherParty = Keypair.fromSecret(requiredEnv("OTHER_PARTY_SECRET"));
    if (otherParty.publicKey() === sponsor.publicKey()) {
      throw new Error("The fee-bump sponsor must not also be a cancel party");
    }
    signers.set(otherParty.publicKey(), otherParty);
  }

  const server = new rpc.Server(
    process.env.STELLAR_RPC_URL ?? "https://soroban-testnet.stellar.org",
  );
  const networkPassphrase = Networks.TESTNET;
  const sourceAccount = await server.getAccount(source.publicKey());
  const transaction = new TransactionBuilder(sourceAccount, {
    fee: BASE_FEE,
    networkPassphrase,
  })
    .addOperation(new Contract(contractId).call(method))
    .setTimeout(60)
    .build();
  const prepared = await server.prepareTransaction(transaction);
  const invoke = prepared.operations[0];
  if (invoke.type !== "invokeHostFunction") {
    throw new Error(`Expected an invokeHostFunction operation, got ${invoke.type}`);
  }

  const validUntilLedger = (await server.getLatestLedger()).sequence + 100;
  const signedAuth = await Promise.all(
    (invoke.auth ?? []).map(async (entry) => {
      if (entry.credentials.type === "sorobanCredentialsSourceAccount") {
        return entry;
      }

      const address = authAddress(entry);
      const signer = signers.get(address);
      if (!signer) {
        throw new Error(
          `No party signer was provided for Soroban authorization address ${address}`,
        );
      }
      return authorizeEntry(entry, signer, validUntilLedger, networkPassphrase, address);
    }),
  );
  const innerBuilder = TransactionBuilder.cloneFrom(prepared);
  const envelope = prepared.toEnvelope();
  if (
    envelope.type !== "envelopeTypeTx" ||
    envelope.value.tx.ext.type !== "sorobanData"
  ) {
    throw new Error("Prepared Soroban transaction is missing its resource data");
  }
  innerBuilder.setSorobanData(envelope.value.tx.ext.value);
  innerBuilder.clearOperations();
  innerBuilder.addOperation(
    Operation.invokeHostFunction({
      source: invoke.source,
      func: invoke.func,
      auth: signedAuth,
    }),
  );
  const inner = innerBuilder.build();
  inner.sign(source);

  const feeBumpBaseFee = Math.max(
    Number(BASE_FEE) * 10,
    Number(inner.fee) * 10,
  ).toString();
  const feeBump = TransactionBuilder.buildFeeBumpTransaction(
    sponsor.publicKey(),
    feeBumpBaseFee,
    inner,
    networkPassphrase,
  );
  feeBump.sign(sponsor);

  const submission = await server.sendTransaction(feeBump);
  if (submission.status !== "PENDING") {
    throw new Error(
      `Fee-bump submission was not accepted: ${JSON.stringify(submission)}`,
    );
  }

  const result = await server.pollTransaction(submission.hash);
  if (result.status !== "SUCCESS") {
    throw new Error(`Fee-bumped ${method} failed on-ledger: ${result.status}`);
  }
  console.log(`method: ${method}`);
  console.log(`network: testnet`);
  console.log(`fee-bump transaction hash: ${submission.hash}`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
