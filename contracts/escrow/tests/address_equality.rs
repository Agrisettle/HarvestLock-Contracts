//! #122 — Address-equality audit, plus the tests that make the audit
//! re-runnable rather than a one-time read.
//!
//! ## The audit (every Address-vs-Address comparison in `src/lib.rs`)
//!
//! | Site | Comparison | Verdict |
//! |---|---|---|
//! | `claim_tranche` / `reclaim_tranche` / `lock` / `fund_remainder` / `initialize` / `set_allocation` / `ready_for_delivery` / `mark_checkpoint` / `confirm_delivery` / `reclaim_on_nondelivery` / `fund_fx_shortfall` — the `require_auth()` call sites | no comparison at all; `Address::require_auth()` delegates to the host's auth manager, which matches the *authorizing* address against the address value carried in the auth entry | type-safe, host-enforced |
//! | `flag_dispute`'s `NotAParty` check | `flagger != c.buyer && flagger != c.cooperative && flagger != c.warehouse_operator` | `soroban_sdk::Address: PartialEq` → host `obj_eq` on the `ScAddress` value. Not string- or byte-based |
//! | `require_once_each` (used by `cancel`, `reassign_buyer`, `resolve_dispute`) | `addresses[..i].iter().all(|seen| seen != addr)` on `&Address` | same `PartialEq`, through one layer of reference; no coercion anywhere |
//! | `settle` / `resolve_fx_shortfall` / token transfers | addresses used only as *keys* (`token::Client::new(&env, &c.token)`, transfer `from`/`to` args) | no equality comparison |
//! | `get_commitment` / `initialize` | `Address` fields stored and read back as `ScVal::Address` | `contracttype` round-trip preserves the address value (pinned below) |
//!
//! **Finding: no stringly-typed comparison exists, so there is nothing to
//! fix.** `src/lib.rs` never converts an `Address` to `String`/`Symbol`/
//! bytes for a comparison, never calls `to_string`/`to_strkey`/`format!`, and
//! has no `String` type in scope at all (`#![no_std]`, and `BytesN<32>` appears
//! only as `AllocationMember::member_hash`, which is compared nowhere). The
//! only `Address` equality in the codebase is the `PartialEq` the `flag_dispute`
//! check uses, which the Soroban host evaluates over the canonical `ScAddress`
//! value — so two separately constructed `Address` objects holding the same
//! account/contract bytes compare equal, and formatting can never make two
//! different addresses look alike (or one address look different).
//!
//! Two things this suite adds so the finding can't silently rot:
//! 1. a source scan asserting those string-conversion tokens never appear in
//!    `src/lib.rs` again — the same style of guard the repo already runs in CI
//!    (`.github/scripts/check_error_discriminants.sh`), but scoped to this
//!    issue's specific risk;
//! 2. positive/negative tests of the equality semantics the checks depend on.

#[path = "common/mod.rs"]
mod common;

use common::{world, world_in};
use harvestlock_escrow::{Error, Status};
use soroban_sdk::testutils::{Address as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{token, Address, Env, IntoVal, TryIntoVal, Val};

/// An `Address` survives the `Val`/`ScAddress` round trip the `contracttype`
/// storage layer performs, and stays `==` to the original: equality is on the
/// address *value*, not on object identity or any derived text form.
#[test]
fn address_equality_is_value_based_across_a_val_round_trip() {
    let s = world(1_500, 2_000);
    s.contract.lock();

    let original = s.contract.get_commitment().buyer;
    let val: Val = original.into_val(&s.env);
    let rebuilt: Address = val.try_into_val(&s.env).unwrap();

    assert_eq!(rebuilt, original);
    assert_eq!(rebuilt, s.buyer);
    // And the host agrees they're the same principal: identical balances,
    // because it really is one account, not two look-alikes.
    assert_eq!(s.token.balance(&rebuilt), s.token.balance(&s.buyer));
}

/// The negative half: a distinct generated address is *not* equal, and no
/// amount of identical shape (same version byte / same strkey length, both
/// `ScAddress::TypeAccount`) makes the comparison degrade to a prefix match.
#[test]
fn distinct_addresses_are_unequal_and_not_prefix_confusable() {
    let env = Env::default();
    env.mock_all_auths();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    assert_ne!(a, b);

    // The host agrees they are two different principals, not one principal in
    // two encodings: paying one leaves the other's balance untouched. `Val`
    // deliberately isn't compared here — it has no `PartialEq`, which is the
    // point: the only comparison available on an `Address` is the SDK's
    // value-based `PartialEq`, never a raw-representation one.
    let token_admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin);
    let token = token::Client::new(&env, &sac.address());
    let mint = token::StellarAssetClient::new(&env, &sac.address());
    mint.mint(&a, &1_000);

    assert_eq!(token.balance(&a), 1_000);
    assert_eq!(token.balance(&b), 0);

    // Same host value, re-derived from its own `Val`, is still equal — i.e.
    // the inequality above is about address values, not object handles.
    let va: Val = a.into_val(&env);
    let a_again: Address = va.try_into_val(&env).unwrap();
    assert_eq!(a, a_again);
    assert_ne!(a_again, b);
}

