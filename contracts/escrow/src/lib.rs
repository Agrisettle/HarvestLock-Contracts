//! HarvestLock escrow contract.
//!
//! One instance per commitment (PRD §4.8). Implements the happy-path state
//! machine:
//!
//!   Draft -> Locked -> Advance1Released -> CheckpointPassed
//!         -> Advance2Released -> Delivered -> Settled
//!
//! Cancelled / Defaulted / Disputed exist in the `Status` enum because the
//! PRD's state machine names them, but no function transitions into them
//! yet — see HANDOFF.md for what's deliberately not implemented and why.

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
    Delivered,
    Settled,
    Cancelled,
    Defaulted,
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
}

#[contracttype]
pub enum DataKey {
    Commitment,
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
    /// `initialize`, so the final settlement transfer is always >= 0.
    pub advance2_bps: u32,
    pub status: Status,
    pub created_at: u64,
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
        buyer.require_auth();

        let commitment = Commitment {
            buyer,
            cooperative,
            warehouse_operator,
            token,
            total_amount,
            advance1_bps,
            advance2_bps,
            status: Status::Draft,
            created_at: env.ledger().timestamp(),
        };
        env.storage().instance().set(&DataKey::Commitment, &commitment);
        Ok(())
    }

    /// Draft -> Locked. Pulls the buyer's full deposit into the contract.
    ///
    /// The buyer must have already approved this contract to transfer
    /// `total_amount` of `token` on their behalf (standard SEP-41 token
    /// `approve`), or this call fails at the token contract, not here.
    pub fn lock(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Draft {
            return Err(Error::InvalidState);
        }
        c.buyer.require_auth();

        token::Client::new(&env, &c.token).transfer(
            &c.buyer,
            &env.current_contract_address(),
            &c.total_amount,
        );

        c.status = Status::Locked;
        Self::save(&env, &c);
        Ok(())
    }

    /// Locked -> Advance1Released. Transfers `advance1_bps` of the total to
    /// the cooperative.
    ///
    /// Deliberately not auth-gated: the destination (cooperative) and
    /// amount (a fixed bps of the locked total) are fixed at `initialize`
    /// and cannot be redirected by whoever calls this, so anyone poking the
    /// contract to advance a state that's already reachable isn't a risk.
    /// This is *not* yet the claimable-balance-with-expiry mechanic the PRD
    /// describes (§4.8, §17) — see HANDOFF.md.
    pub fn release_advance_1(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Locked {
            return Err(Error::InvalidState);
        }
        Self::pay_cooperative(&env, &c, c.advance1_bps);
        c.status = Status::Advance1Released;
        Self::save(&env, &c);
        Ok(())
    }

    /// Advance1Released -> CheckpointPassed. Requires the warehouse
    /// operator's attestation — this is a judgment call, not a mechanical
    /// state advance, so unlike the advance releases it *is* auth-gated.
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

    /// CheckpointPassed -> Advance2Released. Same non-auth-gated reasoning
    /// as `release_advance_1`.
    pub fn release_advance_2(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::CheckpointPassed {
            return Err(Error::InvalidState);
        }
        Self::pay_cooperative(&env, &c, c.advance2_bps);
        c.status = Status::Advance2Released;
        Self::save(&env, &c);
        Ok(())
    }

    /// Advance2Released -> Delivered. Warehouse-operator-attested, same
    /// reasoning as `mark_checkpoint`.
    ///
    /// This is a bare boolean gate for now — it does not yet read a
    /// warehouse receipt's quantity/grade or apply the PRD §7 shortfall
    /// adjustment schedule. See HANDOFF.md.
    pub fn confirm_delivery(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Advance2Released {
            return Err(Error::InvalidState);
        }
        c.warehouse_operator.require_auth();
        c.status = Status::Delivered;
        Self::save(&env, &c);
        Ok(())
    }

    /// Delivered -> Settled. Pays the cooperative whatever wasn't already
    /// released as advances. No NGN/oracle conversion yet (PRD §4.2) — the
    /// full `total_amount` is treated as already being in the settlement
    /// asset.
    pub fn settle(env: Env) -> Result<(), Error> {
        let mut c = Self::load(&env)?;
        if c.status != Status::Delivered {
            return Err(Error::InvalidState);
        }
        let already_paid = Self::bps_amount(c.total_amount, c.advance1_bps)
            + Self::bps_amount(c.total_amount, c.advance2_bps);
        let remaining = c.total_amount - already_paid;
        if remaining > 0 {
            token::Client::new(&env, &c.token).transfer(
                &env.current_contract_address(),
                &c.cooperative,
                &remaining,
            );
        }
        c.status = Status::Settled;
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

    fn pay_cooperative(env: &Env, c: &Commitment, bps: u32) {
        let amount = Self::bps_amount(c.total_amount, bps);
        if amount > 0 {
            token::Client::new(env, &c.token).transfer(
                &env.current_contract_address(),
                &c.cooperative,
                &amount,
            );
        }
    }
}

#[cfg(test)]
mod test;
