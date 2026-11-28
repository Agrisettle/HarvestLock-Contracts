# Batching deploy + initialize: the constructor question, answered

Issue #68 asks whether Soroban's constructor-arg deploy pattern
(`Operation.createCustomContract` with `constructorArgs`, backed by a
`__constructor` in the contract) should let a buyer deploy-and-initialize in
one atomic call, reducing the current three-call create-commitment flow.
This doc is the written recommendation that issue's "done" asks for.

Ground truth is the code (`contracts/escrow/src/lib.rs`), the host/SDK
behaviors cited inline, and the sibling repo's
[`api/HANDOFF.md`](https://github.com/Agrisettle/HarvestLock/blob/main/api/HANDOFF.md)
("Design decisions and why"), which flagged this as a contracts-repo
decision and whose finding this doc follows up on.

## TL;DR

- **Recommendation: do not add a `__constructor` now.** The current
  deploy → initialize → lock flow stays.
- Adding one is a **breaking deploy-path change, not an additive option**:
  on protocol 22+ every deploy invokes `__constructor`, and a
  parameterized constructor invoked with the API's current empty
  `constructorArgs` fails the deploy outright. The two paths cannot
  coexist; the contract change and every deploy caller must land together.
- It cannot buy what the issue hopes for anyway. It could fold
  *deploy + initialize* into one op, but never *initialize + lock* — a
  transaction permits exactly one `InvokeHostFunctionOp`, and the Draft
  window between initialize and lock is load-bearing (PRD §4.9's
  `set_allocation` ledger is pre-lock by design).
- The one real benefit — atomicity, so there's no orphaned
  "deployed but never initialized" instance — removes a failure mode that
  involves no funds and is already detectable. That doesn't justify a
  coordinated cross-repo break of every deploy path (API, frontends, all
  143 tests).
- The flip conditions are concrete (§4). If one becomes true, the change
  is well-scoped and §4 lists exactly what it would touch.

## 1. What the current flow is, and why each call is separate

Three transactions create a commitment today:

1. **Deploy** — the API's deploy route builds
   `Operation.createCustomContract({ address: deployer, wasmHash, salt })`
   and only the fee-paying deployer signs it (`api/src/stellar/deploy.ts`).
   No `constructorArgs` are passed; no party auth is involved.
2. **initialize** — a separate call, signed by the **buyer**
   (`buyer.require_auth()` in `lib.rs`). One `CommitmentTerms` struct
   (fourteen positional parameters before #270), including the
   `grade_price_bps` schedule, plus the optional `OracleConfig`.
3. **lock** — also buyer-signed (`c.buyer.require_auth()`), escrows the
   deposit, and is the point of no return.

The split is load-bearing, per `api/HANDOFF.md`:

- The API is **build-unsigned / client-signs** by principle — it never
  holds a party's key, only ever signing the deployer's own fee-paying
  deploy op. `initialize` and `lock` therefore can't be silently fused
  into ops the deployer signs.
- Folding initialize into a constructor would make the **deploy op carry
  the buyer's auth entry** — the API's staged multi-party signing
  machinery (`api/src/stellar/multiParty.ts`, Freighter `signAuthEntry`,
  `signatureExpirationLedger`) would have to retarget the deploy op
  instead of the initialize route.
- The API's note ends: "Changing the contract to add a constructor is a
  contracts-repo decision, not an API one." This doc is that decision.

## 2. What adding a `__constructor` would actually do

Mechanics, against `soroban-env-host` 28.x and `@stellar/stellar-sdk` 16:

- On protocol 22+, **every** wasm deploy invokes `__constructor`. A wasm
  *without* one, deployed with empty args, is a no-op
  (`treat_missing_function_as_noop` when `constructor_args.is_empty()`) —
  which is why today's deployment works. A wasm *with* a parameterized
  constructor, deployed with empty args, is an arity mismatch and **fails
  the deploy**. So this is not a "both paths work" option; it is a
  replacement, and every caller moves in the same release.
- The client-side capability exists: the JS SDK's `createCustomContract`
  passes `constructorArgs` through to `CreateContractArgsV2`. Using it is
  a payload change, not a platform limitation.
- The payload would be initialize's full surface — the four party/token
  addresses, the `CommitmentTerms` struct, and `OracleConfig` — plus the
  buyer's signed auth entry traveling on the deploy op. The contract-side
  code for that already exists — it is `initialize`, moved — but the
  public surface changes either way:
  - replace `initialize` entirely (the API's initialize route and its
    staged signing flow disappear), or
  - keep both, where the constructor initializes and `initialize` becomes
    a call that can only ever return `AlreadyInitialized` on new
    deployments — a footgun with no upside.

## 3. Why "initialize + lock" can't be one call

Even in the most aggressive version, the flow goes 3 calls → 2, because:

- **A transaction permits exactly one `InvokeHostFunctionOp`** (Stellar
  docs, "Stellar Transaction" — contracts are the unit of atomic
  composition). "Two ops in one transaction" does not exist; batching
  means *one contract method doing both*, constructor or not.
- The Draft window between `initialize` and `lock` is a **product
  feature, not an accident**: `set_allocation` — the PRD §4.9 Rung 1
  allocation ledger — is pre-lock only, pinned by
  `cannot_set_allocation_after_lock`. Fusing initialize + lock deletes
  the only window in which the member ledger can be written, or forces
  the member list into the initialize/constructor payload, which is the
  same deletion wearing a different hat. A combined
  `initialize_and_lock` method would have the same problem.
- So the issue's title premise — batching *initialize + lock* — is
  unachievable without a product regression. What's actually on the
  table is only *deploy + initialize* (2 of the 3 calls).

## 4. When this flips, and what it would touch

Adopt the constructor if/when one of these becomes true:

- **Orphaned deploys become a measured operational problem**: today a
  deploy that lands and an initialize that never does leaves an instance
  holding nothing (no funds, no party auth consumed); if that ever stops
  being benign in practice, atomic deploy+initialize is the fix.
- **The product requires one-signature onboarding**: a buyer signing a
  single op that creates their commitment, instead of deploy (deployer)
  + initialize and lock (buyer).
- **The signing model changes** so attaching the buyer's auth to the
  deploy op is natural, rather than a retarget of the staged multi-party
  flow.

Scope if it flips, so the next reader doesn't have to re-derive it:

- **contracts**: `__constructor` mirroring `initialize`'s argument list
  (parties, `CommitmentTerms`, `OracleConfig`) + buyer auth; `initialize` deleted or reduced to a guard; every test's
  `env.register(EscrowContract, ())` becomes
  `env.register(EscrowContract, (args...))`; initialization-failure tests
  become deploy-failure tests (including the fx/zero-window validation
  cases, which move from `Error` returns to deploy aborts).
- **api**: `deploy.ts` passes `constructorArgs` and grows buyer-auth
  staging on the deploy op; the initialize route and its multi-party
  signing flow retarget or disappear; `HANDOFF.md`'s decision note
  updates.
- **frontends / docs**: the create-commitment journey in each frontend,
  plus any deployment guide that shows the three-call flow.
- **Note the revert cost is the same**: once a constructor exists, going
  back is equally breaking, because empty-arg deploys fail against a
  parameterized constructor. This is a one-way door per contract version;
  if it's ever taken, it should be taken for a demonstrated reason.

## 5. Events vs. batching for the API's cache (context for #69)

#69's investigation (in the same PR as this doc) found the contract now
publishes a `status_changed` event on every real transition — see
`StatusChanged` in `lib.rs`. That addresses the API's read-triggered
cache-refresh trade-off from the *observation* side without touching the
create-commitment flow. Batching and eventing are independent decisions;
nothing in this doc changes because events now exist.
