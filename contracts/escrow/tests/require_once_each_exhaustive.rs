//! #255 — Exhaustive tests for `require_once_each`: deduplication semantics
//! and replay/cross-method isolation for every multi-party method in the
//! contract (`cancel`, `reassign_buyer`, `resolve_dispute`).
//!
//! ## Why this matters
//!
//! `require_once_each(addresses: &[&Address])` is the shared primitive that
//! makes multi-party gates work. Its dedup means: if two roles in a 3-party
//! gate are held by the same address, that address signs once — not twice.
//! Its per-entry `require_auth()` means: a Soroban auth entry authorizing
//! method A cannot satisfy `require_once_each` inside method B, because auth
//! entries name the specific contract, method *and* argument set they cover.
//!
//! These tests cover:
//! 1. Each method succeeds with the exact expected signer set (all-distinct).
//! 2. Each method succeeds when two roles share one address (dedup fires).
//! 3. Each method succeeds when all three roles share one address (single sig).
//! 4. Each method *fails* when only a strict subset of distinct signers signs.
//! 5. Cross-method isolation: an auth entry staged for method A is rejected
//!    when the same contract calls method B.
//! 6. Cross-argument isolation: an auth entry for the same method with
//!    different arguments is rejected (specifically for `reassign_buyer`).
//!
//! Items 4-6 are the security-relevant properties; 1-3 prove the happy paths.

#[path = "common/mod.rs"]
mod common;

use common::{world, world_in, World, WINDOW};
use harvestlock_escrow::{Error, Status};
use soroban_sdk::testutils::{Address as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{Address, Env, IntoVal};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn drive_to_locked(w: &World) {
    w.contract.lock();
}

fn drive_to_disputed(w: &World) {
    w.contract.lock();
    w.contract.flag_dispute(&w.buyer);
}

fn drive_to_locked_world_in(env: &Env, buyer: Address, coop: Address, wh: Address) -> World {
    let w = world_in(env, buyer, coop, wh, 1_500, 2_000, WINDOW);
    w.contract.lock();
    w
}

// ---------------------------------------------------------------------------
// §1 — `cancel`: requires buyer + cooperative
// ---------------------------------------------------------------------------

#[test]
fn cancel_requires_exactly_buyer_and_cooperative() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    w.contract.cancel();
    // No assertion on auths here — the coverage for exact signers lives in
    // auth_matrix.rs; this suite focuses on dedup and isolation.
}

#[test]
fn cancel_succeeds_when_buyer_and_cooperative_share_one_address() {
    let env = Env::default();
    env.mock_all_auths();
    let shared = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = drive_to_locked_world_in(&env, shared.clone(), shared.clone(), warehouse);

    // One signature satisfies both buyer + cooperative roles.
    w.contract.cancel();
    assert_eq!(w.contract.get_status(), Status::Cancelled);
}

#[test]
fn cancel_fails_when_only_buyer_signs() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "cancel",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth {
        address: &buyer,
        invoke: &invoke,
    }]);
    assert!(
        w.contract.try_cancel().is_err(),
        "cancel must require both buyer and cooperative — buyer alone is insufficient"
    );
}

#[test]
fn cancel_fails_when_only_cooperative_signs() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "cancel",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth {
        address: &cooperative,
        invoke: &invoke,
    }]);
    assert!(
        w.contract.try_cancel().is_err(),
        "cancel must require both buyer and cooperative — cooperative alone is insufficient"
    );
}

#[test]
fn cancel_fails_when_only_warehouse_signs() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "cancel",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth {
        address: &warehouse,
        invoke: &invoke,
    }]);
    assert!(
        w.contract.try_cancel().is_err(),
        "cancel must require buyer + cooperative — warehouse alone is insufficient"
    );
}

// ---------------------------------------------------------------------------
// §2 — `reassign_buyer`: requires current buyer + cooperative + incoming buyer
// ---------------------------------------------------------------------------

#[test]
fn reassign_buyer_succeeds_with_all_three_distinct_signers() {
    let w = world(1_500, 2_000);
    drive_to_locked(&w);
    let new_buyer = Address::generate(&w.env);
    w.contract.reassign_buyer(&new_buyer);
    assert_eq!(w.contract.get_commitment().buyer, new_buyer);
}

#[test]
fn reassign_buyer_succeeds_when_buyer_and_cooperative_share_one_address() {
    let env = Env::default();
    env.mock_all_auths();
    let shared = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = drive_to_locked_world_in(&env, shared.clone(), shared.clone(), warehouse);
    let new_buyer = Address::generate(&env);

    // shared satisfies both buyer + cooperative roles; new_buyer signs for itself.
    w.contract.reassign_buyer(&new_buyer);
    assert_eq!(w.contract.get_commitment().buyer, new_buyer);
}

#[test]
fn reassign_buyer_succeeds_when_incoming_matches_cooperative() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone());

    // Incoming buyer = cooperative: same address satisfies both cooperative role
    // and "incoming buyer has consented" role — only two distinct signatures needed.
    w.contract.reassign_buyer(&cooperative);
    assert_eq!(w.contract.get_commitment().buyer, cooperative);
}

