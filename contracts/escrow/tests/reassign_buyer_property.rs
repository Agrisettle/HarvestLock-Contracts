//! Differential lifecycle checks for buyer reassignment: reassignment may
//! change the buyer who receives a transfer, but not the transfer amount or
//! the cooperative's receipts.

#[path = "common/mod.rs"]
mod common;

use common::{world_in, World, DELIVERY_WINDOW, TOTAL_AMOUNT};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use soroban_sdk::testutils::{Address as _, EnvTestConfig};
use soroban_sdk::{Address, Env};

struct Receipts {
    original_buyer: i128,
    incoming_buyer: i128,
    cooperative: i128,
}

fn exercise_lifecycle(
    world: &World,
    incoming_buyer: &Address,
    reassign: bool,
    scenario: u8,
    advance1_bps: u32,
    advance2_bps: u32,
    delivered_quantity: u32,
    grade_index: u32,
) -> Receipts {
    let deposit = world.bps(advance1_bps + advance2_bps);
    let remainder = TOTAL_AMOUNT - deposit;
    let funded_remainder = matches!(scenario, 3 | 4).then_some(remainder).unwrap_or(0);

    match scenario {
        0 => {
            let deadline = world.locked_tranche1_open();
            if reassign {
                world.contract.reassign_buyer(incoming_buyer);
            }
            world.set_time(deadline + 1);
            world.contract.reclaim_advance_1();
        }
        1 => {
            world.contract.lock();
            world.contract.release_advance_1();
            world.contract.claim_advance_1();
            world.contract.mark_checkpoint();
            world.contract.release_advance_2();
            let deadline = world.contract.get_commitment().advance2_deadline;
            if reassign {
                world.contract.reassign_buyer(incoming_buyer);
            }
            world.set_time(deadline + 1);
            world.contract.reclaim_advance_2();
        }
        2 => {
            world.contract.lock();
            world.contract.release_advance_1();
            if reassign {
                world.contract.reassign_buyer(incoming_buyer);
            }
            world.contract.cancel();
        }
        3 | 4 => {
            world.contract.lock();
            world.contract.release_advance_1();
            world.contract.claim_advance_1();
            world.contract.mark_checkpoint();
            world.contract.release_advance_2();
            world.contract.claim_advance_2();
            world.contract.ready_for_delivery();
            if reassign {
                world.contract.reassign_buyer(incoming_buyer);
            }
            world.contract.fund_remainder();
            if scenario == 3 {
                world.contract.cancel();
            } else {
                world
                    .contract
                    .confirm_delivery(&delivered_quantity, &grade_index);
                world.contract.settle();
            }
        }
        5 => {
            world.contract.lock();
            world.contract.release_advance_1();
            world.contract.claim_advance_1();
            world.contract.mark_checkpoint();
            world.contract.release_advance_2();
            if reassign {
                world.contract.reassign_buyer(incoming_buyer);
            }
            world.set_time(DELIVERY_WINDOW + 1);
            world.contract.reclaim_on_nondelivery();
        }
        6 => {
            world.contract.lock();
            world.contract.release_advance_1();
            world.contract.mark_checkpoint();
            world.contract.release_advance_2();
            world.contract.ready_for_delivery();
            let deadline = world.contract.get_commitment().remainder_deadline;
            if reassign {
                world.contract.reassign_buyer(incoming_buyer);
            }
            world.set_time(deadline + 1);
            world.contract.expire_remainder_window();
        }
        _ => unreachable!("scenario is generated in 0..7"),
    }

    let original_buyer_delta = world.token.balance(&world.buyer) - TOTAL_AMOUNT;
    let incoming_buyer_delta = world.token.balance(incoming_buyer) - TOTAL_AMOUNT;
    let incoming_funding = if reassign { funded_remainder } else { 0 };
    let original_funding = if reassign { 0 } else { funded_remainder };

    Receipts {
        original_buyer: original_buyer_delta + deposit + original_funding,
        incoming_buyer: incoming_buyer_delta + incoming_funding,
        cooperative: world.token.balance(&world.cooperative),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        rng_seed: RngSeed::Fixed(0x303_304_305),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn reassign_changes_the_buyer_recipient_without_changing_amounts_or_cooperative_receipts(
        advance1_bps in 500u32..3_500,
        advance2_bps in 500u32..3_500,
        delivered_quantity in 0u32..999,
        grade_index in 0u32..3,
    ) {
        for scenario in 0u8..7 {
            // Per-case footprint snapshots would duplicate large randomized
            // traces; this property asserts payouts and balances directly.
            let env = Env::new_with_config(EnvTestConfig {
                capture_snapshot_at_drop: false,
            });
            env.mock_all_auths();
            let buyer = Address::generate(&env);
            let cooperative = Address::generate(&env);
            let warehouse = Address::generate(&env);
            let baseline = world_in(
                &env,
                buyer.clone(),
                cooperative.clone(),
                warehouse.clone(),
                advance1_bps,
                advance2_bps,
                common::WINDOW,
            );
            let reassigned = world_in(
                &env,
                buyer,
                cooperative,
                warehouse,
                advance1_bps,
                advance2_bps,
                common::WINDOW,
            );
            let incoming_buyer = Address::generate(&env);
            baseline.token_admin.mint(&incoming_buyer, &TOTAL_AMOUNT);
            reassigned.token_admin.mint(&incoming_buyer, &TOTAL_AMOUNT);

            let unchanged = exercise_lifecycle(
                &baseline,
                &incoming_buyer,
                false,
                scenario,
                advance1_bps,
                advance2_bps,
                delivered_quantity,
                grade_index,
            );
            // Both worlds share one ledger clock and were initialized at 0;
            // rewind it so the second lifecycle starts inside its delivery
            // window (`lock` refuses after the deadline, #277).
            reassigned.set_time(0);
            let changed = exercise_lifecycle(
                &reassigned,
                &incoming_buyer,
                true,
                scenario,
                advance1_bps,
                advance2_bps,
                delivered_quantity,
                grade_index,
            );

            prop_assert_eq!(unchanged.original_buyer, changed.incoming_buyer);
            prop_assert_eq!(unchanged.cooperative, changed.cooperative);
            prop_assert_eq!(unchanged.incoming_buyer, 0);
            prop_assert_eq!(changed.original_buyer, 0);
        }
    }
}
