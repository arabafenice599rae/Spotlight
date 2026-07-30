# Spotlight — Vetrina

On-chain "vetrina" (showcase) mechanic for a Solana memecoin launchpad: an
**all-pay auction with linear decay and a consumable score**.

The bonding curve, graduation and migration are delegated to **Meteora Dynamic
Bonding Curve** (external program, no code here). This repository contains
**only** the vetrina mechanic — the sole proprietary on-chain piece.

## Layout

```
programs/vetrina/          Anchor program (anchor-lang 1.0.2, feature event-cpi)
  src/lib.rs               instructions + bar() + invariants I1–I8, I17
  tests/properties.rs      proptests P1–P4 + edge cases (pure, no artifact needed)
tests-litesvm/             litesvm integration suite (detached workspace, see below)
Anchor.toml                anchor_version = 1.0.2
.github/workflows/ci.yml   build .so, run both suites against it, report CU

```

## Instructions

| ix | permission | notes |
|----|------------|-------|
| `initialize(treasury, decay, lease_duration)` | authority | `init`s `Config` (holds `treasury`/`decay`/`lease_duration`, capped) **and** `Spotlight` (pure `init`) |
| `update_config(treasury, decay, lease_duration)` | authority (`has_one`) | re-set params inside the compile-time caps; out-of-range → `ParamOutOfBounds` |
| `create_priority` | `init` (never `init_if_needed`) | one per mint; validates the mint via `InterfaceAccount<Mint>` |
| `bump(lamports)` | signer pays | deposits lamports, grows `paid` **and** `effective` together (I17); `0` → `ZeroAmount` |
| `claim_spotlight` | **permissionless** | needs `effective > bar(now)` (I2); consumes the bar out of `effective`, snapshots the remainder (I4) |
| `sweep` | **permissionless** | moves `paid - swept` to `config.treasury` (`address =`); checks `lamports >= rent_min + owed` first (I7) |

Parameter caps (compile-time, `update_config` cannot escape them): `decay`
∈ [60 s, 30 d], `lease_duration` ∈ [60 s, 30 d].

`bar(paid_snapshot, lease_end, now, decay)` — the decaying threshold: full while
`now < lease_end` (P1), zero once `now >= lease_end + decay` (P2), monotone
non-increasing (P3), never above `paid_snapshot` (P4). At `now == lease_end` it
enters the decay branch with `elapsed == 0`, i.e. still **full**.

## Build

Requires the Solana platform tools (for `cargo build-sbf`) and the Anchor CLI
`1.0.2`:

```bash
# Anchor CLI (from crates.io — no platform tools needed for `anchor keys sync`)
cargo install anchor-cli --version 1.0.2 --locked

# Solana tool suite (provides cargo-build-sbf + solana-verify)
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"

anchor build            # -> target/deploy/vetrina.so + target/deploy/vetrina-keypair.json
```

## Test

```bash
# Property tests P1–P4 + edge cases (pure, no on-chain artifact needed)
cargo test -p vetrina

# litesvm integration suite (loads target/deploy/vetrina.so — run `anchor build` first).
# Detached workspace, so its dependency tree cannot perturb the on-chain crate.
cd tests-litesvm && cargo test -- --nocapture
```

Every litesvm scenario starts from `initialize(treasury, decay, lease_duration)`.
Coverage: double `create_priority` (a); `bump 0` → `ZeroAmount` (b); `sweep`
with `owed == 0` → `NothingToSweep` (c); `sweep` after `bump` moves exactly
`owed`, leaves the PDA rent-exempt, `swept == paid` (d); claim below the bar →
`BelowBar` (e); claiming the current holder → `AlreadyHolder` (f); the
bump-A/bump-B/claim-B consumption sequence then claim-A-below-bar (g); first
claim `initialize → create_priority → bump(1) → claim` with bar 0 (h); the
substituted-candidate seeds guard (i); `update_config` out of cap →
`ParamOutOfBounds` (j); `update_config` from a non-authority → `has_one` (k);
`sweep` toward a non-`config.treasury` account → `address` constraint (l).
`z_report_compute_units` prints CU per instruction (task 6).

Without the `.so` the litesvm tests print a `SKIP` notice and pass.

## Program ID / keys sync

`anchor keys sync` has been run; `declare_id!` in `lib.rs` and `Anchor.toml`
carry the synced id:

```
gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4
```

The litesvm tests deploy the `.so` at this **declared** id (a `PROGRAM_ID`
constant kept equal to `declare_id!`), not at the `target/deploy` keypair — a
fresh `anchor build` mints a random keypair that need not match `declare_id`,
and the baked-in `declare_id` is what the program derives its PDAs from.

## Reproducible (verifiable) build — solana-verify

Pin the build container to the Solana toolchain paired with Anchor 1.0.2 (match
`solana --version` from your install; substitute below):

```bash
# one-off install
cargo install solana-verify

# verifiable build (Docker), pinned base image
solana-verify build \
  --library-name vetrina \
  --base-image solanafoundation/solana-verifiable-build:<solana-version>

# after deploy, verify the on-chain bytes match this source
solana-verify verify-from-repo \
  --program-id gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4 \
  https://github.com/arabafenice599rae/spotlight
```

`solana-verify build` runs `cargo build-sbf` inside the pinned Docker image, so
the on-chain hash is reproducible independent of the host toolchain.

## Crate versions (locked)

| crate | version |
|-------|---------|
| anchor-lang | **1.0.2** (feature `event-cpi`) |
| anchor-spl | **1.0.2** (`token`, `token_2022` — for `InterfaceAccount<Mint>`) |
| solana-* (via anchor) | 3.x modular crates |
| proptest | 1.x |
| litesvm | **0.15.1** |

`anchor-spl` 1.0.2 exists on crates.io, so `InterfaceAccount<Mint>` is kept (the
`AccountInfo` fallback from the original task 1 was unnecessary).

### One adaptation vs. the delivered `lib.rs`

The delivered file called `CpiContext::new(system_program.to_account_info(), …)`
in `bump` — an older-Anchor signature. On **anchor-lang 1.0.2** `CpiContext::new`
takes the program **`Pubkey`**, so that single argument is `system_program.key()`.
Same System Program, same CPI, no semantic change; it is the only line that
differs from the reference.

### litesvm dependency pinning

`litesvm 0.15.1` targets the `wincode 0.5` generation of the split `solana-*`
crates, but several shipped a later patch on `wincode 0.6`. The detached
`tests-litesvm/Cargo.lock` pins the last `wincode 0.5` patch of each affected
crate so the host build resolves cleanly:

```
solana-fee-calculator = 3.2.2   solana-signature      = 3.4.1
solana-rent           = 4.3.0   solana-epoch-schedule = 3.2.0
```

Keeping this suite in its own workspace is what makes those pins possible without
touching the on-chain crate's graph — commit `tests-litesvm/Cargo.lock`.

## What runs in a stock CI vs. this environment

`cargo test -p vetrina` (properties) and `anchor keys sync` run anywhere with
Rust + the Anchor CLI. `anchor build`, the litesvm suite's *execution*, and
`solana-verify build` additionally need the Solana platform tools / Docker image
from `release.anza.xyz`; where that host is blocked by egress policy, run those
steps in an environment that can reach it.
