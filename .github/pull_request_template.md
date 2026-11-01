## Summary

<!-- What changed and why? Link the issue if there is one. -->

## Changes

-
-

## Verification

<!-- Paste the commands you ran and their results. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets`
- [ ] `cargo test --workspace`
- [ ] `npm run typecheck` in `apps/web`
- [ ] `npm run build` in `apps/web`
- [ ] Relevant smoke/security/webhook/UI tests

## Screenshots

<!-- For UI changes, include before/after images. Do not include real customer data. -->

## Security and data checklist

- [ ] No `.env`, secrets, real invoices, or customer data are included.
- [ ] Tenant-scoped queries remain scoped by `organization_id`.
- [ ] Approval remains an explicit server-side transition.
- [ ] Money remains exact decimal, not floating point.
- [ ] API/docs/openapi are updated if routes or schemas changed.
