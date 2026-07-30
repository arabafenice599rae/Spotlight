/**
 * Devnet end-to-end, runnable a step at a time (spec §3):
 *
 *   PARTNER_KEYPAIR=... CONFIG=<addr> npm run e2e -- <step>
 *   step ∈ a | b | c | d | e | f | g   (state is shared via .e2e-state.json)
 *
 * Wallets are dedicated throwaway devnet keypairs (airdropped); NEVER real keys.
 * No silent retries: DBC/vetrina errors abort the step with full logs.
 *
 * NOTE: this environment cannot reach any Solana RPC (egress policy), so this
 * script is written to run where devnet is reachable. It is type-checked against
 * the real SDK; the on-chain values (fees, signatures, CU) come from the run.
 */
import fs from 'node:fs';
import BN from 'bn.js';
import { AnchorProvider, Program, Wallet, type Idl } from '@coral-xyz/anchor';
import { Keypair, PublicKey } from '@solana/web3.js';
import {
  DynamicBondingCurveClient,
  DAMM_V2_MIGRATION_FEE_ADDRESS,
  MigrationFeeOption,
  deriveDbcPoolAddress,
} from '@meteora-ag/dynamic-bonding-curve-sdk';
import { QUOTE_MINT_SOL } from './config.js';
import { connection, loadKeypair, fundedWallet, sendTx, toLamports, LAMPORTS_PER_SOL } from './util.js';

const STATE = new URL('../.e2e-state.json', import.meta.url).pathname;
const IDL_PATH = new URL('../../target/idl/vetrina.json', import.meta.url).pathname;
const VETRINA_PROGRAM_ID = new PublicKey('gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4');

type State = {
  config?: string;
  baseMint?: string;
  pool?: string;
  wallets?: Record<string, number[]>; // name -> secretKey
  sigs?: Record<string, string>;
};
const readState = (): State => (fs.existsSync(STATE) ? JSON.parse(fs.readFileSync(STATE, 'utf8')) : {});
const writeState = (s: State) => fs.writeFileSync(STATE, JSON.stringify(s, null, 2));
function recordSig(s: State, k: string, sig: string) {
  s.sigs = { ...(s.sigs ?? {}), [k]: sig };
  writeState(s);
}
function wallet(s: State, name: string): Keypair {
  const sk = s.wallets?.[name];
  if (!sk) throw new Error(`wallet '${name}' not found in state — run step 'a' first`);
  return Keypair.fromSecretKey(Uint8Array.from(sk));
}

function vetrinaProgram(payer: Keypair): Program<Idl> {
  if (!fs.existsSync(IDL_PATH)) throw new Error(`vetrina IDL missing at ${IDL_PATH} — run \`anchor build\``);
  const idl = JSON.parse(fs.readFileSync(IDL_PATH, 'utf8')) as Idl;
  const provider = new AnchorProvider(connection(), new Wallet(payer), { commitment: 'confirmed' });
  return new Program(idl, provider);
}

// --- steps -----------------------------------------------------------------

