use harvestlock_escrow::{AllocationMember, CommitmentTerms, OracleConfig, Status};
use soroban_sdk::xdr::{Limits, ScVal, WriteXdr};
use soroban_sdk::{
    testutils::Ledger as _, Address, Env, IntoVal, String, Symbol, TryFromVal, Val, Vec as SdkVec,
};

use std::fs;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/xdr"))
}

fn to_b64(scval: &ScVal) -> std::string::String {
    scval.to_xdr_base64(Limits::none()).unwrap()
}

fn write_fixture(name: &str, content: &str) {
    let dir = fixture_dir();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), content).unwrap();
}

/// Regenerates the canonical XDR fixtures under `tests/fixtures/xdr/`.
///
/// These are the byte-level cross-check artifacts for issue #268: the Rust
/// side writes canonical `ScVal` XDR (base64) for a handful of
/// representative values, and the TypeScript bindings test in
/// `bindings/typescript` decodes them and re-encodes the same logical values
/// through the generated `contract.Spec`, asserting byte equality in both
/// directions. Re-run with:
///
///     cargo test --test test_xdr_scratch -- --nocapture
#[test]
fn test_generate_xdr_fixtures() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1700000000);

    // Shared representative addresses (match the comparison doc, #64).
    let buyer = Address::from_string(&String::from_str(
        &env,
        "GD5RFZ4IADL7KS3J2AMCXNUDT3G7V7U4KMTX54UWXO7J3MTICWBS2LSI",
    ));
    let cooperative = Address::from_string(&String::from_str(
        &env,
        "GA6BHLUH7BC65ZOBZ2OPAXGTNVM7ZFPTACJ5I6FTEZ2P7FCAKUJFD3SC",
    ));
    let warehouse = Address::from_string(&String::from_str(
        &env,
        "GA3FPIF3JAI5AH2PHBYO3AI4QXOJEA5NP6Q7N7QLIX2YFWDUR7SXLWQ3",
    ));
    let token = Address::from_string(&String::from_str(
        &env,
        "CAVWBS5USXL5WRIB2HBGKBEABQZ6AT6YH7P4JO452WWEMWPWER5T6RDF",
    ));
    let oracle_contract = Address::from_string(&String::from_str(
        &env,
        "CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W",
    ));

    let oracle_config = OracleConfig {
        oracle_contract: oracle_contract.clone(),
        price_asset: Symbol::new(&env, "GBP"),
        max_age_secs: 300,
        denominated_amount: 500000,
    };

    let grade_prices = SdkVec::from_array(&env, [10000u32, 9000u32, 7500u32]);

    // ------------------------------------------------------------------
    // 1. Full initialize argument vector, WITH oracle_config.
    // ------------------------------------------------------------------
    let terms = CommitmentTerms {
        total_amount: 100_000_000,
        advance1_bps: 1500,
        advance2_bps: 1500,
        claim_window_secs: 604800,
        remainder_window_secs: 604800,
        delivery_window_secs: 10368000,
        contracted_quantity: 1000,
        grade_price_bps: grade_prices.clone(),
    };

    let args_with_oracle: [Val; 6] = [
        buyer.clone().into_val(&env),
        cooperative.clone().into_val(&env),
        warehouse.clone().into_val(&env),
        token.clone().into_val(&env),
        terms.clone().into_val(&env),
        Some(oracle_config.clone()).into_val(&env),
    ];
    let b64_with: std::vec::Vec<std::string::String> = args_with_oracle
        .iter()
        .map(|v| {
            let scval = ScVal::try_from_val(&env, v).unwrap();
            to_b64(&scval)
        })
        .collect();
    let json_with = format!(
        "{{\"function\":\"initialize\",\"oracle_config\":true,\"args\":[{}]}}",
        b64_with
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",")
    );
    write_fixture("initialize_with_oracle.json", &json_with);

    // ------------------------------------------------------------------
    // 2. Full initialize argument vector, WITHOUT oracle_config.
    // ------------------------------------------------------------------
    let args_without: [Val; 6] = [
        buyer.clone().into_val(&env),
        cooperative.clone().into_val(&env),
        warehouse.clone().into_val(&env),
        token.clone().into_val(&env),
        terms.clone().into_val(&env),
        Option::<OracleConfig>::None.into_val(&env),
    ];
    let b64_without: std::vec::Vec<std::string::String> = args_without
        .iter()
        .map(|v| {
            let scval = ScVal::try_from_val(&env, v).unwrap();
            to_b64(&scval)
        })
        .collect();
    let json_without = format!(
        "{{\"function\":\"initialize\",\"oracle_config\":false,\"args\":[{}]}}",
        b64_without
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",")
    );
    write_fixture("initialize_without_oracle.json", &json_without);

    // ------------------------------------------------------------------
    // 3. Vec<AllocationMember>.
    // ------------------------------------------------------------------
    let member_hash_a = [0x11u8; 32];
    let member_hash_b = [0x22u8; 32];
    let members = SdkVec::from_array(
        &env,
        [
            AllocationMember {
                member_hash: soroban_sdk::BytesN::from_array(&env, &member_hash_a),
                share_bps: 6000,
            },
            AllocationMember {
                member_hash: soroban_sdk::BytesN::from_array(&env, &member_hash_b),
                share_bps: 4000,
            },
        ],
    );
    let scval_members = ScVal::try_from_val(
        &env,
        &<SdkVec<AllocationMember> as IntoVal<Env, Val>>::into_val(&members, &env),
    )
    .unwrap();
    let json_members = format!(
        "{{\"function\":\"set_allocation\",\"arg\":\"members\",\"xdr\":\"{}\"}}",
        to_b64(&scval_members)
    );
    write_fixture("allocation_members.json", &json_members);

    // ------------------------------------------------------------------
    // 4. Commitment read back mid-lifecycle.
    // ------------------------------------------------------------------
    // Manually constructed to keep addresses fixed and avoid token transfers,
    // while representing a commitment that has reached `Delivered`.
    let commitment = harvestlock_escrow::Commitment {
        buyer: buyer.clone(),
        cooperative: cooperative.clone(),
        warehouse_operator: warehouse.clone(),
        token: token.clone(),
        total_amount: 100_000_000,
        advance1_bps: 1500,
        advance2_bps: 1500,
        claim_window_secs: 604800,
        remainder_window_secs: 604800,
        status: Status::Delivered,
        created_at: 1700000000,
        delivery_deadline: 1700000000 + 10368000,
        advance1_deadline: 1700000600,
        advance1_claimed: true,
        advance1_expired: false,
        advance2_deadline: 1700001200,
        advance2_claimed: true,
        advance2_expired: false,
        remainder_deadline: 1700001800,
        remainder_funded: true,
        contracted_quantity: 1000,
        grade_price_bps: grade_prices.clone(),
        delivered_quantity: 950,
        grade_index: 1,
        settlement_bps: 9000,
        fx_resolved: true,
        fx_adjusted_total: 90_000_000,
        fx_shortfall_amount: 5_000_000,
        fx_shortfall_funded: true,
        fx_shortfall_deadline: 1700002400,
        fx_quote_stale: false,
        dispute_pre_status: Status::Draft,
        dispute_deadline: 0,
        dispute_count: 0,
    };
    let scval_commitment = ScVal::try_from_val(
        &env,
        &<harvestlock_escrow::Commitment as IntoVal<Env, Val>>::into_val(&commitment, &env),
    )
    .unwrap();
    let json_commitment = format!(
        "{{\"function\":\"get_commitment\",\"xdr\":\"{}\"}}",
        to_b64(&scval_commitment)
    );
    write_fixture("commitment_mid_lifecycle.json", &json_commitment);
}
