# Architecture

Spotlight is a Solana memecoin launchpad. It has two on-chain halves:

1. **Bonding curve, graduation, migration** — delegated to **Meteora Dynamic Bonding Curve (DBC)**,
   an external, audited program. Spotlight ships *no* code for this; it only supplies a **config**
   (see [`client/`](../client) and [`client/DIVERGENCES.md`](../client/DIVERGENCES.md)).
2. **The vetrina auction** — the only bespoke on-chain program, in [`programs/vetrina`](../programs/vetrina).

This document covers (2).

## The vetrina program

A single **Spotlight** slot that projects compete for by spending a *consumable score*. It is an
**all-pay auction with linear decay**: everyone who bids pays, and the entry threshold decays over
time so the slot becomes contestable again.

### Accounts

| Account | Seeds | Role |
|---------|-------|------|
| `Config` | `["config"]` | Singleton. `authority`, `treasury`, `decay`, `lease_duration`. Params clamped to compile-time caps. |
| `Spotlight` | `["spotlight"]` | Singleton. Current holder `mint`, `paid_snapshot`, absolute `lease_end`. |
| `Priority` | `["priority", mint]` | Per-candidate. `paid` (monotone), `effective` (consumable), `swept`. |

### Instructions

| Instruction | Access | Effect |
|-------------|--------|--------|
| `initialize(treasury, decay, lease_duration)` | authority | `init`s `Config` + `Spotlight` (pure `init`, once). |
| `update_config(treasury, decay, lease_duration)` | authority (`has_one`) | Re-sets params, still within the caps. |
| `create_priority` | anyone | `init`s the per-mint `Priority` (never `init_if_needed`). |
| `bump(lamports)` | signer pays | Deposits SOL into the PDA; `paid += x` **and** `effective += x`. |
| `claim_spotlight` | **permissionless** | Requires `effective > bar(now)`; consumes the bar from `effective`; snapshots the remainder. |
| `sweep` | **permissionless** | Moves `paid - swept` to `config.treasury`; leaves the PDA rent-exempt. |

### The bar

```
bar(paid_snapshot, lease_end, now, decay):
    if now < lease_end:            return paid_snapshot          # full during the lease
    elapsed = now - lease_end
    if elapsed >= decay:           return 0                      # fully decayed
    return paid_snapshot * (decay - elapsed) / decay             # linear ramp
```

Properties (proptested, P1–P4):

- **P1** full while `now < lease_end`
- **P2** zero once `now >= lease_end + decay`
- **P3** monotone non-increasing in `now`
- **P4** never above `paid_snapshot`

At `now == lease_end` the bar is still **full** (`elapsed == 0`) — intended.

### Safety invariants

| # | Invariant |
|---|-----------|
| I1 | `Priority.paid` is monotone — never decremented. |
| I2 | `claim` requires `effective > bar(now)`. |
| I3 | A candidate cannot claim the slot it already holds. |
| I4 | `paid_snapshot` = the candidate's `effective` **after** the bar is consumed. |
| I5 | `lease_end` is absolute and written **only** in `claim`. |
| I7 | `lamports(Priority) >= rent_min + (paid - swept)`, checked **before** the sweep transfer. |
| I8 | `swept <= paid`. |
| I17 | `effective <= paid` always — they grow together in `bump`; only `effective` descends, in `claim`. |

All arithmetic is `checked_*` and `overflow-checks = true` is on. Account validation is declarative
(seeds / bump / `has_one` / `address =`). Events are emitted via `emit_cpi!` (transaction metadata,
not truncatable logs): `BumpEvent`, `SweepEvent`, `ClaimEvent`.

## Data flow (launch → spotlight → graduation)

```
        ┌─────────────┐   createPoolWithPartnerAndCreatorFirstBuy   ┌──────────────┐
        │  DBC config  │ ──────────────────────────────────────────▶│ Token-2022   │
        │ (Soluzione A)│                                             │ + DBC pool   │
        └─────────────┘                                             └──────┬───────┘
                                                                           │ buys/sells (1.25% fee)
                                       vetrina                             ▼
   create_priority ─▶ bump(SOL) ─▶ claim_spotlight ─▶ sweep      migrationQuoteThreshold reached
        (Priority PDA grows)      (bar consumed)   (→ treasury)          │
                                                                          ▼
                                                              graduation → DAMM v2 (100% locked LP)
```

The vetrina auction runs alongside DBC trading: a project `bump`s its `Priority` and `claim`s the
Spotlight to signal attention, entirely independent of its bonding-curve progress. See
[`client/README.md`](../client/README.md) for the end-to-end devnet flow.