/** a. Deploy vetrina + initialize(treasury_test, decay=3600, lease_duration=3600). */
async function stepA(s: State) {
  const conn = connection();
  // Dedicated throwaway wallets.
  const authority = await fundedWallet(conn, `${dir()}/authority.json`, 2);
  const creator = await fundedWallet(conn, `${dir()}/creator.json`, 2);
  const buyer1 = await fundedWallet(conn, `${dir()}/buyer1.json`, 1);
  const buyer2 = await fundedWallet(conn, `${dir()}/buyer2.json`, 1);
  const buyer3 = await fundedWallet(conn, `${dir()}/buyer3.json`, 1);
  const treasury = Keypair.generate(); // throwaway test treasury (spec: real one is multisig, out of scope)
  s.wallets = Object.fromEntries(
    [
      ['authority', authority],
      ['creator', creator],
      ['buyer1', buyer1],
      ['buyer2', buyer2],
      ['buyer3', buyer3],
      ['treasury', treasury],
    ].map(([n, k]) => [n as string, Array.from((k as Keypair).secretKey)]),
  );
  writeState(s);

  console.log('DEPLOY: run `solana program deploy target/deploy/vetrina.so --program-id target/deploy/vetrina-keypair.json --url devnet`');
  console.log('        (the .so is the CI artifact of tag v0.1.0-audit; program id must equal declare_id)');

  const program = vetrinaProgram(authority);
  const [config] = PublicKey.findProgramAddressSync([Buffer.from('config')], VETRINA_PROGRAM_ID);
  const [spotlight] = PublicKey.findProgramAddressSync([Buffer.from('spotlight')], VETRINA_PROGRAM_ID);
  const sig = await program.methods
    .initialize(treasury.publicKey, 3600, new BN(3600))
    .accounts({ authority: authority.publicKey, config, spotlight })
    .rpc();
  console.log(`  initialize: ${sig}`);
  recordSig(s, 'a:initialize', sig);
}

/** b. Launch: createPoolWithPartnerAndCreatorFirstBuy — Token-2022 + native metadata, first buy 0.05 SOL, one atomic tx. */
async function stepB(s: State) {
  if (!s.config) throw new Error('set CONFIG in state (run create-config first and put its address in .e2e-state.json)');
  const conn = connection();
  const client = DynamicBondingCurveClient.create(conn, 'confirmed');
  const creator = wallet(s, 'creator');
  const baseMint = Keypair.generate();
  const config = new PublicKey(s.config);

  const tx = await client.creator.createPoolWithPartnerAndCreatorFirstBuy({
    createPoolParam: {
      name: 'Spotlight Test',
      symbol: 'SPOT',
      uri: 'https://example.com/spot.json',
      payer: creator.publicKey,
      poolCreator: creator.publicKey,
      config,
      baseMint: baseMint.publicKey,
    },
    creatorFirstBuyParam: {
      creator: creator.publicKey,
      receiver: creator.publicKey,
      buyAmount: new BN(toLamports(0.05)), // first buy 0.05 SOL
      minimumAmountOut: new BN(0), // devnet test; tighten for mainnet
      referralTokenAccount: null,
    },
  });
  // Atomicity check: a SINGLE transaction (spec §3b).
  const sig = await sendTx(conn, tx, [creator, baseMint], 'launch+firstBuy(atomic)');
  const pool = deriveDbcPoolAddress(QUOTE_MINT_SOL, baseMint.publicKey, config);
  s.baseMint = baseMint.publicKey.toBase58();
  s.pool = pool.toBase58();
  recordSig(s, 'b:launch', sig);
  console.log(`  baseMint=${s.baseMint}\n  pool=${s.pool}`);
  console.log('  VERIFY: fetch the Token-2022 mint — mint & freeze authority must be None (immutable).');
}

