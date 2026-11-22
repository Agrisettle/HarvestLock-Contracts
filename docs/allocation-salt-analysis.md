# Allocation Salt Collision Probability Analysis

## Overview

This document analyzes the collision probability of the per-member allocation salt scheme used in the HarvestLock escrow contract's allocation ledger.

## Salt Scheme Details

As documented in [HANDOFF.md](../HANDOFF.md) and implemented in the contract:

1. Each member's allocation is recorded as an `AllocationMember` struct:
   ```rust
   #[contracttype]
   #[derive(Clone, Debug, PartialEq, Eq)]
   pub struct AllocationMember {
       pub member_hash: BytesN<32>,  // HMAC-SHA256(salt, phone_number)
       pub share_bps: u32,           // Basis points of total payout
   }
   ```

2. The `member_hash` is computed off-chain in the API as `HMAC-SHA256(salt, phone_number)` where:
   - `salt` is a fresh random 32-byte value generated per member
   - `phone_number` is the member's phone number (the identifier being hashed)
   - The salt and phone number mapping is stored only in the API's Postgres database

3. This design ensures NDPA erasability: deleting the off-chain salt+phone row makes the on-chain hash permanently unlinkable to a real person, since brute-forcing a random salt is infeasible regardless of phone number keyspace size.

## Collision Probability Analysis

### Hash Function Properties
- HMAC-SHA256 produces a 256-bit (32-byte) output
- With a fresh random 32-byte salt per member, each member's hash is effectively independent and uniformly distributed across the 2^256 possible outputs

### Birthday Problem Calculation
The probability of at least one collision among `n` members is approximately:
```
P(collision) ≈ n² / 2^(b+1)
```
where `b` is the bit length of the hash output (256 for SHA-256).

### Realistic Allocation Sizes
From the contract constants and PRD specifications:
- Maximum allocation members: `MAX_ALLOCATION_MEMBERS = 200` (anchored to PRD §16.3's 200-farmer cooperative scale)
- Realistic range: tens to low hundreds of members per commitment

### Probability Calculations

For n = 50 members (lower end of realistic range):
```
P(collision) ≈ 50² / 2^257 = 2500 / 2^257 ≈ 1.3 × 10^-75
```

For n = 100 members:
```
P(collision) ≈ 100² / 2^257 = 10000 / 2^257 ≈ 5.3 × 10^-74
```

For n = 200 members (maximum):
```
P(collision) ≈ 200² / 2^257 = 40000 / 2^257 ≈ 2.1 × 10^-73
```

### Contextual Interpretation
These probabilities are extraordinarily small:
- For comparison, the odds of winning a typical lottery are around 10^-8
- The odds of being struck by lightning in a year are about 10^-6
- The collision probability for our allocation scheme is smaller than these by 65+ orders of magnitude

In practical terms, with a fresh random salt per member:
1. Each member receives a effectively unique 256-bit identifier
2. The salt prevents any correlation between members' hashes
3. Even if an attacker knew the phone number format/keyspace, they would need to brute force a 256-bit salt per member attempt
4. The off-chain storage of the salt+phone mapping is what enables true erasability per NDPA s.34

## Security Implications

1. **Forward Security**: Even if future advances in computing make brute-forcing more feasible, the per-member salt means each phone number must be attacked separately with a unique 256-bit salt.

2. **Erasability Guarantee**: Deleting the off-chain salt+phone row (as permitted by NDPA s.34) renders the on-chain hash permanently unlinkable, as recovering the salt would require brute-forcing 2^256 possibilities per attempt.

3. **No Practical Collision Risk**: At realistic allocation sizes (≤200 members), collision probability is negligible for all practical security considerations.

## Conclusion

The per-member allocation salt scheme using HMAC-SHA256 with fresh 32-byte salts provides:
- Cryptographically secure member identification
- Negligible collision probability at realistic allocation scales (< 10^-72 for 200 members)
- Strong erasability guarantees compliant with NDPA s.34
- Forward security against advances in computational power

The scheme is appropriate for its intended purpose of providing transparent, yet privacy-preserving, allocation records on-chain.