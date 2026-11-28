//! # Settlement-asset issuer powers over a live escrow.
//!
//! Every live verification of this contract used the native XLM SAC, which has
//! no issuer controls. A real pilot settles in an issued stablecoin, where the
//! issuer's `AUTH_REVOCABLE` / `AUTH_CLAWBACK_ENABLED` flags sit *above*
//! anything the escrow does. This suite uses a classic-asset SAC
//! (`register_stellar_asset_contract_v2`) with both flags enabled before any
//! escrow balance exists, and pins what each issuer power does to a commitment
//! that is already holding funds.
//!
//! Three scenarios, three conclusions:
//!
//! (a) **Deauthorizing the escrow contract address** freezes every call that
//!     moves tokens out of (or into) escrow: claims, reclaims, `cancel`,
//!     `fund_remainder`, `settle`, and both expiry sweeps abort at the token
//!     layer. State transitions that move no tokens still work, so the
//!     commitment can walk to `ReadyForDelivery` — but it cannot reach any
//!     terminal status until the issuer re-authorizes. The balance is not lost;
//!     it is frozen.
//! (b) **Deauthorizing the cooperative** makes `claim_advance_1` abort, while
//!     the buyer's `reclaim_advance_1` still works (it pays the buyer, not the
//!     cooperative). If the deal nonetheless reaches `Delivered`, `settle`
//!     reverts forever because its payout to the cooperative is refused, and
//!     `cancel` is not available from `Delivered` — the commitment is stuck in
//!     a non-terminal state until the issuer re-authorizes.
//! (c) **Clawing back part of the escrow's balance before `settle`** does not
//!     brick settlement: `settle` is deliberately balance-based, so it still
//!     succeeds and splits whatever is actually left. At full delivery the
//!     cooperative is the residual claimant and absorbs the entire clawback;
//!     the buyer's refund is `balance - coop_payment`, which is zero either
//!     way. The recorded `total_amount` is not rewritten, so the tokens simply
//!     vanish from the deal's accounting.
//!
//! See `docs/threat-model.md` entry 31 and
//! `docs/griefing-and-inaction.md` §4.5 for the written-down verdicts.

#[path = "common/mod.rs"]
mod common;

use common::{world, World};
use harvestlock_escrow::{Error, Status};
use soroban_sdk::testutils::IssuerFlags;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{ConversionError, InvokeError};

/// The SAC's issuer-control client for this world's settlement asset.
fn asset_admin(s: &World) -> StellarAssetClient<'static> {
    StellarAssetClient::new(&s.env, &s.token.address)
}

/// Enables both issuer powers *before* the escrow contract's balance is
/// created (at `lock`). The clawback-enabled bit on a contract balance is
/// latched when that balance first exists, so ordering matters here.
fn enable_issuer_controls(s: &World) -> StellarAssetClient<'static> {
    s.issuer.set_flag(IssuerFlags::RevocableFlag);
    s.issuer.set_flag(IssuerFlags::ClawbackEnabledFlag);
    asset_admin(s)
}

/// Did an escrow call fail?
///
/// Deliberately not an `assert_eq!(…, Err(Ok(Error::X)))`. The SAC reports a
/// deauthorized balance as its own contract error 11
/// (`BalanceDeauthorizedError`), and the generated escrow client decodes nested
/// contract-error codes in the same numeric space, so a frozen balance surfaces
/// as `Err(Ok(Error::ClaimWindowNotPassed))` — escrow discriminant 11. That is
/// a decoy, not an escrow guard firing. What these tests can pin honestly is
/// the behavior: the call fails, the whole invocation rolls back, and the very
/// same call succeeds once the issuer re-authorizes (asserted at the end of
/// each scenario). The state/balance checks after every failure are the real
/// assertion.
fn failed(result: Result<Result<(), ConversionError>, Result<Error, InvokeError>>) -> bool {
    result.is_err()
}

fn is_terminal(status: Status) -> bool {
    matches!(
        status,
        Status::Settled | Status::Cancelled | Status::Defaulted | Status::Forfeited
    )
}

// ---------- (a) the issuer deauthorizes the escrow contract ----------