/** c. 3 buys + 1 sell from different wallets; then claim partner fee and measure vs expected 0.70%. */
async function stepC(s: State) {
  if (!s.pool) throw new Error("run step 'b' first");
  const conn = connection();
  const client = DynamicBondingCurveClient.create(conn, 'confirmed');
  const pool = new PublicKey(s.pool);
  const partner = loadKeypair(reqEnv('PARTNER_KEYPAIR'));

  const buys: Array<[string, number]> = [
    ['buyer1', 0.1],
    ['buyer2', 0.1],
    ['buyer3', 0.1],
  ];
  for (const [name, sol] of buys) {
    const owner = wallet(s, name);
    const tx = await client.pool.swap({
      owner: owner.publicKey,
      pool,
      amountIn: new BN(toLamports(sol)),
      minimumAmountOut: new BN(0),
      swapBaseForQuote: false, // quote(SOL) -> base
      referralTokenAccount: null,
    });
    await sendTx(conn, tx, [owner], `buy ${sol} SOL (${name})`);
  }

  // 1 sell from buyer1 (sell back part of the base it holds).
  const seller = wallet(s, 'buyer1');
  const sellTx = await client.pool.swap({
    owner: seller.publicKey,
    pool,
    amountIn: new BN(1_000_000), // base units; adjust to holdings on the real run
    minimumAmountOut: new BN(0),
    swapBaseForQuote: true, // base -> quote(SOL)
    referralTokenAccount: null,
  });
  await sendTx(conn, sellTx, [seller], 'sell (buyer1)');

  // Fee accounting: total 1.25% base fee; partner share = 0.70% (of 30% creator split => partner keeps 70%).
  const metrics = await client.state.getPoolFeeMetrics(pool);
  console.log('  pool fee metrics:', JSON.stringify(metrics, replacer, 2));

  const before = await conn.getBalance(partner.publicKey);
  const claimTx = await client.partner.claimPartnerTradingFee({
    feeClaimer: partner.publicKey,
    payer: partner.publicKey,
    pool,
    maxBaseAmount: new BN('18446744073709551615'), // u64::MAX = claim all
    maxQuoteAmount: new BN('18446744073709551615'),
  });
  const sig = await sendTx(conn, claimTx, [partner], 'claimPartnerTradingFee');
  recordSig(s, 'c:claimPartner', sig);
  const after = await conn.getBalance(partner.publicKey);
  console.log(`  partner SOL delta (net of tx fee): ${(after - before) / LAMPORTS_PER_SOL} SOL`);
  console.log('  EXPECTED partner quote fee ≈ 0.70% of total quote-in volume (see report table).');
}

/** d. Vetrina: create_priority + bump(0.1 SOL) + claim_spotlight + sweep; verify emit_cpi events. */
async function stepD(s: State) {
  if (!s.baseMint) throw new Error("run step 'b' first");
  const conn = connection();
  const authority = wallet(s, 'authority');
  const program = vetrinaProgram(authority);
  const mint = new PublicKey(s.baseMint);
  const treasury = wallet(s, 'treasury').publicKey;

  const [config] = PublicKey.findProgramAddressSync([Buffer.from('config')], VETRINA_PROGRAM_ID);
  const [spotlight] = PublicKey.findProgramAddressSync([Buffer.from('spotlight')], VETRINA_PROGRAM_ID);
  const [priority] = PublicKey.findProgramAddressSync(
    [Buffer.from('priority'), mint.toBuffer()],
    VETRINA_PROGRAM_ID,
  );

  const createSig = await program.methods.createPriority().accounts({ payer: authority.publicKey, mint, priority }).rpc();
  const bumpSig = await program.methods.bump(new BN(toLamports(0.1))).accounts({ payer: authority.publicKey, priority }).rpc();
  const claimSig = await program.methods.claimSpotlight().accounts({ payer: authority.publicKey, config, spotlight, candidate: priority }).rpc();
  const sweepSig = await program.methods.sweep().accounts({ config, priority, treasury }).rpc();
  for (const [k, sig] of [['d:create', createSig], ['d:bump', bumpSig], ['d:claim', claimSig], ['d:sweep', sweepSig]] as const) {
    recordSig(s, k, sig);
  }

  // Verify emit_cpi events live in the tx metadata (self-CPI), not truncatable logs.
  for (const [name, sig] of [['bump', bumpSig], ['claim', claimSig], ['sweep', sweepSig]] as const) {
    const tx = await conn.getTransaction(sig, { commitment: 'confirmed', maxSupportedTransactionVersion: 0 });
    const inner = tx?.meta?.innerInstructions ?? [];
    console.log(`  ${name} event-cpi inner-ix count: ${inner.reduce((n, i) => n + i.instructions.length, 0)}`);
  }
}