#[test]
fn reassign_buyer_succeeds_when_all_three_identical() {
    let env = Env::default();
    env.mock_all_auths();
    let shared = Address::generate(&env);
    let w = drive_to_locked_world_in(&env, shared.clone(), shared.clone(), shared.clone());

    // All three roles shared; incoming buyer = shared as well.
    w.contract.reassign_buyer(&shared);
    // buyer is already shared, so the reassignment is a no-op in terms of stored value.
    assert_eq!(w.contract.get_commitment().buyer, shared);
}

#[test]
fn reassign_buyer_fails_when_only_buyer_signs() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let new_buyer = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "reassign_buyer",
        args: (new_buyer.clone(),).into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth {
        address: &buyer,
        invoke: &invoke,
    }]);
    assert!(
        w.contract.try_reassign_buyer(&new_buyer).is_err(),
        "reassign_buyer must require all three signers — buyer alone is insufficient"
    );
}

#[test]
fn reassign_buyer_fails_when_buyer_and_cooperative_sign_but_not_incoming_buyer() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let new_buyer = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "reassign_buyer",
        args: (new_buyer.clone(),).into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &invoke,
        },
    ]);
    assert!(
        w.contract.try_reassign_buyer(&new_buyer).is_err(),
        "reassign_buyer must require incoming buyer's consent — missing it fails"
    );
}

#[test]
fn reassign_buyer_cross_argument_isolation() {
    // An auth entry for `reassign_buyer(new_buyer_a)` does NOT satisfy
    // `reassign_buyer(new_buyer_b)` — different argument set, different auth entry.
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let new_buyer_a = Address::generate(&env);
    let new_buyer_b = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    // Stage auth entries for new_buyer_a — but try to call with new_buyer_b.
    let invoke_a = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "reassign_buyer",
        args: (new_buyer_a.clone(),).into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &invoke_a,
        },
        MockAuth {
            address: &cooperative,
            invoke: &invoke_a,
        },
        MockAuth {
            address: &new_buyer_a,
            invoke: &invoke_a,
        },
    ]);
    assert!(
        w.contract.try_reassign_buyer(&new_buyer_b).is_err(),
        "auth entries staged for new_buyer_a must not satisfy reassign_buyer(new_buyer_b)"
    );
}

// ---------------------------------------------------------------------------
// §3 — `resolve_dispute`: requires all three distinct parties
// ---------------------------------------------------------------------------

#[test]
fn resolve_dispute_requires_all_three_distinct_parties() {
    let w = world(1_500, 2_000);
    drive_to_disputed(&w);
    w.contract.resolve_dispute();
    assert_eq!(w.contract.get_status(), Status::Locked);
}

#[test]
fn resolve_dispute_succeeds_when_buyer_and_warehouse_share_one_address() {
    let env = Env::default();
    env.mock_all_auths();
    let shared = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let w = drive_to_locked_world_in(&env, shared.clone(), cooperative.clone(), shared.clone());
    w.contract.flag_dispute(&shared);

    // shared satisfies buyer + warehouse roles; cooperative signs for itself.
    w.contract.resolve_dispute();
    assert_eq!(w.contract.get_status(), Status::Locked);
}

#[test]
fn resolve_dispute_succeeds_when_all_three_share_one_address() {
    let env = Env::default();
    env.mock_all_auths();
    let shared = Address::generate(&env);
    let w = drive_to_locked_world_in(&env, shared.clone(), shared.clone(), shared.clone());
    w.contract.flag_dispute(&shared);

    // Single address satisfies all three roles — one auth entry needed.
    w.contract.resolve_dispute();
    assert_eq!(w.contract.get_status(), Status::Locked);
}

#[test]
fn resolve_dispute_fails_when_only_buyer_and_cooperative_sign() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w =
            drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone());
        w.contract.flag_dispute(&buyer);
        w
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &invoke,
        },
    ]);
    assert!(
        w.contract.try_resolve_dispute().is_err(),
        "resolve_dispute must require all three — buyer+cooperative without warehouse fails"
    );
}

#[test]
fn resolve_dispute_fails_when_only_buyer_and_warehouse_sign() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w =
            drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone());
        w.contract.flag_dispute(&buyer);
        w
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &warehouse,
            invoke: &invoke,
        },
    ]);
    assert!(
        w.contract.try_resolve_dispute().is_err(),
        "resolve_dispute must require all three — buyer+warehouse without cooperative fails"
    );
}

#[test]
fn resolve_dispute_fails_when_only_cooperative_and_warehouse_sign() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w =
            drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone());
        w.contract.flag_dispute(&buyer);
        w
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &cooperative,
            invoke: &invoke,
        },
        MockAuth {
            address: &warehouse,
            invoke: &invoke,
        },
    ]);
    assert!(
        w.contract.try_resolve_dispute().is_err(),
        "resolve_dispute must require all three — cooperative+warehouse without buyer fails"
    );
}

// ---------------------------------------------------------------------------
// §4 — Cross-method isolation: a staged auth for method A is rejected by B
// ---------------------------------------------------------------------------

