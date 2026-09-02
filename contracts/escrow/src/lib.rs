//! HarvestLock escrow contract.
//!
//! One instance per commitment (PRD §4.8). Implements the state machine:
//!
//!   Draft -> Locked -> Advance1Released -> CheckpointPassed
//!         -> Advance2Released -> ReadyForDelivery -> Delivered -> Settled
//!
//! **Two-phase funding**, not one: `lock` escrows only the deposit
//! (`advance1_bps + advance2_bps` of `total_amount`) — not the whole
//! amount the way earlier versions of this contract did. The remainder
//! is escrowed later, at `fund_remainder`, once the cooperative signals
//! `ready_for_delivery`. This is a deliberate redesign, not a bug fix:
//! it's what makes "buyer default" a clean, contract-enforceable
//! deadline (`expire_remainder_window`) instead of something that has to
//! be asserted by a person. The mirror case — the cooperative never
//! delivering at all — gets its own deadline (`reclaim_on_nondelivery`),
//! since a contract can't detect "nobody showed up" the way it can
//! detect "nobody paid by the deadline."
//!
//! `cancel` is a mutual-consent unwind (PRD §7) reachable from any state
//! up through `ReadyForDelivery` — see its doc comment. `reassign_buyer`
//! (PRD §4.8) transfers the buyer position, three-party-consented, over
//! the same range. `Disputed` still exists in the `Status` enum because
//! the PRD's state machine names it, but no function transitions into it
//! yet — arbitrating a *contested* fault claim needs a mechanism this
//! contract doesn't have an answer for; `Defaulted` and `Forfeited` below
//! cover the *uncontested*, deadline-triggered cases only.
//!
//! Advance tranches use claimable-balance-equivalent semantics, built
//! natively in this contract rather than via classic Stellar
//! `ClaimableBalanceEntry` interop (HANDOFF.md explains why): opening a
//! tranche starts a claim window; the cooperative can `claim_*` within it;
//! the buyer can `reclaim_*` once it's passed and nobody claimed. Both
//! tranches must be resolved (claimed or expired) before `settle` will
//! run — see `settle`'s doc comment for why that's required rather than
//! `settle` inferring an outcome for whatever's left unresolved.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, token, Address, Env};

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Draft,
    Locked,
    Advance1Released,
    CheckpointPassed,
    Advance2Released,
    /// The cooperative has signaled intent to deliver — opens the
    /// remainder-payment window. See module docs.
    ReadyForDelivery,
    Delivered,
    Settled,
    Cancelled,
    /// Buyer failed to fund the remainder before its deadline. Uncontested
    /// by construction — the deadline either passed or it didn't.
    Defaulted,
    /// Cooperative never reached `Delivered` before the overall delivery
    /// deadline. Also uncontested by construction. Deliberately a
    /// separate variant from `Defaulted`, not a reuse of it — the two
    /// represent opposite parties' failure, and collapsing them into one
    /// status would make an already-settled commitment's history
    /// ambiguous about who actually failed to perform.
    Forfeited,
    Disputed,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidState = 3,
    InvalidBps = 4,
    ZeroAmount = 5,
    InvalidWindow = 6,
    NotYetOpened = 7,
    AlreadyClaimed = 8,
    AlreadyExpired = 9,
    ClaimWindowPassed = 10,
    ClaimWindowNotPassed = 11,
    /// A tranche is neither claimed nor expired yet — `settle` refuses to
    /// run until someone resolves it via `claim_advance_*` or
    /// `reclaim_advance_*`. See `settle`'s doc comment for why this is a
    /// hard requirement, not just a nicety.
    TrancheUnresolved = 12,
    RemainderAlreadyFunded = 13,
    RemainderWindowPassed = 14,
    RemainderWindowNotPassed = 15,
    /// `confirm_delivery` called while `ReadyForDelivery` but the buyer
    /// hasn't funded the remainder yet.
    RemainderNotFunded = 16,
    DeliveryDeadlineNotPassed = 17,
}

#[contracttype]
pub enum DataKey {
    Commitment,
}

/// Which advance tranche an operation applies to. Not part of the public
/// contract interface — `claim_advance_1`/`claim_advance_2` etc. are
/// separate exported functions that both call into the same internal
/// logic parameterized by this, so the claim/reclaim rules can't drift
/// between the two tranches.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tranche {
    One,
    Two,
}

