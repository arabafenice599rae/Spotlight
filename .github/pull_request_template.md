## Summary

<!-- What does this change and why? -->

## Changes

-

## Testing

<!-- How was this verified? -->

- [ ] `cargo test -p vetrina` (property tests) passes
- [ ] `make build` + `make test-litesvm` passes (or CI is green)
- [ ] `cd client && npm run typecheck` passes (if the client changed)

## Checklist

- [ ] The `vetrina` program semantics/invariants are unchanged (or an issue was opened first)
- [ ] Arithmetic stays `checked_*`; account validation stays declarative
- [ ] No new dependencies without justification
- [ ] Docs updated if behavior or layout changed