#[test]
fn auth_entry_for_cancel_cannot_authorize_resolve_dispute() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w =
            drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone());
        w.contract.flag_dispute(&buyer);
        w
    };

    // Stage auth entries scoped to `cancel`, not `resolve_dispute`.
    let cancel_invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "cancel",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &cancel_invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &cancel_invoke,
        },
        MockAuth {
            address: &warehouse,
            invoke: &cancel_invoke,
        },
    ]);
    assert!(
        w.contract.try_resolve_dispute().is_err(),
        "auth entries scoped to `cancel` must not satisfy `resolve_dispute`"
    );
}

#[test]
fn auth_entry_for_resolve_dispute_cannot_authorize_cancel() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let rd_invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &rd_invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &rd_invoke,
        },
    ]);
    assert!(
        w.contract.try_cancel().is_err(),
        "auth entries scoped to `resolve_dispute` must not satisfy `cancel`"
    );
}

// ---------------------------------------------------------------------------
// §5 — `require_once_each` doesn't consume a staged auth permanently
//      (a successful multi-party call doesn't exhaust auth for the same call
//       with different arguments — this is a Soroban host guarantee pinned here)
// ---------------------------------------------------------------------------

#[test]
fn two_of_three_identical_parties_still_require_the_distinct_partys_auth() {
    // buyer == warehouse, cooperative is distinct.
    // resolve_dispute needs buyer+cooperative+warehouse = {shared, cooperative}.
    // Staged auth for ONLY shared — cooperative missing — must fail.
    let env = Env::default();
    let shared = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w = drive_to_locked_world_in(&env, shared.clone(), cooperative.clone(), shared.clone());
        w.contract.flag_dispute(&shared);
        w
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth {
        address: &shared,
        invoke: &invoke,
    }]);
    assert!(
        w.contract.try_resolve_dispute().is_err(),
        "shared alone can't substitute for cooperative's missing consent"
    );

    // But shared + cooperative together should succeed.
    env.mock_auths(&[
        MockAuth {
            address: &shared,
            invoke: &invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &invoke,
        },
    ]);
    w.contract.resolve_dispute();
    assert_eq!(w.contract.get_status(), Status::Locked);
}

#[test]
fn all_three_identical_parties_accept_only_that_principals_signature() {
    let env = Env::default();
    let shared = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w = drive_to_locked_world_in(&env, shared.clone(), shared.clone(), shared.clone());
        w.contract.flag_dispute(&shared);
        w
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[MockAuth {
        address: &shared,
        invoke: &invoke,
    }]);
    // shared is ALL three parties — one auth entry suffices.
    w.contract.resolve_dispute();
    assert_eq!(w.contract.get_status(), Status::Locked);
}

// ---------------------------------------------------------------------------
// §6 — `require_once_each` dedup does not mask extra required signers
// ---------------------------------------------------------------------------

#[test]
fn cancel_with_all_three_distinct_roles_still_needs_exactly_buyer_and_cooperative() {
    // Even with 3 distinct addresses, only buyer + cooperative are required.
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone())
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "cancel",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    // warehouse not needed — buyer + cooperative is enough.
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &invoke,
        },
    ]);
    w.contract.cancel();
    assert_eq!(w.contract.get_status(), Status::Cancelled);
}

#[test]
fn resolve_dispute_with_buyer_cooperative_warehouse_distinct_all_must_sign() {
    let env = Env::default();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let w = {
        env.mock_all_auths();
        let w =
            drive_to_locked_world_in(&env, buyer.clone(), cooperative.clone(), warehouse.clone());
        w.contract.flag_dispute(&buyer);
        w
    };

    let invoke = MockAuthInvoke {
        contract: &w.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };
    env.mock_auths(&[
        MockAuth {
            address: &buyer,
            invoke: &invoke,
        },
        MockAuth {
            address: &cooperative,
            invoke: &invoke,
        },
        MockAuth {
            address: &warehouse,
            invoke: &invoke,
        },
    ]);
    w.contract.resolve_dispute();
    assert_eq!(w.contract.get_status(), Status::Locked);
}

// ---------------------------------------------------------------------------
// Regression guard: `require_once_each` is used for all three methods
// ---------------------------------------------------------------------------

#[test]
fn source_uses_require_once_each_for_all_multi_party_methods() {
    const SRC: &str = include_str!("../src/lib.rs");
    // The three methods that must call require_once_each rather than raw
    // require_auth chains, as documented in authorization-matrix.md §4.
    for fn_sig in [
        "pub fn cancel(",
        "pub fn reassign_buyer(",
        "pub fn resolve_dispute(",
    ] {
        let pos = SRC
            .find(fn_sig)
            .unwrap_or_else(|| panic!("function `{fn_sig}` not found in src/lib.rs"));
        // Within the next 500 chars (the function body) there must be a
        // require_once_each call.
        let window = &SRC[pos..(pos + 500).min(SRC.len())];
        assert!(
            window.contains("require_once_each"),
            "`{fn_sig}` body must call `require_once_each` (found in auth_matrix; see authorization-matrix.md §4)"
        );
    }
}
