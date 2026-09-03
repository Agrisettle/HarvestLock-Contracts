# Security Policy

This is the Soroban escrow contract for HarvestLock — the component
where a bug has the highest real consequence, since it's what moves
tokens once this reaches mainnet. It's pre-pilot software today (no
real cooperative or buyer has used it, no mainnet deployment exists —
see the Status section in [`README.md`](./README.md) and
[`HANDOFF.md`](./HANDOFF.md)), but report vulnerabilities privately
regardless, not through a public issue.

## Reporting a vulnerability

Email **samuelojetunde898@gmail.com** with:

- Which function in `contracts/escrow/src/lib.rs` is affected
- Steps to reproduce — ideally a failing `cargo test` case, or a real
  testnet transaction that demonstrates it
- What you think the impact is: funds getting stuck, an
  authorization check that can be bypassed (a `require_auth()` that
  doesn't actually gate what it should), a state transition reachable
  from a state it shouldn't be, incorrect settlement math (the PRD §7
  shortfall/grade adjustment or the advance-tranche accounting are the
  parts with the most arithmetic to get wrong)

There's no dedicated security inbox or bug-bounty program yet — this is
a two-person team pre-pilot. You'll get a human reply, not an automated
one.

**Please don't open a public GitHub issue for a suspected vulnerability**
until it's been triaged privately first — same policy across every repo
under [Agrisettle](https://github.com/Agrisettle), including the main
[`HarvestLock`](https://github.com/Agrisettle/HarvestLock) repo.

## Scope

Everything in `contracts/escrow/` is in scope. Findings that matter
most: any `require_auth()` that doesn't actually restrict the caller
the way its doc comment claims, any path where escrowed tokens could
end up stuck (unreachable by any function) or double-spent, any
settlement math (advance-tranche bps, the two-phase-funding
deposit/remainder split, or the shortfall/grade `settlement_bps`
calculation) that can be made to pay out more than `total_amount` or
less than what's actually owed.

Out of scope: findings that require a compromised private key already
in an attacker's possession — this contract, like the rest of the
project, assumes keys stay private (PRD §4.6). Report issues in
`soroban-sdk` itself upstream, not here, unless there's a
HarvestLock-specific exploit path through it.

## Supported versions

No versioned release or mainnet deployment exists yet — `main` is the
only thing that exists, and every testnet deployment listed in
`HANDOFF.md` is a throwaway validation artifact, not something anyone
should depend on continuing to exist.
