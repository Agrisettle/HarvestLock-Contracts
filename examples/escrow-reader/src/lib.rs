//! A minimal downstream contract that reads HarvestLock escrow state (#276).
//!
//! This exists to answer one question a future factory, a pooled buyer
//! contract, or anything else that needs escrow state on chain will ask:
//! *how should another contract call the escrow?* The answer this example
//! pins is **build the escrow to WASM and import the WASM**:
//!
//! ```ignore
//! mod escrow {
//!     soroban_sdk::contractimport!(
//!         file = "../../target/wasm32v1-none/release/harvestlock_escrow.wasm"
//!     );
//! }
//! ```
//!
//! `contractimport!` reads the `contractspecv0` section of the deployed
//! artifact and generates, in the importing crate: the cross-contract client
//! (`escrow::Client`), every type its interface names (`Status`,
//! `Commitment`, `Action`, …), the error enum, and the event types. Those
//! generated types are *the spec's* types — the same wire representation the
//! deployed contract encodes — so a downstream contract cannot disagree with
//! the escrow about how a `Status` is laid out.
//!
//! ## Why the WASM and not the rlib
//!
//! `harvestlock-escrow` is `crate-type = ["cdylib", "rlib"]`, which does
//! invite `harvestlock-escrow = { path = "..." }` as a normal Rust
//! dependency. Don't do that from a contract that will be deployed:
//!
//! * It compiles the escrow's **implementation** into the caller's WASM. Every
//!   downstream contract would then carry a second copy of the escrow's state
//!   machine, its storage keys and its error enum — a few tens of kilobytes
//!   per deploy, and a second, caller-local copy of the very code the escrow
//!   contract is supposed to own.
//! * Storage is per-contract-address, so those compiled-in copies would read
//!   and write *the caller's* instance storage, not the escrow's. A "read the
//!   commitment" call built that way returns `NotInitialized`, not the deal.
//! * The escrow's `#![no_std]` + `cfg` ledger is tuned for a single deployed
//!   instance: the public entry points are `#[contractimpl]`-generated for
//!   that contract's own address. Calling them in-process is not calling the
//!   escrow.
//! * The rlib is a *source-level* coupling: a caller rebuilds whenever the
//!   escrow's internals change, and can be compiled against a version whose
//!   deployed instance has a different spec. The WASM import is the
//!   *interface* coupling: it tracks the spec of the artifact actually being
//!   called.
//!
//! The rlib has exactly two legitimate uses in this repo, neither of which is
//! a deployed contract: `cargo test`'s own targets (the integration suites
//! link it), and tools that need the Rust types without deploying anything.
//!
//! ## What it costs
//!
//! Importing the WASM means the escrow has to be built first — see this
//! directory's README. That is a build-order fact, not a runtime one: the
//! generated client is a thin `invoke_contract` wrapper, and the example's
//! own WASM contains only this contract plus the generated client, not the
//! escrow.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env};

/// The escrow's interface, imported from its built WASM. `pub` so the types
/// this contract's own interface names (`escrow::Status`,
/// `escrow::Commitment`) are nameable by callers and tests; the module holds
/// no state of its own.
pub mod escrow {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/harvestlock_escrow.wasm"
    );
}

/// A contract whose only job is to read the escrow on someone else's behalf.
///
/// Both calls are pure reads — the escrow's own `get_status` and
/// `get_commitment` take no auth and change nothing — so this contract needs
/// neither `require_auth` nor any storage of its own. It is the smallest
/// useful demonstration of the pattern: a cross-contract call in each
/// direction of the ABI (a unit-variant enum and a struct).
#[contract]
pub struct EscrowReader;

#[contractimpl]
impl EscrowReader {
    /// The escrow's current `Status`, read across the contract boundary.
    ///
    /// The generated `escrow::Client` method (`get_status`) traps if the
    /// callee returns an error, which is the right default here: a caller
    /// that cannot read the status has nothing useful to do with the result.
    /// A caller that wants to handle the escrow's `Error` itself uses the
    /// client's `try_get_status` instead — same call, recoverable result.
    pub fn status_of(env: Env, escrow_id: Address) -> escrow::Status {
        escrow::Client::new(&env, &escrow_id).get_status()
    }

    /// The escrow's whole `Commitment` struct, read across the boundary.
    ///
    /// This is the shape that matters for the "why WASM, not rlib" argument:
    /// `escrow::Commitment` here is generated from the deployed spec, so its
    /// fields are the fields the escrow actually stores and returns. A
    /// caller that needs one field (the buyer, say) still pays for the whole
    /// struct on the wire — that is the escrow's interface, and
    /// `get_commitment_view` is the alternative when a stable read model is
    /// preferable to the storage struct.
    pub fn commitment_of(env: Env, escrow_id: Address) -> escrow::Commitment {
        escrow::Client::new(&env, &escrow_id).get_commitment()
    }
}
