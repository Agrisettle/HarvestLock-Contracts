# Dev-environment troubleshooting

The toolchain problems this repo has actually hit — and the fixes that
worked — in one place. If you're setting this repo up for the first time
and something won't build, start here.

Both of these were solved once already, but written down in two different
places: [`HANDOFF.md`](../HANDOFF.md)'s toolchain section and
[`.github/workflows/test.yml`](../.github/workflows/test.yml)'s own
comments. This page consolidates them; those two stay the originals, and
neither should drift from what's here.

## What you actually need

| Working on | Needs |
| --- | --- |
| Running the tests | Rust only — `cargo test` runs on the host target |
| Building the deployable `.wasm` | `stellar-cli`, plus the `wasm32v1-none` target |
| Deploying / invoking on testnet | `stellar-cli`, plus a funded testnet identity |

`cargo test` does **not** need `stellar-cli`, and doesn't wait on it. If all
you're doing is verifying contract logic, that's the faster path.

## The pinned toolchain — `rust-toolchain.toml`

There is a [`rust-toolchain.toml`](../rust-toolchain.toml) at the repo root:

```toml
[toolchain]
channel = "1.97.1"
targets = ["wasm32v1-none"]
components = ["rustfmt", "clippy"]
```

**Why it exists.** [`docs/versioning.md`](./versioning.md) makes the WASM hash
the canonical identity of a version — but the hash is only an identity if the
compiler that produced it is fixed. Before this file, the hash depended on
whichever `stable` a machine happened to have that week, so the same commit
could yield a different hash next month. The reproducibility check in CI only
proves two builds *on the same runner* agree; it can't notice that the runner
itself changed. Pinning the channel is what makes the hash reproducible across
time and machines, not just across two consecutive builds.

**How it takes effect.** `rustup` reads it from the repo root and applies it to
every `cargo`/`rustc` invocation in this tree — including the ones CI runs and
the `cargo install --locked stellar-cli` step inside the `stellar contract
build` job. It is not something you have to remember to opt into:

```bash
rustup show          # "active toolchain" should read 1.97.1-<host>
rustc --version      # rustc 1.97.1 (...)
```

On a machine that has never installed `1.97.1`, the first `cargo`/`rustc` call
triggers rustup to fetch that channel, with the components and target listed
in the file. If you already had `1.97.1` installed without them, add them
explicitly rather than assuming the file retrofits an existing install:

```bash
rustup target add wasm32v1-none --toolchain 1.97.1
rustup component add rustfmt clippy --toolchain 1.97.1
```

**What it does *not* change.** The workflow still installs a toolchain with
`dtolnay/rust-toolchain@stable`; that action runs `rustup default stable`, but
the per-directory `rust-toolchain.toml` wins when `cargo` resolves a toolchain
inside this checkout, so CI builds with `1.97.1`. **Maintainers should read
that as a real change to what CI resolves** — the action's `@stable` is no
longer the effective compiler for this repo, and the `targets:
wasm32v1-none` input on the build job installs the target for `stable`, while
`1.97.1` gets it from this file. The CLI's own from-source install still
compiles under it — `stellar-cli` 30.0.1 declares `rust-version = "1.93.0"`.
Nothing else in the workflow needs to change for the pin to hold; changing
the workflow itself is maintainer-managed, per the note at the bottom of
this page.

**Bumping the pin.** Do it in its own PR, never bundled with an unrelated
change: update `channel`, run `cargo fmt --check`, `cargo test --locked`, and
`stellar contract build --locked`, re-record the WASM hash (HANDOFF.md's
toolchain section and the PR body), and expect a different hash even when the
contract source is untouched. Under [`docs/versioning.md`](./versioning.md)
that is a PATCH-class rebuild — and only becomes a version when a build is
actually uploaded.

**One thing to expect:** `cargo clippy --all-targets -- -D warnings` is *not*
clean at the pinned toolchain, and was not clean before it. Clippy reports
five `clippy::too_many_arguments` warnings on the contract's own entry points
(13–14 arguments against a threshold of 7) — a shape the Soroban ABI dictates,
not something to "fix" by bundling arguments. CI does not run Clippy, so this
is informational; don't be surprised by it, and don't silence it
repo-wide to make a local run look green.

