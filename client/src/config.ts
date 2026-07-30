/**
 * DBC config — "Soluzione A" values, expressed against the REAL types of
 * @meteora-ag/dynamic-bonding-curve-sdk 1.5.11.
 *
 * The VALUES are fixed by the spec; the NAMES are dictated by the SDK. Every
 * place where the SDK name/shape differs from the spec's wording is called out
 * in client/DIVERGENCES.md and flagged inline with `DIVERGENCE:` / `PROVISIONAL:`.
 */
import { PublicKey } from '@solana/web3.js';
import {
  buildCurve,
  ActivationType,
  BaseFeeMode,
  CollectFeeMode,
  DammV2BaseFeeMode,
  DammV2DynamicFeeMode,
  MigratedCollectFeeMode,
  MigrationFeeOption,
  MigrationOption,
  TokenAuthorityOption,
  TokenDecimal,
  TokenType,
  type BuildCurveParams,
  type ConfigParameters,
} from '@meteora-ag/dynamic-bonding-curve-sdk';

/** Wrapped-SOL mint = the quote mint (collectFeeMode = quote → fees accrue in SOL). */
export const QUOTE_MINT_SOL = new PublicKey('So11111111111111111111111111111111111111112');

export type Preset = 'devnet' | 'mainnet';

/** migrationQuoteThreshold in SOL (UI units; buildCurve converts internally). */
const MIGRATION_QUOTE_THRESHOLD_SOL: Record<Preset, number> = {
  devnet: 0.5, // graduation reachable in the test run
  mainnet: 85, // file-only; do NOT execute (spec §2)
};

/**
 * Base fee: 125 bps FIXED, no scheduling. The SDK has no "fixed" mode — a flat
 * fee is a linear scheduler with start == end and zero periods.
 * DIVERGENCE: spec "base fee 125 bps fissi, no scheduler" → FeeSchedulerLinear flat.
 */
const FLAT_BASE_FEE_125_BPS = {
  baseFeeMode: BaseFeeMode.FeeSchedulerLinear as const,
  feeSchedulerParam: {
    startingFeeBps: 125,
    endingFeeBps: 125,
    numberOfPeriod: 0,
    totalDuration: 0,
  },
};

/**
 * Build the fixed BuildCurveParams for a preset. Only migrationQuoteThreshold
 * differs between devnet and mainnet.
 */
export function buildCurveParams(preset: Preset): BuildCurveParams {
  return {
    // ---- token (TokenConfig) ------------------------------------------------
    token: {
      tokenType: TokenType.Token2022, // spec: Token-2022
      tokenBaseDecimal: TokenDecimal.SIX, // spec: base 6   (DIVERGENCE: name tokenBaseDecimal)
      tokenQuoteDecimal: TokenDecimal.NINE, // spec: quote 9 (DIVERGENCE: name tokenQuoteDecimal)
      // spec "tokenUpdateAuthority = immutable" → DIVERGENCE: field is tokenAuthorityOption.
      tokenAuthorityOption: TokenAuthorityOption.Immutable,
      totalTokenSupply: 1_000_000_000, // spec: supply fissa 1B (DIVERGENCE: name totalTokenSupply)
      leftover: 0, // PROVISIONAL: not in spec; 0 = no leftover carve-out
    },

    // ---- fee (FeeConfig) ----------------------------------------------------
    fee: {
      baseFeeParams: FLAT_BASE_FEE_125_BPS, // 125 bps fixed
      dynamicFeeEnabled: false, // spec: no dynamic fee
      collectFeeMode: CollectFeeMode.QuoteToken, // spec: quote (SOL)
      creatorTradingFeePercentage: 30, // spec: 30 (exact name match)
      poolCreationFee: 0, // PROVISIONAL: not in spec
      enableFirstSwapWithMinFee: false, // PROVISIONAL: first buy pays the 125 bps like any swap
    },

    // ---- migration (MigrationConfig) ---------------------------------------
    migration: {
      migrationOption: MigrationOption.MET_DAMM_V2, // spec: DAMM v2
      // ⚠ CONFLICT (see DIVERGENCES.md): the spec asks for BOTH migration fee
      // "FixedBps25" AND a market-cap-linear scheduler on the migrated pool.
      // The SDK only honors a custom `migratedPoolFee` when migrationFeeOption
      // is Customizable. We keep the 25 bps *minimum* via poolFeeBps = 25 and
      // set the option to Customizable so the scheduler actually applies.
      migrationFeeOption: MigrationFeeOption.Customizable,
      // One-time migration fee skim (% of migrated quote). PROVISIONAL: 0/0.
      migrationFee: { feePercentage: 0, creatorFeePercentage: 0 },
      // spec "migratedPoolBaseFeeMode = scheduler market-cap lineare".
      // DIVERGENCE: this lives under migratedPoolFee (MigratedPoolFeeConfig).
      migratedPoolFee: {
        collectFeeMode: MigratedCollectFeeMode.QuoteToken,
        dynamicFee: DammV2DynamicFeeMode.Disabled,
        poolFeeBps: 25, // "opzione minima"
        baseFeeMode: DammV2BaseFeeMode.FeeMarketCapSchedulerLinear,
        // PROVISIONAL params — taratura dopo (spec).
        marketCapFeeSchedulerParams: {
          endingBaseFeeBps: 25,
          numberOfPeriod: 10,
          priceMultiple: 2,
          schedulerExpirationDuration: 86_400,
        },
      },
    },

    // ---- liquidity distribution (LiquidityDistributionConfig) --------------
    // spec: 100% locked permanent, partner 70 / creator 30, non-locked = 0.
    // DIVERGENCE: names are partner/creatorPermanentLockedLiquidityPercentage
    // and partner/creatorLiquidityPercentage.
    liquidityDistribution: {
      partnerPermanentLockedLiquidityPercentage: 70,
      partnerLiquidityPercentage: 0,
      creatorPermanentLockedLiquidityPercentage: 30,
      creatorLiquidityPercentage: 0,
    },

    // ---- locked vesting (LockedVestingParams) ------------------------------
    // PROVISIONAL: spec has no team/creator token vesting → all zero.
    lockedVesting: {
      totalLockedVestingAmount: 0,
      numberOfVestingPeriod: 0,
      cliffUnlockAmount: 0,
      totalVestingDuration: 0,
      cliffDurationFromMigrationTime: 0,
    },

    activationType: ActivationType.Timestamp, // PROVISIONAL: time-based activation

    // ---- top-level BuildCurveParams ----------------------------------------
    // PROVISIONAL: percentageSupplyOnMigration is REQUIRED by buildCurve but is
    // NOT in the spec. 20% sold-on-curve / rest migrated is a placeholder.
    percentageSupplyOnMigration: 20,
    migrationQuoteThreshold: MIGRATION_QUOTE_THRESHOLD_SOL[preset],
  };
}

/** Single-segment constant-product curve → ConfigParameters (spec §2 "comune"). */
export function configParameters(preset: Preset): ConfigParameters {
  return buildCurve(buildCurveParams(preset));
}
