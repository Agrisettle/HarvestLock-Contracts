# `escrow-reader` — how another contract should call the escrow

A minimal downstream Soroban contract that reads HarvestLock escrow state
across the contract boundary, and the worked example of the **supported**
integration pattern (`#276`): build the escrow to WASM, then import that
artifact with `soroban_sdk::contractimport!`.

```rust
pub mod escrow {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/harvestlock_escrow.wasm"
    );
}

pub fn status_of(env: Env, escrow_id: Address) -> escrow::Status {
    escrow::Client::new(&env, &escrow_id).get_status()
}
```

`contractimport!` reads the `contractspecv0` section of the built artifact and
generates the cross-contract client (`escrow::Client`), the interface types it
names (`Status`, `Commitment`, `Action`, the events) and the error enum. Those
types come from the spec of the artifact being called, so a caller cannot
disagree with the escrow about the wire format.

## Build it first

The example compiles against the escrow's built WASM, so that has to exist:

```bash
# from the repo root
stellar contract build --package harvestlock-escrow

# then the example's test, and (optionally) its own WASM
cargo test --manifest-path examples/escrow-reader/Cargo.toml
cd examples/escrow-reader && stellar contract build
```

`cargo test` inside this directory fails to compile without that artifact —
that is the one prerequisite of the pattern, and the reason this crate is its
own workspace instead of a member of the root one (a member would impose the
artifact requirement on `cargo build`/`cargo test`/`cargo llvm-cov
--workspace` for the whole repo, so a clean checkout would fail those before
the escrow had ever been built).

## Why the WASM, and not the `rlib`

The escrow is `crate-type = ["cdylib", "rlib"]`, which does invite
`harvestlock-escrow = { path = "..." }` as an ordinary Rust dependency. A
contract that will be deployed should not do that:

| | importing the WASM | depending on the rlib |
| --- | --- | --- |
| What gets compiled into the caller | the generated client and interface types | the escrow's whole implementation (state machine, storage keys, error enum) |
| Which storage the caller touches | the escrow contract's, via a cross-contract call | its own instance storage — a compiled-in copy is a *different* contract |
| Coupling | the deployed artifact's spec | the escrow's source |
| Rebuild needed when the escrow's internals change | no, as long as the spec is unchanged | yes |

Storage is the decisive one: Soroban storage is per contract address. A
caller that links the rlib and calls `get_commitment`-shaped code in-process
reads *its own* instance storage, which was never initialized, and gets
`NotInitialized`. The rlib's legitimate uses are test targets (`cargo test`
in `contracts/escrow`) and tooling that wants the Rust types without
deploying anything — not a deployed contract.

The sizes make the difference visible: the reader's own WASM is ~8 KB and
exports exactly two functions, because it embeds a thin `invoke_contract`
wrapper — not the escrow.

## What it demonstrates

`status_of` returns a unit-variant enum (`Status`) and `commitment_of`
returns the whole `Commitment` struct, so both directions of the ABI are
covered. Both escrow calls are auth-free reads, which is why this contract
needs no `require_auth` and no storage of its own. The test registers the
escrow from `escrow_wasm::WASM` — the built artifact, exactly what a real
deployment uploads — registers the reader from source, initializes the deal,
and checks the reader sees both the starting status and a later transition.
