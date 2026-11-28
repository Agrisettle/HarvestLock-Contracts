# `tests/fixtures/xdr/` — canonical XDR (base64) fixtures for issue #268

These JSON files hold canonical `ScVal` XDR (base64) produced by the Rust
contract for representative values. They are the regression anchors for the
TypeScript bindings: `bindings/typescript` decodes them through the generated
`contract.Spec` and re-encodes the same logical values, asserting byte
equality in both directions. Any client language can reuse the same fixtures.

## Regenerate (Rust)

From the repository root:

```sh
cargo test --test test_xdr_scratch --manifest-path contracts/escrow/Cargo.toml
```

This rewrites every `*.json` file in this directory.

## Verify (TypeScript)

From `bindings/typescript`:

```sh
npm install && npm run build && node --test test/*.mjs
```

Runs `test/xdr.test.mjs`, which loads the fixtures above and asserts byte
equality both ways. The test fails if a struct field is renamed or reordered
on either side.

## Files

- `initialize_with_oracle.json` — all 13 `initialize` arguments, `oracle_config = Some`
- `initialize_without_oracle.json` — all 13 `initialize` arguments, `oracle_config = None`
- `allocation_members.json` — a `Vec<AllocationMember>` (two members)
- `commitment_mid_lifecycle.json` — a `Commitment` read back mid-lifecycle (`Delivered`)
