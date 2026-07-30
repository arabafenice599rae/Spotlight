# SDK name/shape divergences — Soluzione A vs `@meteora-ag/dynamic-bonding-curve-sdk@1.5.11`

The **values** are the spec's; the **names/shapes** are the SDK's (verified by
`tsc` against the installed 1.5.11 `.d.ts`). Source of truth: `src/config.ts`.

Legend: ✅ name matches · ✏️ name/shape differs · ⚠️ conflict or missing in spec.

| # | Spec wording (value) | SDK path & type | | Note |
|---|---|---|---|---|
| 1 | collectFeeMode = quote (SOL) | `fee.collectFeeMode = CollectFeeMode.QuoteToken` | ✅ | value is an enum member |
| 2 | base fee 125 bps fissi, no scheduler | `fee.baseFeeParams = { baseFeeMode: BaseFeeMode.FeeSchedulerLinear, feeSchedulerParam: { startingFeeBps:125, endingFeeBps:125, numberOfPeriod:0, totalDuration:0 } }` | ✏️ | no "fixed" mode exists; a flat fee = linear scheduler with start==end, 0 periods |
| 3 | no dynamic fee | `fee.dynamicFeeEnabled = false` | ✅ | |
| 4 | creatorTradingFeePercentage = 30 | `fee.creatorTradingFeePercentage = 30` | ✅ | exact |
| 5 | tokenType = Token-2022 | `token.tokenType = TokenType.Token2022` | ✅ | |
| 6 | tokenUpdateAuthority = immutable | `token.tokenAuthorityOption = TokenAuthorityOption.Immutable` | ✏️ | field is **tokenAuthorityOption** (not tokenUpdateAuthority) |
| 7 | supply fissa 1B | `token.totalTokenSupply = 1_000_000_000` | ✏️ | field **totalTokenSupply**; UI units (SDK scales by decimals) |
| 8 | decimali base 6 / quote 9 | `token.tokenBaseDecimal = TokenDecimal.SIX`, `token.tokenQuoteDecimal = TokenDecimal.NINE` | ✏️ | fields **tokenBaseDecimal/tokenQuoteDecimal**, enum `TokenDecimal` |
| 9 | migrationOption = DAMM v2 | `migration.migrationOption = MigrationOption.MET_DAMM_V2` | ✅ | enum member `MET_DAMM_V2` |
| 10 | migration fee = FixedBps25 (opzione minima) | `migration.migrationFeeOption` → **Customizable** (see #12); 25 bps carried by `migratedPoolFee.poolFeeBps = 25` | ⚠️ | `MigrationFeeOption.FixedBps25` exists, **but** a custom market-cap scheduler (#12) is only honored when `migrationFeeOption = Customizable`. We keep the 25 bps minimum via `poolFeeBps=25`. If you insist on the `FixedBps25` **enum**, you must drop the market-cap scheduler. |
| 11 | LP 100% locked: partnerLockedLp 70 / creatorLockedLp 30; partnerLp = creatorLp = 0 | `liquidityDistribution.partnerPermanentLockedLiquidityPercentage = 70`, `creatorPermanentLockedLiquidityPercentage = 30`, `partnerLiquidityPercentage = 0`, `creatorLiquidityPercentage = 0` | ✏️ | names differ; 70+30+0+0 = 100 (validated by SDK) |
| 12 | migratedPoolBaseFeeMode = scheduler market-cap lineare (provvisori) | `migration.migratedPoolFee = { collectFeeMode, dynamicFee, poolFeeBps:25, baseFeeMode: DammV2BaseFeeMode.FeeMarketCapSchedulerLinear, marketCapFeeSchedulerParams:{…} }` | ✏️⚠️ | lives under **migratedPoolFee** (`MigratedPoolFeeConfig`); requires `migrationFeeOption = Customizable` (#10). `marketCapFeeSchedulerParams` are provisional (`endingBaseFeeBps/numberOfPeriod/priceMultiple/schedulerExpirationDuration`). |
| 13 | migrationQuoteThreshold = 0.5 / 85 | `migrationQuoteThreshold` (top-level `BuildCurveParams`) | ✅ | SOL UI units as `number` |
| 14 | curva segmento singolo prodotto costante | `buildCurve(params)` (not `buildCurveWithTwoSegments`) | ✅ | single-segment constant product |
| 15 | — (not in spec) | `percentageSupplyOnMigration` (REQUIRED) | ⚠️ | required by `buildCurve`; **provisional 20** — pick the real split |
| 16 | — | `migration.migrationFee = { feePercentage, creatorFeePercentage }` (REQUIRED) | ⚠️ | one-time migration skim; **provisional 0 / 0** |
| 17 | — | `token.leftover` (REQUIRED) | ⚠️ | **provisional 0** |
| 18 | — | `fee.poolCreationFee` (REQUIRED) | ⚠️ | **provisional 0** |
| 19 | — | `fee.enableFirstSwapWithMinFee` (REQUIRED) | ⚠️ | **provisional false** (first buy pays 125 bps) |
| 20 | — | `lockedVesting` (REQUIRED) | ⚠️ | no vesting in spec → **all zero** |
| 21 | — | `activationType` (REQUIRED) | ⚠️ | **provisional `Timestamp`** |
| 22 | partner wallet | `CreateConfigParams.feeClaimer` | ✏️ | the partner = fee claimer |
| 23 | — | `CreateConfigParams.leftoverReceiver` | ⚠️ | **provisional = partner** |
| 24 | quote = SOL | `CreateConfigParams.quoteMint = So1111…1112` (wSOL) | ✏️ | |
| 25 | — | `CreateConfigParams.config` (new address) | ✏️ | a fresh `Keypair`, co-signs `createConfig` |

## Method-name map (for §3)

| Spec step | SDK call |
|---|---|
| create config | `client.partner.createConfig(CreateConfigParams)` |
| launch + first buy (atomic) | `client.creator.createPoolWithPartnerAndCreatorFirstBuy({ createPoolParam, creatorFirstBuyParam })` → one `Transaction` |
| buy / sell | `client.pool.swap({ owner, pool, amountIn, minimumAmountOut, swapBaseForQuote, referralTokenAccount:null })` |
| claim partner fee | `client.partner.claimPartnerTradingFee({ feeClaimer, payer, pool, maxBaseAmount, maxQuoteAmount })` |
| pool address | `deriveDbcPoolAddress(quoteMint, baseMint, config)` |
| fee metrics | `client.state.getPoolFeeMetrics(pool)` |
| migrate (permissionless) | `client.migration.migrateToDammV2({ payer, pool, dammConfig: DAMM_V2_MIGRATION_FEE_ADDRESS[MigrationFeeOption.Customizable] })` → `{ transaction, firstPositionNftKeypair, secondPositionNftKeypair }` |
| migrated (DAMM v2) pool | `deriveDammV2PoolAddress(config, baseMint, quoteMint)` |

## Open decisions for you (blocking a mainnet run, not a devnet smoke test)

- **#10/#12**: confirm Customizable + `poolFeeBps=25` + market-cap scheduler is the intent (vs strict `FixedBps25`, no scheduler).
- **#15**: real `percentageSupplyOnMigration`.
- **#12**: real `marketCapFeeSchedulerParams` (taratura).
- **#16**: whether any one-time migration fee is wanted.