#[test]
fn deauthorizing_the_escrow_address_freezes_every_value_moving_path() {
    let s = world(1_500, 2_000); // 15% + 20% deposit == 350_000
    let admin = enable_issuer_controls(&s);

    s.contract.lock();
    let deposit = s.bps(3_500);
    assert_eq!(s.token.balance(&s.contract.address), deposit);

    // The freeze: the escrow contract may hold the tokens but no longer move
    // them. The balance is untouched — this is a freeze, not a seizure.
    admin.set_authorized(&s.contract.address, &false);
    assert!(!admin.authorized(&s.contract.address));
    assert_eq!(s.token.balance(&s.contract.address), deposit);

    // Opening a window moves no tokens, so it is unaffected.
    s.contract.release_advance_1();
    assert_eq!(s.status(), Status::Advance1Released);

    // Every escrow -> party transfer now aborts at the token layer.
    let claim = s.contract.try_claim_advance_1();
    assert!(
        failed(claim),
        "a deauthorized escrow balance must not pay out"
    );
    let c = s.contract.get_commitment();
    assert!(!c.advance1_claimed, "the aborted claim wrote nothing");
    assert_eq!(s.token.balance(&s.cooperative), 0);
    assert_eq!(s.token.balance(&s.contract.address), deposit);

    s.advance(common::WINDOW + 1);
    assert!(failed(s.contract.try_reclaim_advance_1()));
    assert_eq!(s.token.balance(&s.contract.address), deposit);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount - deposit);

    // The mutual unwind cannot spend from the frozen contract either.
    assert!(failed(s.contract.try_cancel()));
    assert_eq!(s.status(), Status::Advance1Released);

    // Tokenless transitions still advance the state machine.
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.ready_for_delivery();
    assert_eq!(s.status(), Status::ReadyForDelivery);

    // Funding the remainder is a buyer -> escrow transfer, so the frozen
    // *recipient* blocks it too.
    assert!(failed(s.contract.try_fund_remainder()));
    assert!(!s.contract.get_commitment().remainder_funded);

    // The permissionless buyer-default sweep cannot pay the cooperative from
    // the frozen balance...
    s.advance(common::REMAINDER_WINDOW + 1);
    assert!(failed(s.contract.try_expire_remainder_window()));
    assert_eq!(s.status(), Status::ReadyForDelivery);

    // ...and the buyer's own non-delivery exit cannot pay the buyer either.
    s.advance(common::DELIVERY_WINDOW + 1);
    assert!(failed(s.contract.try_reclaim_on_nondelivery()));
    assert_eq!(s.status(), Status::ReadyForDelivery);
    assert!(!is_terminal(s.status()), "no terminal state is reachable");
    assert_eq!(s.token.balance(&s.contract.address), deposit);

    // Liveness is external: only the issuer's re-authorization restores it.
    admin.set_authorized(&s.contract.address, &true);
    s.contract.cancel();
    assert_eq!(s.status(), Status::Cancelled);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(s.token.balance(&s.buyer), s.total_amount);
}