#[contracttype]
#[derive(Clone)]
pub struct Commitment {
    pub buyer: Address,
    pub cooperative: Address,
    pub warehouse_operator: Address,
    pub token: Address,
    pub total_amount: i128,
    /// Basis points (of `total_amount`) released at lock-in. 0-10_000.
    pub advance1_bps: u32,
    /// Basis points released at the mid-season checkpoint. 0-10_000.
    /// `advance1_bps + advance2_bps` must not exceed 10_000 — enforced at
    /// `initialize`. Together these two define the *deposit* — the
    /// portion `lock` actually escrows; see module docs.
    pub advance2_bps: u32,
    /// How long the cooperative has to claim an advance once it opens,
    /// in seconds. Same window for both tranches — PRD doesn't call for
    /// different windows per tranche, so one shared value is the simpler
    /// choice until a reason to split them shows up.
    pub claim_window_secs: u64,
    /// How long the buyer has to fund the remainder once
    /// `ready_for_delivery` opens the window. Independent of
    /// `claim_window_secs` — this is a payment deadline, not a claim one.
    pub remainder_window_secs: u64,
    pub status: Status,
    pub created_at: u64,

    /// Absolute deadline (`created_at + delivery_window_secs`), computed
    /// once at `initialize` — unlike the tranche/remainder deadlines,
    /// this one doesn't depend on some other call happening first, so
    /// there's no "0 = unset" state for it.
    pub delivery_deadline: u64,

    /// 0 = not yet opened. Once `release_advance_1` runs, this is set to
    /// the ledger timestamp after which the cooperative can no longer
    /// claim and the buyer becomes eligible to reclaim.
    pub advance1_deadline: u64,
    pub advance1_claimed: bool,
    pub advance1_expired: bool,

    pub advance2_deadline: u64,
    pub advance2_claimed: bool,
    pub advance2_expired: bool,

    /// 0 = not yet opened. Set by `ready_for_delivery`.
    pub remainder_deadline: u64,
    pub remainder_funded: bool,
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Creates the commitment in `Draft`. No funds move yet.
    ///
    /// Requires the buyer's auth because they're the one committing to a
    /// future deposit — the cooperative and warehouse operator don't need
    /// to co-sign contract creation itself, only the state transitions
    /// that matter to them (`mark_checkpoint`, `confirm_delivery`).
    pub fn initialize(
        env: Env,
        buyer: Address,
        cooperative: Address,
        warehouse_operator: Address,
        token: Address,
        total_amount: i128,
        advance1_bps: u32,
        advance2_bps: u32,
        claim_window_secs: u64,
        remainder_window_secs: u64,
        delivery_window_secs: u64,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Commitment) {
            return Err(Error::AlreadyInitialized);
        }
        if total_amount <= 0 {
            return Err(Error::ZeroAmount);
        }
        if advance1_bps.saturating_add(advance2_bps) > 10_000 {
            return Err(Error::InvalidBps);
        }
        if claim_window_secs == 0 || remainder_window_secs == 0 || delivery_window_secs == 0 {
            return Err(Error::InvalidWindow);
        }
        buyer.require_auth();

