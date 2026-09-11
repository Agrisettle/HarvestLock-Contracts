# Contributing

This repo doesn't keep a separate contribution guide — one guide for
every repo under the [Agrisettle](https://github.com/Agrisettle) org,
to avoid two copies drifting apart. Read
[`HarvestLock`'s `CONTRIBUTING.md`](https://github.com/Agrisettle/HarvestLock/blob/main/CONTRIBUTING.md).

This file exists (rather than only the pointer in `README.md`) so
GitHub's own repo health check recognizes a contribution guide is
in place — a real gap an audit caught: the org-wide pointer in prose
wasn't machine-recognized as satisfying it.

Before touching anything in `contracts/escrow/src/lib.rs`: read
[`HANDOFF.md`](./HANDOFF.md) first, then the file itself — the doc
comments carry real design intent, not boilerplate. Run `cargo test`
to confirm your starting point actually matches `HANDOFF.md` before
changing anything; if it doesn't, trust the code and test output over
the doc, and fix the doc to match before doing anything else.

Security issues go through [`SECURITY.md`](./SECURITY.md), not a
public issue.
