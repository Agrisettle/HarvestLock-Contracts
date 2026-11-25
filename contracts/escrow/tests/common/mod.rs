//! Re-export shim, nothing more (#274).
//!
//! The fixtures every suite here uses — the default windows, the grade
//! schedule, the token minting, the clock helpers, and the `World`
//! constructors/drive methods — now live in one place, in the crate itself,
//! as `harvestlock_escrow::testutils`. This module used to be a parallel copy
//! of `src/test.rs`'s harness, documented as "mirroring the conventions" of
//! it, with the same constants stored twice; the two copies could drift, and
//! every new suite was one more chance to pick one or add a third.
//!
//! Why the shim at all: keeping the `tests/common/mod.rs` path (rather than
//! rewriting every suite's `#[path = "common/mod.rs"] mod common;` to import
//! the crate module directly) means the suites' own code is unchanged by the
//! refactor, which is what makes "no test asserts anything different" easy to
//! check — the diff for this issue is the harness moving, not the tests.
//!
//! The module stays in a subdirectory (`tests/common/mod.rs`) rather than
//! `tests/common.rs` so cargo does not treat it as its own test target.

pub use harvestlock_escrow::testutils::*;
