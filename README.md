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
```

## Instructions

| ix | permission | notes |
|----|------------|-------|
| `create_priority` | `init` (never `init_if_needed`) | one per mint; validates the mint via `InterfaceAccount<Mint>` |
| `bump(amount)` | signer pays | deposits lamports, grows `paid` **and** `effective` together (I17); `amount == 0` → `ZeroAmount` |
| `claim_spotlight` | **permissionless** | needs `effective > bar(now)` (I2); consumes the bar out of `effective`, snapshots the remainder (I4) |
| `sweep` | **permissionless** | moves `paid - swept` to the fixed treasury; checks `lamports >= rent_min + owed` **before** moving (I7) |

`bar()` — the decaying entry bar: full during the lease (P1), zero after full
decay (P2), monotone non-increasing (P3), never above `paid_snapshot` (P4). At
`now == lease_end` exactly the bar is **full** (`elapsed == 0`), by design.

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

The litesvm suite covers: double `create_priority` (a), `bump 0` → `ZeroAmount`
(b), `sweep` with `owed == 0` → `NothingToSweep` (c), `sweep` after `bump` moves
the exact `owed` and leaves the PDA rent-exempt with `swept == paid` (d), claim
below the bar → `BelowBar` (e), claiming the current holder → `AlreadyHolder`
(f), the bump-A/bump-B/claim-B consumption sequence then claim-A-below-bar (g),
first-ever claim with `effective = 1` (h), and the substituted-candidate seeds
guard (i). `z_report_compute_units` prints CU per instruction (task 6).

Without the `.so` the litesvm tests print a `SKIP` notice and pass, so the crate
stays green before a build.

## Program ID / keys sync

`anchor keys sync` has been run; `declare_id!` in `lib.rs` and `Anchor.toml`
carry the synced id:

```
gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4
```

The litesvm tests read the program id from `target/deploy/vetrina-keypair.json`
at runtime, so they follow any future re-sync automatically.

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
| anchor-lang | **1.0.2** (features `event-cpi`, `init-if-needed`) |
| anchor-spl | **1.0.2** (`token`, `token_2022` — for `InterfaceAccount<Mint>`) |
| solana-* (via anchor) | 3.x modular crates |
| proptest | 1.x |
| litesvm | **0.15.1** |

`anchor-spl` 1.0.2 exists on crates.io, so `InterfaceAccount<Mint>` is kept (the
`AccountInfo` + owner-check fallback from task 1 was unnecessary).

### litesvm dependency pinning

`litesvm 0.15.1` targets the `wincode 0.5` generation of the split `solana-*`
crates, but several of them shipped a later patch on `wincode 0.6`. The detached
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
