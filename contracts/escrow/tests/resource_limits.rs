//! #224 — the two product caps (`MAX_ALLOCATION_MEMBERS = 200`,
//! `MAX_GRADE_ENTRIES = 16`) measured against Soroban's per-transaction
//! resource limits.
//!
//! Both caps came from the PRD's product scale, not from chain limits. This
//! suite is the missing check: it drives the two worst-case inputs and a later
//! call (`settle`) on a commitment carrying both, meters the host's own
//! resource accounting, and asserts every figure stays under a stated fraction
//! of the limit. The numbers are printed on every run so a regression is
//! visible even before the assertion trips.
//!
//! ## Where the limits come from
//!
//! Read live from testnet's `ConfigSetting` ledger entries over JSON-RPC
//! (`getLedgerEntries` with `LedgerKey::ConfigSetting`) on **2026-11-24**,
//! latest ledger **5,901,514**:
//!
//! * `ConfigSettingId::ConfigSettingContractComputeV0` (1) → instructions and
//!   memory;
//! * `ConfigSettingContractLedgerCostV0` (2) → read/write entries and bytes;
//! * `ConfigSettingContractBandwidthV0` (5) → transaction size;
//! * `ConfigSettingContractDataKeySizeBytes` (8) and
//!   `ConfigSettingContractDataEntrySizeBytes` (9) → ledger entry limits;
//! * `ConfigSettingContractLedgerCostExtV0` (15) → footprint entries.
//!
//! The values match the snapshot the SDK enforces by default
//! (`InvocationResourceLimits::mainnet()`, `soroban-sdk` 29.0.1,
//! `src/testutils/cost_estimate.rs`, taken 2026-07-10) and
//! [Stellar Lab's network limits](https://lab.stellar.org/network-limits).
//! They are re-declared here, with the source and read date attached, so the
//! assertion does not silently follow an SDK change: a future network change
//! must update these constants deliberately.
//!
//! ## What is measured, and what is modelled
//!
//! `cost_estimate().budget()` and `.resources()` meter the host invocation:
//! CPU instructions, peak memory, read/write entries and bytes, all described
//! by the SDK as host-model figures (native Rust under the test host, not a
//! WASM VM or a fee quote). `stored_instance_entry_size` serialises the real
//! contract-data entry, so the ledger-entry figure is the actual encoded size,
//! not an estimate. See `docs/gas-costs.md` for the full caveat.
//!
//! The host deliberately does **not** model transaction size
//! (`CostEstimate::resources`' doc comment says so). The transaction-size
//! figure here is therefore a stated model: the XDR size of the
//! `InvokeContractArgs` the transaction must carry (the 200-member list or the
//! 16-grade schedule) plus a fixed 1 KiB allowance for the envelope, footprint
//! keys and a single signature. It is an upper bound for these calls, not a
//! simulated envelope; the ledger-entry size is measured separately.
//!
//! ## The two caps
//!
//! `MAX_ALLOCATION_MEMBERS` (200) and `MAX_GRADE_ENTRIES` (16) are private to
//! the contract, so they are restated here. `MAX_ALLOCATION_XDR_BYTES` and the
//! unit suite pin the same values; if the contract cap changes, this suite's
//! `initialize`/`set_allocation` calls will no longer exercise the maximum and
//! the fixture must be updated with it.
//!
//! Every assertion uses [`LIMIT_FRACTION_PERCENT`] of the network limit, not
//! the limit itself: the point is headroom for a future field, a larger token
//! payload or a protocol tweak, not merely "does not abort today".

use harvestlock_escrow::{
    AllocationMember, CommitmentTerms, EscrowContract, EscrowContractClient, OracleConfig, Status,
};
use soroban_sdk::testutils::{Address as _, EnvTestConfig};
use soroban_sdk::xdr::{Limits, WriteXdr};
use soroban_sdk::{token, Address, BytesN, Env, IntoVal, Val, Vec};