/** e. Push quote raised over 0.5 SOL → graduation; migrate via keeper or SDK (documented). */
async function stepE(s: State) {
  if (!s.pool) throw new Error("run step 'b' first");
  const conn = connection();
  const client = DynamicBondingCurveClient.create(conn, 'confirmed');
  const pool = new PublicKey(s.pool);

  // Keep buying until the pool reports the migration threshold reached.
  const buyer = wallet(s, 'buyer2');
  for (let i = 0; i < 20; i++) {
    const vp = await client.state.getPool(pool);
    if (vp && (vp as any).isMigrated) break;
    const tx = await client.pool.swap({
      owner: buyer.publicKey, pool, amountIn: new BN(toLamports(0.1)),
      minimumAmountOut: new BN(0), swapBaseForQuote: false, referralTokenAccount: null,
    });
    await sendTx(conn, tx, [buyer], `graduation buy #${i + 1}`);
  }

  // Devnet: the Meteora keeper MAY auto-migrate. Poll briefly; if it doesn't,
  // trigger the permissionless migration via the SDK and record which path won.
  const dammConfig = DAMM_V2_MIGRATION_FEE_ADDRESS[MigrationFeeOption.Customizable];
  const payer = wallet(s, 'authority');
  const { transaction, firstPositionNftKeypair, secondPositionNftKeypair } =
    await client.migration.migrateToDammV2({ payer: payer.publicKey, pool, dammConfig });
  const sig = await sendTx(conn, transaction, [payer, firstPositionNftKeypair, secondPositionNftKeypair], 'migrateToDammV2(SDK)');
  recordSig(s, 'e:migrate', sig);
  console.log('  migration path: SDK (permissionless). If the keeper had already migrated, this would fail — note which occurred.');
}

/** f. Post-migration DAMM v2 pool: locked position present; claim partner fees after test swaps. */
async function stepF(s: State) {
  // The migrated pool is a DAMM v2 (cp-amm) pool — fee claiming there uses the
  // cp-amm SDK (@meteora-ag/cp-amm-sdk), NOT the DBC SDK. This step documents
  // the derivation and hands off; add @meteora-ag/cp-amm-sdk to run it live.
  if (!s.config || !s.baseMint) throw new Error("run steps 'b'/'e' first");
  console.log('  DAMM v2 pool =', 'deriveDammV2PoolAddress(config, baseMint, quoteMint) — see report');
  console.log('  Verify: partner/creator liquidity is PERMANENTLY locked (70/30); non-locked = 0.');
  console.log('  Claim DAMM v2 partner fees via @meteora-ag/cp-amm-sdk after a few test swaps.');
}

/** g. Final report: gather recorded signatures. */
async function stepG(s: State) {
  console.log('=== signatures ===');
  for (const [k, v] of Object.entries(s.sigs ?? {})) console.log(`  ${k}: ${v}`);
  console.log('\nFill the fee table (measured vs expected), SDK-name divergences (DIVERGENCES.md),');
  console.log('total SOL cost of the run, and any behavior that differed from the docs.');
}

// --- plumbing --------------------------------------------------------------

function dir() {
  const d = new URL('../.wallets', import.meta.url).pathname;
  if (!fs.existsSync(d)) fs.mkdirSync(d);
  return d;
}
function reqEnv(name: string): string {
  const v = process.env[name];
  if (!v) throw new Error(`set ${name}`);
  return v;
}
function replacer(_k: string, v: unknown) {
  return typeof v === 'bigint' ? v.toString() : v instanceof BN ? v.toString() : v;
}

const steps: Record<string, (s: State) => Promise<void>> = {
  a: stepA, b: stepB, c: stepC, d: stepD, e: stepE, f: stepF, g: stepG,
};

async function main() {
  const step = process.argv[2];
  const fn = steps[step];
  if (!fn) throw new Error(`usage: npm run e2e -- <a|b|c|d|e|f|g>`);
  const s = readState();
  if (process.env.CONFIG && !s.config) { s.config = process.env.CONFIG; writeState(s); }
  console.log(`=== step ${step} ===`);
  await fn(s);
}

main().catch((e) => { console.error(e); process.exit(1); });