        let created_at = env.ledger().timestamp();
        let commitment = Commitment {
            buyer,
            cooperative,
            warehouse_operator,
            token,
            total_amount,
            advance1_bps,
            advance2_bps,
            claim_window_secs,
            remainder_window_secs,
            status: Status::Draft,
            created_at,
            delivery_deadline: created_at + delivery_window_secs,
            advance1_deadline: 0,
            advance1_claimed: false,
            advance1_expired: false,
            advance2_deadline: 0,
            advance2_claimed: false,
            advance2_expired: false,
            remainder_deadline: 0,
            remainder_funded: false,
        };
        env.storage()
            .instance()
            .set(&DataKey::Commitment, &commitment);
        Ok(())
    }

    /// Draft -> Locked. Pulls the buyer's **deposit** into the contract —
    /// `advance1_bps + advance2_bps` of `total_amount`, not the full
    /// amount. See module docs for why this changed from earlier versions
    /// of this contract, which escrowed everything here.
    ///
    /// The buyer must have already approved this contract to transfer at
    /// least the deposit amount of `token` on their behalf (standard
    /// SEP-41 token `approve`), or this call fails at the token contract,
    /// not here.
    pub fn lock(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Draft {
            return Err(Error::InvalidState);
        }
        c.buyer.require_auth();

        let deposit = Self::deposit_amount(&c);

        // State is updated and saved *before* the external transfer
        // (checks-effects-interactions): if a reentrant call somehow
        // landed mid-transfer, it would see `Locked` already and be
        // rejected by the guard above, instead of being able to pull the
        // deposit a second time before this invocation's save lands.
        // The standard SAC/native token used here has no such hook — this
        // is defensive hardening against a future token type, not a
        // response to a demonstrated exploit against the current one.
        c.status = Status::Locked;
        Self::save(&env, &c);

        if deposit > 0 {
            token::Client::new(&env, &c.token).transfer(
                &c.buyer,
                env.current_contract_address(),
                &deposit,
            );
        }
        Ok(())
    }

    /// Locked -> Advance1Released. Opens tranche 1's claim window — does
    /// **not** move funds. The cooperative claims via `claim_advance_1`,
    /// or the buyer reclaims via `reclaim_advance_1` after the window
    /// passes unclaimed.
    ///
    /// Deliberately not auth-gated: it starts a clock, it doesn't move
    /// money or grant anyone anything they weren't already entitled to
    /// under the agreed terms.
    pub fn release_advance_1(env: Env) -> Result<(), Error> {
        Self::open_tranche(&env, Tranche::One, Status::Locked, Status::Advance1Released)
    }

    /// Cooperative claims tranche 1, if within the window.
    pub fn claim_advance_1(env: Env) -> Result<(), Error> {
        Self::claim_tranche(&env, Tranche::One)
    }

    /// Buyer reclaims tranche 1, if the window has passed unclaimed.
    pub fn reclaim_advance_1(env: Env) -> Result<(), Error> {
        Self::reclaim_tranche(&env, Tranche::One)
    }

    /// Advance1Released -> CheckpointPassed. Requires the warehouse
    /// operator's attestation — this is a judgment call, not a mechanical
    /// state advance, so unlike opening an advance tranche it *is*
    /// auth-gated. Independent of whether tranche 1 has actually been
    /// claimed yet — crop progress doesn't wait on paperwork.
    pub fn mark_checkpoint(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Advance1Released {
            return Err(Error::InvalidState);
        }
        c.warehouse_operator.require_auth();
        c.status = Status::CheckpointPassed;
        Self::save(&env, &c);
        Ok(())
    }

    /// CheckpointPassed -> Advance2Released. Same mechanics as
    /// `release_advance_1`, for tranche 2.
    pub fn release_advance_2(env: Env) -> Result<(), Error> {
        Self::open_tranche(
            &env,
            Tranche::Two,
            Status::CheckpointPassed,
            Status::Advance2Released,
        )
    }

    pub fn claim_advance_2(env: Env) -> Result<(), Error> {
        Self::claim_tranche(&env, Tranche::Two)
    }

    pub fn reclaim_advance_2(env: Env) -> Result<(), Error> {
        Self::reclaim_tranche(&env, Tranche::Two)
    }

    /// Advance2Released -> ReadyForDelivery. The cooperative signaling
    /// "setting out for delivery" — opens the remainder-payment window
    /// (`remainder_window_secs` from now). Auth-gated, unlike
    /// `release_advance_*`: this one has real stakes for the buyer (it
    /// starts the clock that can end in `Defaulted`), so it shouldn't be
    /// triggerable by just anyone.
    pub fn ready_for_delivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Advance2Released {
            return Err(Error::InvalidState);
        }
        c.cooperative.require_auth();
        c.remainder_deadline = env.ledger().timestamp() + c.remainder_window_secs;
        c.status = Status::ReadyForDelivery;
        Self::save(&env, &c);
        Ok(())
    }

    /// Buyer escrows `total_amount - deposit` — the second half of
    /// two-phase funding. Must happen within the window
    /// `ready_for_delivery` opened, or it's a default (see
    /// `expire_remainder_window`).
    pub fn fund_remainder(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::ReadyForDelivery {
            return Err(Error::InvalidState);
        }
        if c.remainder_funded {
            return Err(Error::RemainderAlreadyFunded);
        }
        if env.ledger().timestamp() > c.remainder_deadline {
            return Err(Error::RemainderWindowPassed);
        }
        c.buyer.require_auth();

        let remainder = c.total_amount - Self::deposit_amount(&c);

        // Effects before interaction — see `lock`'s comment for why.
        c.remainder_funded = true;
        Self::save(&env, &c);

        if remainder > 0 {
            token::Client::new(&env, &c.token).transfer(
                &c.buyer,
                env.current_contract_address(),
                &remainder,
            );
        }
        Ok(())
    }

    /// The buyer-default path: `remainder_deadline` passed with the
    /// remainder never funded. Sweeps whatever's currently escrowed (the
    /// deposit, or whatever of it the cooperative hasn't already claimed)
    /// to the cooperative and sets `Defaulted` — uncontested by
    /// construction, so unlike `cancel`/`reassign_buyer` this needs no
    /// consent from anyone.
    ///
    /// Permissionless, deliberately: the outcome (sweep to cooperative)
    /// doesn't depend on who calls it, only on whether the deadline has
    /// passed, the same reasoning `reclaim_tranche` would use if it
    /// weren't already scoped to benefit the buyer specifically. Anyone —
    /// including an off-chain watcher — can trigger it once the fact of
    /// the matter (deadline passed, unfunded) is true.
    pub fn expire_remainder_window(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::ReadyForDelivery {
            return Err(Error::InvalidState);
        }
        if c.remainder_funded {
            return Err(Error::RemainderAlreadyFunded);
        }
        if env.ledger().timestamp() <= c.remainder_deadline {
            return Err(Error::RemainderWindowNotPassed);
        }

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Defaulted;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
        }
        Ok(())
    }

    /// The seller-non-delivery path: `delivery_deadline` passed without
    /// `confirm_delivery` ever having run. Returns whatever's currently
    /// escrowed to the buyer and sets `Forfeited`.
    ///
    /// Buyer-gated, not permissionless — unlike `expire_remainder_window`,
    /// the beneficiary here is a specific party (the buyer) reclaiming
    /// what's rightfully theirs, the same reasoning `reclaim_tranche` uses.
    ///
    /// Not reachable from `Draft`: nothing's ever been escrowed at that
    /// point, so there's nothing to reclaim and no real event has
    /// occurred worth recording as a forfeiture.
    pub fn reclaim_on_nondelivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery => {}
            _ => return Err(Error::InvalidState),
        }
        if env.ledger().timestamp() <= c.delivery_deadline {
            return Err(Error::DeliveryDeadlineNotPassed);
        }
        c.buyer.require_auth();

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Forfeited;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
        }
        Ok(())
    }

    /// ReadyForDelivery -> Delivered. Warehouse-operator-attested, same
    /// reasoning as `mark_checkpoint`. Requires the remainder to already
    /// be funded — delivery shouldn't be confirmable while the buyer
    /// still owes money on the deal.
    ///
    /// This is a bare boolean gate for now — it does not yet read a
    /// warehouse receipt's quantity/grade or apply the PRD §7 shortfall
    /// adjustment schedule. See HANDOFF.md.
    pub fn confirm_delivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::ReadyForDelivery {
            return Err(Error::InvalidState);
        }
        if !c.remainder_funded {
            return Err(Error::RemainderNotFunded);
        }
        c.warehouse_operator.require_auth();
        c.status = Status::Delivered;
        Self::save(&env, &c);
        Ok(())
    }

    /// Delivered -> Settled. Pays the cooperative the contract's *entire*
    /// remaining token balance — not a recomputed bps figure, so it's
    /// automatically correct regardless of the exact claim/reclaim history
    /// (and regardless of the deposit/remainder split, both of which are
    /// already sitting in the contract's balance by this point).
    ///
    /// **Requires both advance tranches already resolved** (each either
    /// claimed or expired) before it will run. This was *not* the first
    /// design tried: an earlier version had `settle` silently sweep any
    /// still-unresolved tranche into the cooperative's payment. That's
    /// wrong — if a tranche's claim window had already lapsed but the
    /// buyer simply hadn't gotten around to calling `reclaim_advance_*`
    /// yet, that sweep would hand the buyer's already-vested reclaim right
    /// to the cooperative instead, with no adversarial timing required to
    /// trigger it, just an inactive buyer. Requiring explicit resolution
    /// first means every stroop's destination is always decided by an
    /// actual `claim`/`reclaim` call, never inferred by `settle`.
    ///
    /// No NGN/oracle conversion yet (PRD §4.2) — the full `total_amount`
    /// is treated as already being in the settlement asset.
    pub fn settle(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        if !c.advance1_claimed && !c.advance1_expired {
            return Err(Error::TrancheUnresolved);
        }
        if !c.advance2_claimed && !c.advance2_expired {
            return Err(Error::TrancheUnresolved);
        }

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Settled;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.cooperative, &balance);
        }
        Ok(())
    }

    /// Mutual cancellation — PRD §7: "Defined unwind: advance settled per
    /// agreed schedule, remaining escrow returned, no penalty, logged."
    /// Allowed from any pre-delivery state (`Draft` through
    /// `ReadyForDelivery`). Not from `Delivered` onward — at that point
    /// `settle` is the correct path, an unwind doesn't apply anymore.
    ///
    /// Requires **both** the buyer's and the cooperative's auth in the same
    /// call, since this is mutual, not unilateral — unlike `reclaim_*`,
    /// which is the buyer's unilateral right once a claim window lapses on
    /// its own.
    ///
    /// "Advance settled per agreed schedule, no penalty": whatever's
    /// already been claimed stays with the cooperative — this doesn't claw
    /// anything back. "Remaining escrow returned": whatever's still in the
    /// contract (zero, if nothing was ever locked) goes back to the buyer,
    /// via the same balance-based transfer `settle` uses, so it's correct
    /// regardless of claim/reclaim history or how much of the deposit vs.
    /// remainder had been funded. "Logged": the state transition and the
    /// transfer both land on the ledger from this call — no separate event
    /// needed, same as every other transition here.
    pub fn cancel(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Draft
            | Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery => {}
            _ => return Err(Error::InvalidState),
        }
        c.buyer.require_auth();
        c.cooperative.require_auth();

        // Effects before interaction — see `lock`'s comment for why.
        c.status = Status::Cancelled;
        Self::save(&env, &c);

        let token_client = token::Client::new(&env, &c.token);
        let balance = token_client.balance(&env.current_contract_address());
        if balance > 0 {
            token_client.transfer(&env.current_contract_address(), &c.buyer, &balance);
        }
        Ok(())
    }

    /// Buyer-position assignability (PRD §4.8): transfers `buyer` to
    /// `new_buyer`. Deliberately **not** a market — there's no order book,
    /// no listing, no on-chain price discovery here, just a novation of
    /// who holds the position. No funds move; this only ever rewrites who
    /// `buyer` refers to for every future `reclaim_*`/`cancel`/
    /// `fund_remainder` auth check.
    ///
    /// Requires **three** signatures in the same call, not two: the
    /// current buyer's (they're giving up the position), the
    /// cooperative's (PRD's explicit "with cooperative consent"), and the
    /// new buyer's. That third one isn't named in the PRD line this
    /// implements, but leaving it out would let the current buyer and
    /// cooperative saddle a third party with a position — including its
    /// obligations — without that party ever agreeing to take it on. Two
    /// consents where the PRD asked for two would be the smaller change;
    /// three is the safer one, and safety wins here.
    ///
    /// Reachable from the same states as `cancel` (`Draft` through
    /// `ReadyForDelivery`) and for the same reason: past `Delivered`,
    /// `buyer` no longer gates any remaining action (`settle` doesn't
    /// check it), so reassigning it after that point would be a no-op
    /// dressed up as a real transfer.
    pub fn reassign_buyer(env: Env, new_buyer: Address) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        match c.status {
            Status::Draft
            | Status::Locked
            | Status::Advance1Released
            | Status::CheckpointPassed
            | Status::Advance2Released
            | Status::ReadyForDelivery => {}
            _ => return Err(Error::InvalidState),
        }
        c.buyer.require_auth();
        c.cooperative.require_auth();
        new_buyer.require_auth();

        c.buyer = new_buyer;
        Self::save(&env, &c);
        Ok(())
    }

    pub fn get_status(env: Env) -> Result<Status, Error> {
        Ok(Self::load(&env)?.status)
    }

    pub fn get_commitment(env: Env) -> Result<Commitment, Error> {
        Self::load(&env)
    }

    // -- internal --

    fn load(env: &Env) -> Result<Commitment, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Commitment)
            .ok_or(Error::NotInitialized)
    }

    fn save(env: &Env, c: &Commitment) {
        env.storage().instance().set(&DataKey::Commitment, c);
    }

    fn bps_amount(total: i128, bps: u32) -> i128 {
        total * (bps as i128) / 10_000
    }

    /// The portion of `total_amount` that `lock` actually escrows —
    /// `advance1_bps + advance2_bps`. See module docs for why funding is
    /// split into this plus `fund_remainder`'s share instead of all at
    /// `lock`, the way earlier versions of this contract worked.
    fn deposit_amount(c: &Commitment) -> i128 {
        Self::bps_amount(
            c.total_amount,
            c.advance1_bps.saturating_add(c.advance2_bps),
        )
    }

    /// Field accessors for a given tranche, so `claim_tranche`/
    /// `reclaim_tranche`/`open_tranche` have one implementation shared by
    /// both `_1` and `_2` entry points instead of two copies that could
    /// silently drift apart.
    fn tranche_bps(c: &Commitment, t: Tranche) -> u32 {
        match t {
            Tranche::One => c.advance1_bps,
            Tranche::Two => c.advance2_bps,
        }
    }

    fn tranche_deadline(c: &Commitment, t: Tranche) -> u64 {
        match t {
            Tranche::One => c.advance1_deadline,
            Tranche::Two => c.advance2_deadline,
        }
    }

    fn tranche_claimed(c: &Commitment, t: Tranche) -> bool {
        match t {
            Tranche::One => c.advance1_claimed,
            Tranche::Two => c.advance2_claimed,
        }
    }

    fn tranche_expired(c: &Commitment, t: Tranche) -> bool {
        match t {
            Tranche::One => c.advance1_expired,
            Tranche::Two => c.advance2_expired,
        }
    }

    fn set_tranche_deadline(c: &mut Commitment, t: Tranche, deadline: u64) {
        match t {
            Tranche::One => c.advance1_deadline = deadline,
            Tranche::Two => c.advance2_deadline = deadline,
        }
    }

    fn set_tranche_claimed(c: &mut Commitment, t: Tranche) {
        match t {
            Tranche::One => c.advance1_claimed = true,
            Tranche::Two => c.advance2_claimed = true,
        }
    }

    fn set_tranche_expired(c: &mut Commitment, t: Tranche) {
        match t {
            Tranche::One => c.advance1_expired = true,
            Tranche::Two => c.advance2_expired = true,
        }
    }

    fn open_tranche(env: &Env, t: Tranche, required: Status, next: Status) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        if c.status != required {
            return Err(Error::InvalidState);
        }
        let deadline = env.ledger().timestamp() + c.claim_window_secs;
        Self::set_tranche_deadline(&mut c, t, deadline);
        c.status = next;
        Self::save(env, &c);
        Ok(())
    }

    fn claim_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        c.cooperative.require_auth();

        if Self::tranche_deadline(&c, t) == 0 {
            return Err(Error::NotYetOpened);
        }
        if Self::tranche_claimed(&c, t) {
            return Err(Error::AlreadyClaimed);
        }
        if Self::tranche_expired(&c, t) {
            return Err(Error::AlreadyExpired);
        }
        if env.ledger().timestamp() > Self::tranche_deadline(&c, t) {
            return Err(Error::ClaimWindowPassed);
        }

        let amount = Self::bps_amount(c.total_amount, Self::tranche_bps(&c, t));
        // Effects before interaction — see `lock`'s comment for why.
        Self::set_tranche_claimed(&mut c, t);
        Self::save(env, &c);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.cooperative,
                &amount,
            );
        }
        Ok(())
    }

    fn reclaim_tranche(env: &Env, t: Tranche) -> Result<(), Error> {
        let mut c = Self::load(env)?;
        c.buyer.require_auth();

        if Self::tranche_deadline(&c, t) == 0 {
            return Err(Error::NotYetOpened);
        }
        if Self::tranche_claimed(&c, t) {
            return Err(Error::AlreadyClaimed);
        }
        if Self::tranche_expired(&c, t) {
            return Err(Error::AlreadyExpired);
        }
        if env.ledger().timestamp() <= Self::tranche_deadline(&c, t) {
            return Err(Error::ClaimWindowNotPassed);
        }

        let amount = Self::bps_amount(c.total_amount, Self::tranche_bps(&c, t));
        // Effects before interaction — see `lock`'s comment for why.
        Self::set_tranche_expired(&mut c, t);
        Self::save(env, &c);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.buyer,
                &amount,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