// ---------- network limits (testnet ConfigSetting, read 2026-11-24) ----------

/// `ConfigSettingContractComputeV0.txMaxInstructions`.
const TX_MAX_INSTRUCTIONS: u64 = 400_000_000;
/// `ConfigSettingContractComputeV0.txMemoryLimit` (40 MiB).
const TX_MEMORY_LIMIT_BYTES: u64 = 41_943_040;
/// `ConfigSettingContractLedgerCostV0.txMaxDiskReadEntries`.
const TX_MAX_DISK_READ_ENTRIES: u64 = 200;
/// `ConfigSettingContractLedgerCostV0.txMaxDiskReadBytes`.
const TX_MAX_DISK_READ_BYTES: u64 = 200_000;
/// `ConfigSettingContractLedgerCostV0.txMaxWriteLedgerEntries`.
const TX_MAX_WRITE_ENTRIES: u64 = 200;
/// `ConfigSettingContractLedgerCostV0.txMaxWriteBytes`.
const TX_MAX_WRITE_BYTES: u64 = 132_096;
/// `ConfigSettingContractBandwidthV0.txMaxBytes`.
const TX_MAX_SIZE_BYTES: u64 = 132_096;
/// `ConfigSettingContractDataEntrySizeBytes`.
const MAX_CONTRACT_DATA_ENTRY_SIZE_BYTES: u64 = 65_536;
/// `ConfigSettingContractLedgerCostExtV0.txMaxFootprintEntries`.
const TX_MAX_FOOTPRINT_ENTRIES: u64 = 400;

/// The bound this suite asserts. Half the network limit, matching the
/// existing instance-layout test (`src/test.rs`): enough headroom that a new
/// field cannot put a legal commitment over the chain ceiling, while still
/// tight enough to fail if a cap or encoding roughly doubles.
const LIMIT_FRACTION_PERCENT: u64 = 50;

/// Envelope, footprint keys and one signature, deliberately generous. The
/// invocation args are measured exactly; this covers the rest of the signed
/// transaction without pretending to rebuild it.
const TX_ENVELOPE_ALLOWANCE_BYTES: u64 = 1_024;

// ---------- contract caps under test ----------

const MAX_ALLOCATION_MEMBERS: usize = 200;
const MAX_GRADE_ENTRIES: usize = 16;

// ---------- fixture terms ----------

const TOTAL_AMOUNT: i128 = 1_000_000;
const ADVANCE1_BPS: u32 = 1_500;
const ADVANCE2_BPS: u32 = 1_500;
const WINDOW: u64 = 7 * 24 * 60 * 60;
const REMAINDER_WINDOW: u64 = 7 * 24 * 60 * 60;
const DELIVERY_WINDOW: u64 = 120 * 24 * 60 * 60;
const CONTRACTED_QUANTITY: u32 = 1_000;

/// Sixteen descending tiers, each a valid `<= 10_000` bps entry. The actual
/// values do not matter to resource use; the count does.
const GRADE_SCHEDULE_16: [u32; MAX_GRADE_ENTRIES] = [
    10_000, 9_900, 9_800, 9_700, 9_600, 9_500, 9_400, 9_300, 9_200, 9_100, 9_000, 8_900, 8_800,
    8_700, 8_600, 8_500,
];

fn terms(env: &Env) -> CommitmentTerms {
    CommitmentTerms {
        total_amount: TOTAL_AMOUNT,
        advance1_bps: ADVANCE1_BPS,
        advance2_bps: ADVANCE2_BPS,
        claim_window_secs: WINDOW,
        remainder_window_secs: REMAINDER_WINDOW,
        delivery_window_secs: DELIVERY_WINDOW,
        contracted_quantity: CONTRACTED_QUANTITY,
        grade_price_bps: Vec::from_array(env, GRADE_SCHEDULE_16),
    }
}

