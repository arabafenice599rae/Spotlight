# Spotlight client — DBC config + devnet end-to-end

TypeScript client for the Meteora **Dynamic Bonding Curve** (DBC) config and the
devnet end-to-end that exercises launch → trade → vetrina → graduation →
migration. Separate `package.json`; the vetrina program is unchanged.

```bash
cd client && npm install
npm run typecheck          # verifies every field name against the installed SDK
```

Pins: `@meteora-ag/dynamic-bonding-curve-sdk@1.5.11`, `@solana/web3.js`,
`@coral-xyz/anchor` (loads the vetrina IDL from `../target/idl/vetrina.json`).

## Config (Soluzione A)

`src/config.ts` is the single source of truth (both presets). Every place the
SDK's name/shape differs from the spec is in **`DIVERGENCES.md`** — read it: it
contains real conflicts (esp. migration fee option vs the market-cap scheduler)
and required-but-unspecified params (all flagged `PROVISIONAL`).

```bash
# devnet: create the config key with the test partner wallet, print its address
PARTNER_KEYPAIR=/path/partner.json npm run create-config -- devnet
# mainnet preset is FILE-ONLY (85 SOL threshold); the script refuses to run it
```

## End-to-end (runnable a step at a time)

```bash
PARTNER_KEYPAIR=/path/partner.json CONFIG=<configAddr> npm run e2e -- a   # deploy note + initialize + wallets
npm run e2e -- b   # launch Token-2022 + first buy 0.05 SOL (atomic)
npm run e2e -- c   # 3 buys + 1 sell; claim partner fee; measure vs 0.70%
npm run e2e -- d   # vetrina create_priority + bump 0.1 + claim + sweep; check emit_cpi
npm run e2e -- e   # buy to >0.5 SOL → graduation; migrate (keeper or SDK)
npm run e2e -- f   # post-migration DAMM v2 (locked position; fee claim via cp-amm SDK)
npm run e2e -- g   # print collected signatures for the report
```

State (config/mint/pool/wallets/sigs) is shared via `.e2e-state.json`; wallets
are dedicated throwaway devnet keypairs in `.wallets/` (git-ignored). **No real
keys.** Errors abort the step with full logs — no silent retries.

### Deploying the audited program (step a)

The `.so` is the CI artifact of tag `v0.1.0-audit`. Deploy it at its
`declare_id` (`gCxZar2t…`):

```bash
solana program deploy target/deploy/vetrina.so \
  --program-id target/deploy/vetrina-keypair.json --url devnet
```

## ⚠️ This environment cannot execute any of the on-chain steps

All Solana RPC endpoints (devnet, mainnet, third-party) are **unreachable from
this session** (egress policy — HTTP 000), and `solana-verify`/platform-tools
hosts are likewise blocked. So `create-config` and `e2e a–g` are written and
**type-checked** here but must be **run where devnet is reachable**. The report
(§3g: tx signatures, measured-vs-expected fee table, SOL cost, doc deviations)
is produced from that run.
