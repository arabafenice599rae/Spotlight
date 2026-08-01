# Contributing to Spotlight

Thanks for helping out! This repo is a small monorepo — an on-chain Anchor program plus a TypeScript
client. Here's how to get productive quickly.

## Prerequisites

- **Rust** (stable) and the **Solana tool suite** (`cargo build-sbf` comes from it)
- **Anchor 1.0.2**: `cargo install anchor-cli --version 1.0.2 --locked`
- **Node 20+** for the client

## Layout

```
programs/vetrina/    on-chain program + property tests (P1–P4)
tests-litesvm/        integration suite against the compiled .so (detached workspace)
client/               TypeScript: DBC config + devnet end-to-end
docs/                 architecture & design notes
```

`tests-litesvm/` is a **separate** Cargo workspace on purpose: `litesvm` pulls a `solana`/`wincode`
crate line that would otherwise clash with `anchor-lang`'s. Keeping it detached lets each resolve
independently — see `tests-litesvm/Cargo.lock`.

## Common commands

| Task | `make` | Raw command |
|------|--------|-------------|
| Build the program | `make build` | `anchor build` |
| Property tests | `make test-vetrina` | `cargo test -p vetrina` |
| litesvm suite | `make test-litesvm` | `cd tests-litesvm && cargo test -- --nocapture` |
| Client typecheck | `make client` | `cd client && npm install && npm run typecheck` |
| Format | `make fmt` | `cargo fmt --all` |

The litesvm tests print a `SKIP` notice and pass if `target/deploy/vetrina.so` is absent, so run
`make build` first for a real run.

## Ground rules

- **The `vetrina` program is frozen.** Don't change `programs/vetrina/src/lib.rs` except for a real,
  demonstrated bug — and if a fix seems to require a semantics change, open an issue first.
- Arithmetic stays `checked_*`; `overflow-checks = true` stays on.
- Account validation is **declarative** (seeds / bump / `has_one` / `address =`) — don't add
  `require!`s that duplicate a constraint expressible as an attribute.
- No new dependencies without a reason.

## CI

Every push and PR runs `anchor build`, the property tests, and the litesvm suite against the freshly
built `.so`, and uploads that `.so` as an artifact. Green CI is required.
