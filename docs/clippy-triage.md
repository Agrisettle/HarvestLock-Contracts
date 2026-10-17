# Clippy pedantic and nursery triage

The CI job runs:

```sh
cargo clippy --workspace --all-targets --locked -- -W clippy::pedantic -W clippy::nursery
```

The first run on Rust/Clippy 1.99.0 completed successfully and emitted 168
individual warning diagnostics. After fixing the straightforward findings,
the verification run completed with 131 warning diagnostics remaining; some
findings are repeated when a target is checked in more than one test
configuration. This baseline is advisory:
Clippy's `-W` flags intentionally report warnings without failing the job.
The lints below were triaged; the clear correctness and readability improvements
were fixed, while the remaining warnings are not worth changing contract
interfaces or making broad documentation-only edits in this pass.

## Fixed

- Replaced the narrowing casts used for delivery and settlement basis points
  with checked `u32::try_from` conversions. The quantities are bounded by the
  validated contract limits; the explicit conversion now also guards that
  invariant if the calculation changes later.
- Replaced lossless `u32`-to-`i128` casts with `i128::from`, and made the
  hexadecimal-byte conversion checked.
- Simplified literal-boolean assertions, changed `0..N + 1` test ranges to
  inclusive ranges, and used `map_or_else` for the fixture lookup fallbacks.
- Removed redundant clones, used `Self` in the `Terms` default implementation,
  and made the snapshot oracle configuration construction an `Option::map`
  expression.
- Changed the no-events assertion to compare the actual event list with an
  empty list so a failure prints the unexpected events.

## Deliberately retained

| Lint | Remaining findings | Triage |
| --- | ---: | --- |
| `needless_pass_by_value` | 43 | Soroban contract entry points use owned SDK values as part of their generated contract/client ABI. Changing those parameters to references is not a local cleanup. Test oracle entry points also mirror that ABI. |
| `missing_errors_doc` | 29 | These are contract entry points whose method docs already describe the relevant authorization and state preconditions. Adding repetitive `# Errors` sections to every entry point would duplicate the contract's documented error enum and is deferred until the API docs are being revised as a whole. |
| `doc_markdown`, `too_long_first_doc_paragraph` | 38 | These are prose-formatting suggestions, not behavior or correctness issues. The existing module and entry-point docs are intentionally narrative; a broad backtick/paragraph rewrite would create churn without improving contract behavior. |
| `missing_const_for_fn` | 9 | Const qualification has no runtime benefit here and is not needed by callers. Defer unless a compile-time use case emerges. |
| `too_many_arguments` | 4 | The large signature is `initialize`, a public Soroban contract entry point. Grouping its inputs would change the deployed contract ABI and generated client calls. |
| `struct_excessive_bools` | 1 | The warning is on the serialized `Commitment` contract state. Packing flags would change its Soroban ScVal representation and break existing state/API compatibility. |
| `used_underscore_binding` | 6 | The unused `_asset` parameters belong to mock oracle entry points that intentionally match the oracle interface. The underscore communicates that the contract accepts but does not inspect this argument. |
| `too_many_lines` | 1 | `load_snapshot` is a single fixture parser with ordered line-by-line state handling and contextual parse errors. Splitting it solely to satisfy a size threshold would add indirection without improving the parser's responsibilities. |

The warning counts are a record of the initial triage run, not a target that
must stay fixed: newly introduced warnings should be reviewed in context, and
the advisory job can be tightened as the remaining baseline is intentionally
reduced.