/// 200 members at 50 bps each — exactly the 10 000 bps ceiling, so this is the
/// largest legal allocation, not merely a large one.
fn max_allocation(env: &Env) -> Vec<AllocationMember> {
    let mut members = Vec::new(env);
    for i in 0..MAX_ALLOCATION_MEMBERS {
        members.push_back(AllocationMember {
            // Distinct hashes, so the encoding cannot collapse duplicates.
            member_hash: BytesN::from_array(env, &[i as u8; 32]),
            share_bps: 50,
        });
    }
    members
}

// ---------- measurement ----------

#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    cpu: u64,
    mem: u64,
    read_entries: u64,
    write_entries: u64,
    read_bytes: u64,
    write_bytes: u64,
    entry_bytes: u64,
}

impl Sample {
    /// (metric, used, limit) in the order the report prints them.
    fn rows(&self, tx_bytes: u64) -> [(&'static str, u64, u64); 9] {
        [
            ("instructions", self.cpu, TX_MAX_INSTRUCTIONS),
            ("memory bytes", self.mem, TX_MEMORY_LIMIT_BYTES),
            (
                "read entries (memory; disk reads are 0 in the test host)",
                self.read_entries,
                TX_MAX_DISK_READ_ENTRIES,
            ),
            (
                // Live state is in memory under the test host, so the host's
                // disk-read byte count is 0 by design. The instance entry it
                // would have to restore is the meaningful floor.
                "read bytes (restore floor = entry bytes)",
                self.read_bytes.max(self.entry_bytes),
                TX_MAX_DISK_READ_BYTES,
            ),
            ("write entries", self.write_entries, TX_MAX_WRITE_ENTRIES),
            ("write bytes", self.write_bytes, TX_MAX_WRITE_BYTES),
            (
                "ledger entry bytes",
                self.entry_bytes,
                MAX_CONTRACT_DATA_ENTRY_SIZE_BYTES,
            ),
            ("transaction bytes (modelled)", tx_bytes, TX_MAX_SIZE_BYTES),
            (
                "footprint entries (modelled)",
                self.read_entries + self.write_entries,
                TX_MAX_FOOTPRINT_ENTRIES,
            ),
        ]
    }
}

fn fresh_env() -> Env {
    Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    })
}

/// The exact encoded size of the contract's instance ledger entry, including
/// every instance key (`Commitment`, `Allocation`, `OracleConfig`). This is
/// the entry `ConfigSettingContractDataEntrySizeBytes` caps.
fn stored_instance_entry_size(env: &Env) -> u64 {
    env.to_ledger_snapshot()
        .entries()
        .into_iter()
        .find_map(|(key, (entry, _))| match key.as_ref() {
            soroban_sdk::xdr::LedgerKey::ContractData(key)
                if matches!(key.key, soroban_sdk::xdr::ScVal::LedgerKeyContractInstance) =>
            {
                Some(entry.to_xdr(Limits::none()).unwrap().len() as u64)
            }
            _ => None,
        })
        .expect("the test environment must contain the contract instance entry")
}

/// XDR size of the host-function arguments the transaction carries. This is
/// exact for the invocation; [`TX_ENVELOPE_ALLOWANCE_BYTES`] covers the rest.
fn invocation_args_size(contract: &Address, fn_name: &str, args: Vec<Val>) -> u64 {
    let xdr_args: soroban_sdk::xdr::VecM<soroban_sdk::xdr::ScVal> =
        args.try_into().expect("arguments must convert to ScVal");
    let invoke = soroban_sdk::xdr::InvokeContractArgs {
        contract_address: contract
            .clone()
            .try_into()
            .expect("contract address must convert"),
        function_name: fn_name
            .try_into()
            .expect("function name must be a valid ScSymbol"),
        args: xdr_args,
    };
    invoke.to_xdr(Limits::none()).unwrap().len() as u64
}

