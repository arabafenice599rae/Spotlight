/** Shared devnet helpers. No silent retries: every on-chain error is surfaced. */
import fs from 'node:fs';
import {
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  VersionedTransaction,
  type Commitment,
  type Signer,
} from '@solana/web3.js';

export const DEVNET_RPC = process.env.RPC_URL ?? 'https://api.devnet.solana.com';
export const COMMITMENT: Commitment = 'confirmed';

export function connection(): Connection {
  return new Connection(DEVNET_RPC, COMMITMENT);
}

/** Load a Solana CLI keypair JSON (64-byte array). Path via arg or env. */
export function loadKeypair(path: string): Keypair {
  const raw = JSON.parse(fs.readFileSync(path, 'utf8')) as number[];
  if (raw.length !== 64) throw new Error(`${path}: expected 64-byte keypair, got ${raw.length}`);
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

/** Create a fresh devnet wallet, save it, and airdrop `sol` SOL (constraint: dedicated throwaway wallets only). */
export async function fundedWallet(conn: Connection, savePath: string, sol: number): Promise<Keypair> {
  const kp = Keypair.generate();
  fs.writeFileSync(savePath, JSON.stringify(Array.from(kp.secretKey)));
  const sig = await conn.requestAirdrop(kp.publicKey, sol * 1_000_000_000);
  const bh = await conn.getLatestBlockhash();
  await conn.confirmTransaction({ signature: sig, ...bh }, COMMITMENT);
  console.log(`airdropped ${sol} SOL -> ${kp.publicKey.toBase58()} (${savePath})`);
  return kp;
}

/**
 * Sign, send and confirm a legacy Transaction. Throws with the full error (and
 * simulation logs when present) — never swallows or silently retries.
 */
export async function sendTx(
  conn: Connection,
  tx: Transaction,
  signers: Signer[],
  label: string,
): Promise<string> {
  const { blockhash, lastValidBlockHeight } = await conn.getLatestBlockhash();
  tx.recentBlockhash = blockhash;
  tx.feePayer = signers[0].publicKey;
  tx.sign(...signers);
  try {
    const sig = await conn.sendRawTransaction(tx.serialize(), { skipPreflight: false });
    await conn.confirmTransaction({ signature: sig, blockhash, lastValidBlockHeight }, COMMITMENT);
    console.log(`  ${label}: ${sig}`);
    return sig;
  } catch (e: any) {
    const logs = e?.logs ? `\n  logs:\n    ${e.logs.join('\n    ')}` : '';
    throw new Error(`[${label}] transaction failed: ${e?.message ?? e}${logs}`);
  }
}

/** Confirm a VersionedTransaction the SDK already signed/returned (used by some flows). */
export async function sendVersioned(
  conn: Connection,
  tx: VersionedTransaction,
  label: string,
): Promise<string> {
  const sig = await conn.sendTransaction(tx, { skipPreflight: false });
  const bh = await conn.getLatestBlockhash();
  await conn.confirmTransaction({ signature: sig, ...bh }, COMMITMENT);
  console.log(`  ${label}: ${sig}`);
  return sig;
}

export const LAMPORTS_PER_SOL = 1_000_000_000;
export const toLamports = (sol: number) => Math.round(sol * LAMPORTS_PER_SOL);
export const pk = (s: string) => new PublicKey(s);
