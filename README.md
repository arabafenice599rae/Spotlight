<div align="center">

# ✦ Spotlight

**A memecoin launchpad on Solana with an on-chain, all-pay attention auction.**

[![CI](https://github.com/arabafenice599rae/Spotlight/actions/workflows/ci.yml/badge.svg)](https://github.com/arabafenice599rae/Spotlight/actions/workflows/ci.yml)
[![Anchor](https://img.shields.io/badge/Anchor-1.0.2-512BD4)](https://www.anchor-lang.com/)
[![Solana](https://img.shields.io/badge/Solana-SBF-14F195)](https://solana.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

</div>

---

Spotlight pairs a battle-tested bonding curve with a novel **"vetrina"** (showcase) slot: projects
compete for a single, decaying spotlight by spending a *consumable score*. Bonding-curve trading,
graduation and migration are delegated to **Meteora Dynamic Bonding Curve (DBC)**; the only bespoke
on-chain code here is the `vetrina` program — the all-pay auction with linear decay.

## What's in the box

| Layer | Path | Description |
|-------|------|-------------|
| 🦀 **On-chain program** | [`programs/vetrina`](./programs/vetrina) | The `vetrina` Anchor program — all-pay auction, linear decay, consumable score. Invariants I1–I8/I17. |
| 🧪 **Property tests** | [`programs/vetrina/tests`](./programs/vetrina/tests) | `proptest` for the decay curve (P1–P4) + edge cases. |
| ⚙️ **litesvm suite** | [`tests-litesvm`](./tests-litesvm) | Integration tests (scenarios a–l) that run against the compiled `.so`. |
| 🟦 **Client** | [`client`](./client) | TypeScript: Meteora DBC config (Soluzione A) + devnet end-to-end. |
| 📖 **Docs** | [`docs`](./docs) | Architecture, invariants, and design notes. |

> **Naming.** *Spotlight* is the launchpad; **`vetrina`** ("shop window" in Italian) is the on-chain
> program that implements the showcase auction. See [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Quickstart

```bash
# Prerequisites: Rust, the Solana tool suite, and Anchor 1.0.2
cargo install anchor-cli --version 1.0.2 --locked
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"

make build          # anchor build  -> target/deploy/vetrina.so
make test           # property tests (P1–P4) + litesvm scenarios a–l
make client         # install + typecheck the TypeScript client
```

No `make`? Every target maps to a plain command — see the [`Makefile`](./Makefile) or
[`CONTRIBUTING.md`](./CONTRIBUTING.md).

## The vetrina mechanic in one minute

- **`initialize`** — set up the singleton `Config` (treasury, decay, lease) and `Spotlight`.
- **`create_priority(mint)`** — one PDA per candidate token (`init`, once).
- **`bump(lamports)`** — deposit SOL; grows `paid` **and** the consumable `effective` together.
- **`claim_spotlight`** *(permissionless)* — take the slot if `effective > bar(now)`; the bar is
  **consumed** from `effective`, and the remainder is snapshotted.
- **`sweep`** *(permissionless)* — move the un-swept backing to the configured treasury.

`bar(now)` is a linearly-decaying threshold: full during the lease, zero after full decay, monotone
non-increasing, never above the snapshot. Full details and the safety invariants are in
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Reproducible builds

The on-chain program is built for verifiable, byte-reproducible output. See
[`SECURITY.md`](./SECURITY.md) for the `solana-verify` command and the audit-freeze tag.

## Contributing

Issues and PRs welcome — start with [`CONTRIBUTING.md`](./CONTRIBUTING.md). All changes run through
CI: `anchor build`, the property tests, and the litesvm suite against the freshly built `.so`.

## License

[MIT](./LICENSE).
