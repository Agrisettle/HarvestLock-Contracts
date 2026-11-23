/**
 * HarvestLock: collecting multi-party auth for `cancel` (and single-party auth
 * for `lock`, for contrast).
 *
 * The contract's `cancel` guard is:
 *
 *     Self::can_cancel(&c)?;
 *     Self::require_once_each(&[&c.buyer, &c.cooperative]);
 *
 * `require_once_each` calls `require_auth()` once per *distinct* address
 * (contracts/escrow/src/lib.rs). When buyer and cooperative are the same
 * address, there is one auth entry, not two. Soroban's SDK reports those
 * entries through `AssembledTransaction.needsNonInvokerSigningBy()`; each
 * address must sign with its own keypair via `signAuthEntries()`.
 *
 * This example deliberately makes a *third* account the transaction source
 * (`INVOKER_SECRET`). The source signs the transaction envelope, so its
 * credentials are source credentials rather than address credentials. That
 * keeps every `require_auth()` address in `needsNonInvokerSigningBy()` and
 * makes the buyer/cooperative split visible. Using the buyer as the source
 * would hide the buyer's auth entry behind the envelope signature.
 *
 * Roles (buyer, cooperative, invoker) must be three funded testnet
 * identities. To exercise the shared-address case, initialize a contract with
 * buyer == cooperative and set COOPERATIVE_SECRET to the buyer's secret; the
 * example reads the roles back from the contract and expects one signer.
 */
import { Keypair, Networks } from '@stellar/stellar-sdk';
import type { AssembledTransaction } from '@stellar/stellar-sdk/contract';
import { Client } from './bindings/dist/index.js';

type Identity = {
  label: string;
  keypair: Keypair;
  address: string;
};

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`Missing ${name}. See README.md (or copy .env.example).`);
  }
  return value;
}

function identity(label: string, secret: string): Identity {
  const keypair = Keypair.fromSecret(secret);
  return { label, keypair, address: keypair.publicKey() };
}

function labelOf(address: string, identities: Identity[]): string {
  return identities.find((id) => id.address === address)?.label ?? address;
}

function clientFor(
  contractId: string,
  rpcUrl: string,
  networkPassphrase: string,
  invoker: Identity,
): Client {
  return new Client({
    contractId,
    rpcUrl,
    networkPassphrase,
    publicKey: invoker.address,
    signTransaction: invoker.keypair,
  });
}

function assertSameAddresses(
  context: string,
  actual: string[],
  expected: string[],
): void {
  const a = [...actual].sort();
  const e = [...new Set(expected)].sort();
  const same = a.length === e.length && a.every((value, i) => value === e[i]);
  if (!same) {
    throw new Error(
      `${context}: expected ${e.join(', ') || '(none)'}, got ${a.join(', ') || '(none)'}`,
    );
  }
}

/**
 * The heart of the example: ask the assembled transaction which addresses
 * still need auth, then sign each address's entries with that address's own
 * keypair. `needsNonInvokerSigningBy()` already deduplicates by address, which
 * is why buyer == cooperative produces a single signature here, exactly as
 * `require_once_each` does on-chain.
 */
async function signAuthEntries(
  tx: AssembledTransaction<unknown>,
  expectedSigners: string[],
  identities: Identity[],
): Promise<void> {
  const needed = tx.needsNonInvokerSigningBy();
  assertSameAddresses('non-invoker signers', needed, expectedSigners);
  console.log(
    `  needs auth: ${needed.map((address) => labelOf(address, identities)).join(', ') || '(none)'}`,
  );

  for (const address of needed) {
    const signer = identities.find((id) => id.address === address);
    if (!signer) {
      throw new Error(
        `No keypair configured for ${address}. Set the matching *_SECRET for this role.`,
      );
    }
    // One keypair per address: signAuthEntries only touches entries whose
    // top-level address matches `address`.
    await tx.signAuthEntries({ address, signAuthEntry: signer.keypair });
  }

  const remaining = tx.needsNonInvokerSigningBy();
  if (remaining.length > 0) {
    throw new Error(`Unsigned auth entries remain for: ${remaining.join(', ')}`);
  }
}

async function submit(tx: AssembledTransaction<unknown>, label: string): Promise<string> {
  const sent = await tx.signAndSend();
  const hash = sent.sendTransactionResponse?.hash ?? sent.getTransactionResponse?.txHash;
  if (!hash) {
    throw new Error(`${label}: transaction was sent but no hash was returned`);
  }
  console.log(`  ${label} tx: ${hash}`);
  return hash;
}

async function main(): Promise<void> {
  const rpcUrl = process.env.RPC_URL ?? 'https://soroban-testnet.stellar.org';
  const networkPassphrase = process.env.NETWORK_PASSPHRASE ?? Networks.TESTNET;
  const contractId = requireEnv('CONTRACT_ID');

  const buyer = identity('buyer', requireEnv('BUYER_SECRET'));
  const cooperative = identity('cooperative', requireEnv('COOPERATIVE_SECRET'));
  const invoker = identity('invoker', requireEnv('INVOKER_SECRET'));
  const identities = [buyer, cooperative, invoker];

  const client = clientFor(contractId, rpcUrl, networkPassphrase, invoker);

  // Read the roles from the contract instead of trusting the local secrets.
  // This is what makes the same script work for distinct and shared addresses.
  const commitment = (await client.get_commitment()).result.unwrap();
  const cancelSigners = [...new Set([commitment.buyer, commitment.cooperative])];
  console.log(`contract: ${contractId}`);
  console.log(`buyer:       ${commitment.buyer}`);
  console.log(`cooperative: ${commitment.cooperative}`);
  console.log(
    commitment.buyer === commitment.cooperative
      ? 'shared address: require_once_each collapses both roles into one signature'
      : 'distinct addresses: require_once_each requires two signatures',
  );

  for (const [envName, id, onChainRole] of [
    ['BUYER_SECRET', buyer, commitment.buyer],
    ['COOPERATIVE_SECRET', cooperative, commitment.cooperative],
  ] as const) {
    if (id.address !== onChainRole) {
      throw new Error(
        `${envName} (${id.address}) does not match the on-chain ${id.label} role (${onChainRole})`,
      );
    }
  }

  // Contrast: lock only requires the buyer's auth entry.
  console.log('\n1) lock — single signer');
  const lockTx = await client.lock();
  await signAuthEntries(lockTx, [commitment.buyer], identities);
  const lockHash = await submit(lockTx, 'lock');

  // The point of the example: cancel needs buyer + cooperative, deduplicated.
  console.log('\n2) cancel — multi-party, deduplicated by address');
  const cancelTx = await client.cancel();
  await signAuthEntries(cancelTx, cancelSigners, identities);
  const cancelHash = await submit(cancelTx, 'cancel');

  console.log('\nResult:');
  console.log(
    JSON.stringify(
      { contractId, signerCount: cancelSigners.length, lockHash, cancelHash },
      null,
      2,
    ),
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