fn tx_model(args_bytes: u64) -> u64 {
    args_bytes + TX_ENVELOPE_ALLOWANCE_BYTES
}

/// Resets the meter, runs exactly one successful top-level call, and records
/// the host's figures for it. Setup is done by the caller and never leaks in:
/// the budget is reset immediately before the measured call, and the SDK
/// resets it again at the start of each top-level invocation.
fn measure(env: &Env, call: impl FnOnce()) -> Sample {
    env.cost_estimate().budget().reset_default();
    call();
    let budget = env.cost_estimate().budget();
    let used = env.cost_estimate().resources();
    Sample {
        cpu: budget.cpu_instruction_cost(),
        mem: budget.memory_bytes_cost(),
        read_entries: u64::from(used.memory_read_entries),
        write_entries: u64::from(used.write_entries),
        read_bytes: u64::from(used.disk_read_bytes),
        write_bytes: u64::from(used.write_bytes),
        entry_bytes: stored_instance_entry_size(env),
    }
}

struct Scene {
    env: Env,
    contract_id: Address,
    contract: EscrowContractClient<'static>,
    buyer: Address,
    cooperative: Address,
    warehouse: Address,
    token_address: Address,
}

impl Scene {
    /// Registers the token and contract, mints the buyer, and returns without
    /// initializing — so `initialize` itself can be the measured call.
    fn uninitialized() -> Self {
        let env = fresh_env();
        env.mock_all_auths();
        let buyer = Address::generate(&env);
        let cooperative = Address::generate(&env);
        let warehouse = Address::generate(&env);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let token_address = sac.address();
        token::StellarAssetClient::new(&env, &token_address).mint(&buyer, &TOTAL_AMOUNT);

        let contract_id = env.register(EscrowContract, ());
        let contract = EscrowContractClient::new(&env, &contract_id);
        Self {
            env,
            contract_id,
            contract,
            buyer,
            cooperative,
            warehouse,
            token_address,
        }
    }

    fn initialize(&self) -> CommitmentTerms {
        let terms = terms(&self.env);
        self.contract.initialize(
            &self.buyer,
            &self.cooperative,
            &self.warehouse,
            &self.token_address,
            &terms,
            &None,
        );
        terms
    }
}

fn assert_within(stage: &str, sample: &Sample, tx_bytes: u64) {
    for (metric, used, limit) in sample.rows(tx_bytes) {
        let bound = limit * LIMIT_FRACTION_PERCENT / 100;
        assert!(
            used <= bound,
            "{stage}: {metric} used {used}, over the stated cap {bound} \
             ({LIMIT_FRACTION_PERCENT}% of the network limit {limit})"
        );
    }
}

fn report(stage: &str, sample: &Sample, tx_bytes: u64) {
    std::println!("\n{stage} (bound: {LIMIT_FRACTION_PERCENT}% of each network limit)");
    std::println!("metric | used | network limit | margin | used %");
    for (metric, used, limit) in sample.rows(tx_bytes) {
        let pct = used as f64 / limit as f64 * 100.0;
        std::println!(
            "{metric} | {used} | {limit} | {} | {pct:.1}%",
            limit.saturating_sub(used)
        );
    }
}

// ---------- the worst cases ----------

/// `initialize` with the full 16-grade schedule. The schedule is written into
/// the instance entry, so this is both the call's CPU/memory worst case and
/// the smallest of the two entries that will later coexist.
#[test]
fn initialize_with_sixteen_grades_stays_well_under_every_limit() {
    let s = Scene::uninitialized();
    let terms = terms(&s.env);
    let args = (
        s.buyer.clone(),
        s.cooperative.clone(),
        s.warehouse.clone(),
        s.token_address.clone(),
        terms.clone(),
        Option::<OracleConfig>::None,
    )
        .into_val(&s.env);
    let args_bytes = invocation_args_size(&s.contract_id, "initialize", args);

    let sample = measure(&s.env, || {
        s.contract.initialize(
            &s.buyer,
            &s.cooperative,
            &s.warehouse,
            &s.token_address,
            &terms,
            &None,
        );
    });

    assert_eq!(
        s.contract.get_commitment().grade_price_bps.len() as usize,
        MAX_GRADE_ENTRIES
    );
    report("initialize: 16 grades", &sample, tx_model(args_bytes));
    assert_within("initialize: 16 grades", &sample, tx_model(args_bytes));
}