/// `flag_dispute`'s `NotAParty` check is the one place this contract
/// hand-compares addresses. It accepts a party's address even when it arrives
/// as a freshly decoded `ScAddress` value (a real cross-ledger occurrence:
/// an address that came in through an auth entry, another contract's storage,
/// or the API's staged transaction rather than as a clone of the stored
/// field), which a stringified comparison would also survive — but see the
/// next two tests for where a stringified comparison would *differ*.
#[test]
fn flag_dispute_recognizes_a_party_by_address_value_not_by_object_provenance() {
    let s = world(1_500, 2_000);
    s.contract.lock();

    // Encode the cooperative's address as an argument value and decode it
    // again, the way a cross-contract caller or the API's staged-transaction
    // layer hands an address over — a different `Val` object than the one
    // storage holds, but the same `ScAddress` value.
    let as_val: Val = s.cooperative.clone().into_val(&s.env);
    let decoded: Address = as_val.try_into_val(&s.env).unwrap();
    assert_eq!(decoded, s.cooperative);

    s.contract.flag_dispute(&decoded);
    assert_eq!(s.status(), Status::Disputed);
    assert_eq!(
        s.contract.get_commitment().dispute_pre_status,
        Status::Locked
    );
}

/// A stranger is rejected — the check compares against all three stored
/// roles, not against "some address that looks like one of ours".
#[test]
fn flag_dispute_rejects_an_unrelated_address() {
    let s = world(1_500, 2_000);
    s.contract.lock();
    let stranger = Address::generate(&s.env);

    assert_eq!(
        s.contract.try_flag_dispute(&stranger),
        Err(Ok(Error::NotAParty))
    );
    assert_eq!(s.status(), Status::Locked);
}

/// The comparison is re-read against live state, not cached: after
/// `reassign_buyer`, the *new* buyer is a party by value and the outgoing one
/// is not. This is the case a stale/stringly comparison is most likely to get
/// wrong, since both addresses are valid, well-formed strkeys of the same
/// type and only one of them is in `c.buyer`.
#[test]
fn party_membership_tracks_the_stored_address_after_reassign() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let cooperative = Address::generate(&env);
    let warehouse = Address::generate(&env);
    let s = world_in(
        &env,
        buyer.clone(),
        cooperative,
        warehouse,
        1_500,
        2_000,
        common::WINDOW,
    );

    let new_buyer = Address::generate(&env);
    // `reassign_buyer` needs three distinct signatures; under
    // `mock_all_auths` they're all granted.
    s.contract.lock();
    s.contract.reassign_buyer(&new_buyer);

    let c = s.contract.get_commitment();
    assert_eq!(c.buyer, new_buyer);
    assert_ne!(c.buyer, buyer);

    // The outgoing buyer is no longer a party; the incoming one is.
    assert_eq!(
        s.contract.try_flag_dispute(&buyer),
        Err(Ok(Error::NotAParty))
    );
    s.contract.flag_dispute(&new_buyer);
    assert_eq!(s.status(), Status::Disputed);
}

/// `require_once_each`'s dedup (`seen != addr`) is the other hand-written
/// address comparison in the contract. With one principal holding two roles,
/// value equality is what makes the dedup fire: `resolve_dispute` then needs
/// exactly *one* auth entry for the shared address plus one for the distinct
/// cooperative — two entries for the shared address would be impossible to
/// satisfy, and a comparison that degraded to object identity would ask twice
/// and abort under enforcing auth.
#[test]
fn require_once_each_dedups_by_address_value() {
    let env = Env::default();
    env.mock_all_auths();
    let shared = Address::generate(&env);
    let coop = Address::generate(&env);
    let s = world_in(
        &env,
        shared.clone(),
        coop.clone(),
        shared.clone(),
        1_500,
        2_000,
        common::WINDOW,
    );

    // The shape this test is actually exercising.
    let c = s.contract.get_commitment();
    assert_eq!(c.buyer, c.warehouse_operator);
    assert_ne!(c.buyer, c.cooperative);

    s.contract.lock();
    s.contract.flag_dispute(&shared);
    assert_eq!(s.status(), Status::Disputed);

    let invoke = MockAuthInvoke {
        contract: &s.contract.address,
        fn_name: "resolve_dispute",
        args: ().into_val(&env),
        sub_invokes: &[],
    };

    // Only the shared principal's signature: the distinct cooperative's
    // consent is still genuinely required.
    env.mock_auths(&[MockAuth {
        address: &shared,
        invoke: &invoke,
    }]);
    assert!(
        s.contract.try_resolve_dispute().is_err(),
        "a dedup that mistook two roles for two principals would let one signature through"
    );

    // One entry for the shared address + one for the cooperative is enough.
    env.mock_auths(&[
        MockAuth {
            address: &shared,
            invoke: &invoke,
        },
        MockAuth {
            address: &coop,
            invoke: &invoke,
        },
    ]);
    s.contract.resolve_dispute();
    assert_eq!(s.status(), Status::Locked);
}

/// The regression guard for the audit itself: if a future change ever compares
/// addresses through a text/bytes form, or brings `String`/`format!` into this
/// `#![no_std]` contract at all, this test fails. It is the machine-checkable
/// version of the table in this file's module doc.
#[test]
fn lib_source_never_stringifies_addresses() {
    const SRC: &str = include_str!("../src/lib.rs");
    for forbidden in [
        "to_string",
        "to_strkey",
        "from_str",
        "format!",
        "String",
        "as_bytes",
        "to_bytes",
        "hex::",
        "Display",
    ] {
        assert!(
            !SRC.contains(forbidden),
            "`{forbidden}` appeared in contracts/escrow/src/lib.rs — Address \
             comparisons must stay on soroban_sdk::Address PartialEq (#122). \
             If this is a legitimate use elsewhere, update \
             tests/address_equality.rs's audit table and this list together."
        );
    }

    // And the two legitimate hand-written comparisons are still the
    // PartialEq-based ones the audit describes (a rewrite to something else
    // should make a reviewer look at this file).
    assert!(SRC.contains(
        "flagger != c.buyer && flagger != c.cooperative && flagger != c.warehouse_operator"
    ));
    assert!(SRC.contains("addresses[..i].iter().all(|seen| seen != addr)"));
}
