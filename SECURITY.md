# Security

## Reporting a vulnerability

Please report suspected vulnerabilities in the `vetrina` program **privately** — open a GitHub
security advisory (Security → Report a vulnerability) rather than a public issue. Include a
reproduction and the affected commit. We aim to acknowledge within a few days.

Do **not** open public issues or PRs for exploitable findings before a fix is available.

## Scope

In scope: the on-chain `vetrina` program ([`programs/vetrina`](./programs/vetrina)) and its account
model / invariants (I1–I8, I17). Out of scope: Meteora Dynamic Bonding Curve (external, separately
audited) and the off-chain client scripts.

## Audit freeze

Audited builds are tagged `v0.1.0-audit`. The auditable artifact is the `vetrina.so` produced by CI
for that tag. To reproduce the exact bytes independently:

```bash
# Pin the base image to the Solana toolchain paired with Anchor 1.0.2
solana-verify build --library-name vetrina \
  --base-image solanafoundation/solana-verifiable-build:<solana-version>

# After deploy, verify on-chain bytes match this source
solana-verify verify-from-repo \
  --program-id gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4 \
  https://github.com/arabafenice599rae/Spotlight
```

## Hardening notes

- All arithmetic is `checked_*`; `overflow-checks = true` is enabled for release builds.
- Account validation is fully declarative (seeds / bump / `has_one` / `address =`).
- `create_priority` uses plain `init` (never `init_if_needed`); `claim_spotlight` and `sweep` are
  permissionless by design, with the treasury pinned via `address = config.treasury`.
- The `sweep` rent-exemption check (I7) runs **before** any lamport movement.