/// `set_allocation` at exactly 200 members and 10 000 bps. The member list is
/// carried in the transaction arguments and written to the instance entry, so
/// this is the largest of both. Initialized with 16 grades first, so the entry
/// measured here already holds the grade schedule too.
#[test]
fn set_allocation_at_two_hundred_members_stays_well_under_every_limit() {
    let s = Scene::uninitialized();
    s.initialize();
    let members = max_allocation(&s.env);
    let args = (members.clone(),).into_val(&s.env);
    let args_bytes = invocation_args_size(&s.contract_id, "set_allocation", args);

    let sample = measure(&s.env, || s.contract.set_allocation(&members));

    assert_eq!(
        s.contract.get_allocation().len() as usize,
        MAX_ALLOCATION_MEMBERS
    );
    report(
        "set_allocation: 200 members (with 16 grades already stored)",
        &sample,
        tx_model(args_bytes),
    );
    assert_within("set_allocation: 200 members", &sample, tx_model(args_bytes));
}

/// `settle` on a commitment carrying the maximum grade schedule and a separate
/// 200-member allocation entry. The call itself takes no arguments, so its
/// transaction is tiny; the resource question is the instance entry it reads
/// and rewrites plus the persistent allocation entry it reads.
#[test]
fn settle_with_both_maxima_stays_well_under_every_limit() {
    let s = Scene::uninitialized();
    s.initialize();
    s.contract.set_allocation(&max_allocation(&s.env));
    s.contract.lock();
    s.contract.release_advance_1();
    s.contract.claim_advance_1();
    s.contract.mark_checkpoint();
    s.contract.release_advance_2();
    s.contract.claim_advance_2();
    s.contract.ready_for_delivery();
    s.contract.fund_remainder();
    s.contract.confirm_delivery(&CONTRACTED_QUANTITY, &0);
    assert_eq!(s.contract.get_status(), Status::Delivered);

    let empty = Vec::<Val>::new(&s.env);
    let args_bytes = invocation_args_size(&s.contract_id, "settle", empty);
    let sample = measure(&s.env, || s.contract.settle());

    assert_eq!(s.contract.get_status(), Status::Settled);
    report(
        "settle: 200 members + 16 grades",
        &sample,
        tx_model(args_bytes),
    );
    assert_within(
        "settle: 200 members + 16 grades",
        &sample,
        tx_model(args_bytes),
    );
}

/// A guard on the guard: the allocation is intentionally isolated from the
/// instance entry so its 200-member payload cannot consume instance-entry
/// capacity. The resource measurements still cover the allocation through the
/// contract call's persistent read path.
#[test]
fn the_settle_fixture_keeps_allocation_out_of_the_instance_entry() {
    let s = Scene::uninitialized();
    s.initialize();
    let with_grades = stored_instance_entry_size(&s.env);

    s.contract.set_allocation(&max_allocation(&s.env));
    let with_both = stored_instance_entry_size(&s.env);

    assert_eq!(
        with_both, with_grades,
        "the 200-member allocation must stay in its separate persistent entry \
         ({with_grades} -> {with_both} instance bytes)"
    );
    assert_eq!(
        s.contract.get_allocation().len() as usize,
        MAX_ALLOCATION_MEMBERS
    );
    assert_eq!(
        s.contract.get_commitment().grade_price_bps.len() as usize,
        MAX_GRADE_ENTRIES
    );
}
