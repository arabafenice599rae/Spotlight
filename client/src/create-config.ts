/**
 * Create the DBC config key on devnet with the test partner wallet, print its
 * address. (spec §2)
 *
 *   PARTNER_KEYPAIR=/path/to/partner.json npm run create-config -- devnet
 *
 * mainnet preset is FILE-ONLY per the spec — this script refuses to run it.
 */
import { Keypair } from '@solana/web3.js';
import { DynamicBondingCurveClient } from '@meteora-ag/dynamic-bonding-curve-sdk';
import { configParameters, QUOTE_MINT_SOL, type Preset } from './config.js';
import { connection, loadKeypair, sendTx } from './util.js';

async function main() {
  const preset = (process.argv[2] ?? 'devnet') as Preset;
  if (preset === 'mainnet') {
    throw new Error('mainnet preset is file-only (spec §2): do NOT create it from this script.');
  }
  const partnerPath = process.env.PARTNER_KEYPAIR;
  if (!partnerPath) throw new Error('set PARTNER_KEYPAIR=/path/to/partner.json');

  const conn = connection();
  const partner = loadKeypair(partnerPath);
  const client = DynamicBondingCurveClient.create(conn, 'confirmed');

  const config = Keypair.generate();
  console.log(`partner (feeClaimer): ${partner.publicKey.toBase58()}`);
  console.log(`new config address:   ${config.publicKey.toBase58()}`);

  const tx = await client.partner.createConfig({
    config: config.publicKey,
    feeClaimer: partner.publicKey, // partner claims the trading-fee share
    leftoverReceiver: partner.publicKey, // PROVISIONAL: leftover -> partner
    quoteMint: QUOTE_MINT_SOL, // SOL
    payer: partner.publicKey,
    ...configParameters(preset), // buildCurve(Soluzione A)
  });

  const sig = await sendTx(conn, tx, [partner, config], 'createConfig');
  console.log(`\nCONFIG=${config.publicKey.toBase58()}`);
  console.log(`tx=${sig}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