#[test]
fn deauthorizing_the_escrow_address_during_delivered_blocks_settle() {
    let s = world(1_500, 2_000);
    let admin = enable_issuer_controls(&s);
    s.drive_to_delivered();
    assert_eq!(s.bps(6_500), s.token.balance(&s.contract.address));

    admin.set_authorized(&s.contract.address, &false);

    assert!(failed(s.contract.try_settle()));
    assert_eq!(s.status(), Status::Delivered);
    assert_eq!(s.token.balance(&s.contract.address), s.bps(6_500));
    assert_eq!(s.token.balance(&s.cooperative), s.bps(3_500));
    // No other terminal path exists from Delivered.
    assert_eq!(s.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    assert!(!is_terminal(s.status()));
}

// ---------- (b) the issuer deauthorizes the cooperative ----------

#[test]
fn deauthorizing_the_cooperative_blocks_claim_advance_1_but_not_the_buyers_reclaim() {
    let s = world(1_500, 2_000);
    let admin = enable_issuer_controls(&s);
    // A real cooperative holds a token balance (and therefore a trustline)
    // before the deal; a deauthorization needs that trustline to exist.
    admin.mint(&s.cooperative, &1);

    s.contract.lock();
    s.contract.release_advance_1();
    admin.set_authorized(&s.cooperative, &false);

    assert!(failed(s.contract.try_claim_advance_1()));
    assert!(!s.contract.get_commitment().advance1_claimed);
    assert_eq!(s.token.balance(&s.cooperative), 1);
    assert_eq!(s.token.balance(&s.contract.address), s.bps(3_500));

    // The buyer's reclaim pays the buyer, not the deauthorized cooperative —
    // it is the one exit that still works while the cooperative is frozen.
    s.advance(common::WINDOW + 1);
    s.contract.reclaim_advance_1();
    assert!(s.contract.get_commitment().advance1_expired);
    assert_eq!(s.token.balance(&s.buyer), s.bps(6_500) + s.bps(1_500));
    assert_eq!(s.token.balance(&s.contract.address), s.bps(2_000));
}

#[test]
fn deauthorizing_the_cooperative_bricks_settle_in_delivered_until_reauthorized() {
    let s = world(1_500, 2_000);
    let admin = enable_issuer_controls(&s);
    admin.mint(&s.cooperative, &1);
    admin.set_authorized(&s.cooperative, &false);

    // Walk to Delivered without ever paying the cooperative: both tranches
    // lapse and the buyer reclaims them, the buyer funds the remainder, and
    // the operator confirms delivery. This is a legitimate deal shape — no
    // escrow guard objects to a deauthorized payee.
    s.contract.lock();
    s.contract.release_advance_1();
    assert!(failed(s.contract.try_claim_advance_1()));
    s.advance(common::WINDOW + 1);
    s.contract.reclaim_advance_1();

    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    assert!(failed(s.contract.try_claim_advance_2()));
    s.advance(common::WINDOW + 1);
    s.contract.reclaim_advance_2();

    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract
        .confirm_delivery(&common::CONTRACTED_QUANTITY, &0);
    assert_eq!(s.status(), Status::Delivered);
    let remainder = s.bps(6_500);

    // settle reverts on the payout to the cooperative. Soroban's atomicity
    // means the status write before the transfer is rolled back too.
    assert!(failed(s.contract.try_settle()));
    assert_eq!(s.status(), Status::Delivered);
    assert_eq!(s.token.balance(&s.contract.address), remainder);
    assert_eq!(s.token.balance(&s.cooperative), 1);

    // Every terminal path from Delivered runs through settle, and cancel is
    // explicitly unavailable here — so the commitment has no on-chain exit.
    assert_eq!(s.contract.try_cancel(), Err(Ok(Error::InvalidState)));
    assert!(!is_terminal(s.status()));

    // Only an issuer action restores liveness. With both advances expired,
    // settle owes the cooperative the whole remaining balance and the buyer
    // gets no refund; the deal reaches Settled.
    admin.set_authorized(&s.cooperative, &true);
    s.contract.settle();
    assert_eq!(s.status(), Status::Settled);
    assert_eq!(s.token.balance(&s.contract.address), 0);
    assert_eq!(s.token.balance(&s.cooperative), 1 + remainder);
}

// ---------- (c) the issuer claws back part of the escrow balance ----------

#[test]
fn clawback_before_settle_is_absorbed_by_the_cooperative_not_the_buyer() {
    let s = world(1_500, 2_000); // 15% + 20% advances, 65% remainder
    let admin = enable_issuer_controls(&s);

    s.drive_to_delivered();
    // 150_000 + 200_000 already paid to the cooperative, 650_000 still escrowed.
    assert_eq!(s.token.balance(&s.cooperative), s.bps(3_500));
    assert_eq!(s.token.balance(&s.contract.address), s.bps(6_500));

    let clawed_back = 200_000i128;
    admin.clawback(&s.contract.address, &clawed_back);
    assert_eq!(
        s.token.balance(&s.contract.address),
        s.bps(6_500) - clawed_back
    );

    // Settlement still succeeds: the split is computed from the actual
    // balance, not from the recorded total_amount. owed_from_remainder is
    // 650_000, but only 450_000 exists, so the cooperative's payment is
    // capped at the balance and the buyer's refund is 0 either way.
    s.contract.settle();
    assert_eq!(s.status(), Status::Settled);

    let expected_coop = s.bps(3_500) + (s.bps(6_500) - clawed_back);
    assert_eq!(s.token.balance(&s.cooperative), expected_coop);
    assert_eq!(s.token.balance(&s.buyer), 0);
    assert_eq!(s.token.balance(&s.contract.address), 0);

    // What was actually paid out is exactly total_amount - clawback: the
    // burned tokens leave the deal's accounting, while the recorded terms
    // still claim full delivery at full value.
    assert_eq!(
        s.token.balance(&s.cooperative) + s.token.balance(&s.buyer),
        s.total_amount - clawed_back
    );
    assert_eq!(s.contract.get_commitment().settlement_bps, 10_000);
}