## Gotcha 1 — `cargo install stellar-cli` fails on Windows

**Symptom.** `cargo install stellar-cli` fails while building the CLI's own
dependency tree, complaining about a missing `dlltool.exe`.

**Why.** `stellar-cli`'s dependency tree needs MinGW binutils to link when
your Rust host toolchain is `stable-x86_64-pc-windows-gnu` (the common
default on Windows). `dlltool.exe` comes from that package, it isn't
installed by default, and adding it isn't trivial. Nothing is wrong with
your checkout or with the CLI — this is the host toolchain being the wrong
shape for a from-source build of that dependency graph.

**Fix: don't compile it — use the prebuilt release binary.**

```bash
# Find the latest release and its Windows asset
gh api repos/stellar/stellar-cli/releases/latest
# Download stellar-cli-<version>-x86_64-pc-windows-msvc.tar.gz,
# extract stellar.exe, and drop it into ~/.cargo/bin/ (already on PATH)
```

The `-msvc` build is the point: it's a **standalone executable**, so it runs
fine regardless of what your local `rustup` host toolchain is — it does not
need to match. That's why this works when `cargo install` doesn't.

**Don't retry `cargo install stellar-cli` on a Windows machine with a GNU
host toolchain** without installing MinGW binutils first; it will fail the
same way every time.

## Gotcha 2 — a fresh Linux runner can't compile `stellar-cli` either

**Symptom.** On a clean `ubuntu-latest` runner, installing the CLI from
source fails deep inside its dependency tree — typically a `pkg-config` /
missing-header error while building `libdbus-sys` or `hidapi`.

**Why.** Two system *dev* libraries that the CLI's dependency tree needs are
not present on a fresh runner image:

- **`dbus-1`** — for `libdbus-sys`, the keyring support
- **`libudev`** — for `hidapi`, the hardware-wallet (Ledger) support

`pkg-config` is needed to find them. This is the same class of problem as
Gotcha 1 (a system-level dependency of the CLI's own tree, not of this
contract), on a different OS.

**Fix.**

```bash
sudo apt-get update && sudo apt-get install -y libdbus-1-dev libudev-dev pkg-config
```

That's exactly the line the `stellar contract build` CI job runs before
`cargo install --locked stellar-cli` — see `test.yml`.

**Worth knowing: they surfaced one at a time.** `dbus-1` broke the build job
unnoticed for two runs on 2 Sept 2026 before anyone caught it; fixing it
made `libudev` appear immediately as the *next* missing dependency, with the
same failure shape and a different library name. So if that job breaks again
on a newer runner image, expect a **third** library in the same shape, not
one of these two coming back — read the actual failing crate name rather
than assuming it's the known pair.

## Other things that look like setup problems but aren't

- **`error: target 'wasm32v1-none' not found`** — normally the root
  `rust-toolchain.toml` installs it along with the pinned channel; on an
  existing `1.97.1` install, add it yourself: `rustup target add
  wasm32v1-none --toolchain 1.97.1`. It's the current recommended Soroban
  target; don't "fix" it back to `wasm32-unknown-unknown`.
- **`Wasm does not exist` when deploying** — the build and the upload are
  two different steps. `stellar contract build` compiles locally;
  `stellar contract upload` is what puts it on testnet. Deploying a hash
  that was never uploaded fails with this. See HANDOFF.md's Deployment 4
  note.
- **A multi-party call fails with `TxBadAuthExtra`** — that's a *signing*
  problem, not a setup one, and it has its own write-up: each non-source
  auth entry has to be signed individually, not by piling up envelope
  signatures. See the `api/` repo's `test/helpers.ts` doc comment.

## If you hit a new one

Add it here, with the symptom as you saw it (the error text matters — these
are searched for). If the fix needs a change to the CI workflow, open an
issue instead: `.github/workflows/*` is maintainer-managed, and PRs are
explicitly asked not to touch it.
