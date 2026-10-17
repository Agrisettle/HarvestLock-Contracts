# Reflector `decimals()` — what the oracle conversion assumes, and why the trust is well-placed

Issue #63 asks what happens to the oracle conversion math if
[Reflector](https://reflector.network)'s own `decimals()` value ever changes,
and to document the assumption this contract makes rather than leave it
implicit. This doc is that documented finding, backed by a regression test
(`contracts/escrow/tests/oracle_fault_injection.rs`) that pins the behavior if
the assumption is ever violated.

Ground truth is the code (`contracts/escrow/src/lib.rs`,
`contracts/escrow/src/reflector.rs`), and — for the oracle side — Reflector's
real published contract source and the SEP-40 standard it implements. Where
this doc and the code disagree about what the contract *does*, the code wins.

## TL;DR

- **`oracle_rate()` does no conversion math at all.** It returns the raw,
  scale-free `price` integer exactly as `lastprice()` gave it, and never calls
  the oracle's `decimals()`. A `decimals()` change therefore cannot change what
  `oracle_rate()` returns. The issue's phrase "oracle_rate()'s conversion
  math" is a slight misattribution: the only conversion is in
  `resolve_fx_shortfall()`.
- **The one place `decimals()` is used is `resolve_fx_shortfall()`**, as the
  divisor `10i128.pow(decimals)` (`lib.rs:1245`). It reads `decimals()` live
  from the oracle on every call.
- **The assumption is that `decimals()` is immutable.** That is well-founded
  for today's deployed Reflector oracles: the real contract writes `decimals`
  exactly once at `config()` and aborts with `AlreadyInitialized` on any second
  call, exposes no `set_decimals`, and SEP-40 explicitly says the value
  "should never change."
- **If it were ever violated, the failure is silent, not loud.** The
  conversion would move by a power of ten with no error and no consistency
  check — pinned by
  `a_plausible_decimals_change_silently_rescales_resolve_fx_shortfall`. A
  *plausible* new value (e.g. 15 instead of 14) rescales by 10×; only an
  *absurd* value large enough to overflow `10i128.pow()` traps (that is
  `an_absurd_decimals_count_traps_resolve_fx_shortfall`, a separate finding).
- **No code change is recommended.** The assumption holds against the live
  oracle, and `OracleConfig` deliberately does not duplicate `decimals()`
  because it is a property of the oracle, not of the commitment. The
  recommendation is to keep this as a documented, tested assumption and
  revisit only if the configured oracle is ever replaced or redeployed with a
  different scale.

## 1. Where `decimals()` actually enters the math

`oracle_rate()` (`lib.rs`) loads the stored `OracleConfig`, calls the shared
`read_oracle_rate()` helper, and returns an `OracleRate { price, timestamp }`.
`read_oracle_rate()` calls exactly one Reflector method — `lastprice()`:

```rust
let client = ReflectorPulseClient::new(env, &config.oracle_contract);
let quote = client
    .lastprice(&ReflectorAsset::Other(config.price_asset.clone()))
    .ok_or(Error::OraclePriceUnavailable)?;
```

No `decimals()` call appears on either path. The raw `price` is returned
untouched.

The scale is applied in exactly one place, `resolve_fx_shortfall()`
(`lib.rs:1242`–`1245`):

```rust
let rate = Self::read_oracle_rate(&env, &oracle_config)?;
let decimals = ReflectorPulseClient::new(&env, &oracle_config.oracle_contract).decimals();

let ngn_owed = Self::bps_amount(oracle_config.denominated_amount, c.settlement_bps);
let fx_adjusted_total = ngn_owed * rate.price / 10i128.pow(decimals);
```

So `decimals()` is read *live, per settlement*, and used to divide the raw
price. The `OracleRate` public type intentionally does **not** carry the
decimals; `lib.rs`'s doc comment for it says callers "can read it directly off
the oracle contract if they need it." (Both reads happen inside one Soroban
invocation/ledger, so the two cross-contract calls cannot straddle a state
change — the consistency question is purely whether `decimals()` is stable
*across time*, which §2 answers.)

## 2. Is the trust well-placed? Yes — `decimals` is immutable in Reflector

Three independent, checkable sources agree:

1. **SEP-40, the standard Reflector implements**, under "Timeframe Resolution
   and Price Feed Precision":

   > Once the price feed aggregation contract is deployed, these values should
   > never change. It is impossible to guarantee that consumer contracts can
   > react to any updates in these values.

   And under "Design Rationale": "the actual price can be calculated as
   `price / 10^decimals`."

2. **Reflector's own integration documentation** (`reflector.network`, the
   "Integrate oracle in your code" example comments): "Do not forget for price
   precision, get decimals from the oracle (this value can be also hardcoded
   once the price feed has been selected because **decimals never change in
   live oracles**)."

3. **The real contract source** (`reflector-network/reflector-contract`,
   `oracle/src/settings.rs`). `decimals` is stored under `DECIMALS_KEY` and
   written only inside `init()` — whose first line aborts if the contract is
   already initialized:

   ```rust
   pub fn init(e, base, decimals, resolution, ...) {
       //do not allow to initialize more than once
       if e.storage().instance().has(&RETENTION_PERIOD_KEY) {
           e.panic_with_error(Error::AlreadyInitialized);
       }
       ...
       //initialized only once and cannot be changed in the future
       instance.set(&BASE_KEY, base);
       instance.set(&DECIMALS_KEY, &decimals);
       ...
   }
   ```

   `PriceOracleContractBase` exposes `decimals()` as a pure getter and offers
   admin setters for `cache_size`, `history_retention_period`, `fee_config`,
   assets, and prices — but **no `set_decimals`**. `config()` re-invokes
   `init()`, so a redeploy/upgrade that tried to re-scale would abort with
   `AlreadyInitialized` rather than silently change the value. (The admin can
   `update_contract` to new WASM, but the decimals value lives in instance
   storage and remains write-once; a future implementation *could* add a
   migration that changes it, which is the one scenario §3 covers.)

The assumption this contract makes, stated plainly:

> The configured Reflector oracle's `decimals()` is a constant for the life of
> the oracle contract. The raw `price` returned by `lastprice()` and the
> `decimals()` used to scale it are always the same unit, so scaling a
> settlement by the live `decimals()` is correct.

That holds for the deployed/testnet oracles this contract targets. It is a
dependency on the provider's own invariant, not something HarvestLock
independently enforces.

## 3. What happens if the assumption is ever violated

`a_plausible_decimals_change_silently_rescales_resolve_fx_shortfall`
(`contracts/escrow/tests/oracle_fault_injection.rs`) drives the mock oracle to
report `decimals() == 15` with the *same* raw price a 14-decimal oracle would
report, and asserts:

- `oracle_rate()` returns the raw price unchanged — it never consults
  `decimals()`, so a change cannot touch it;
- `resolve_fx_shortfall()` **succeeds with no error and no mismatch
  detection**, but converts the same raw price to 1/10th the value
  (`fx_adjusted_total` 175 000 instead of 1 750 000 in the fixture). The
  conversion simply moves by `10^(old_decimals - new_decimals)`.

The important property is that the failure is **silent**: there is no
`OracleDecimalsChanged`-shaped error and no cross-check, because the contract
never records the scale it expected. This is why it is worth documenting: an
operator seeing a wildly wrong `fx_adjusted_total` after an oracle migration
would get no on-chain signal pointing at the cause.

Contrast with the separate, already-pinned F5 finding
(`an_absurd_decimals_count_traps_resolve_fx_shortfall`): a value like `200`
makes `10i128.pow(decimals)` overflow, which *does* trap (and Soroban's
atomicity leaves no partial state). A merely-different-but-valid value such as
15 or 6 does **not** trap; it silently rescales.

## 4. Why no code change follows

- **The assumption is verified, not guessed.** It is grounded in the oracle's
  actual source, not just its docs.
- **`OracleConfig` not storing `decimals` is deliberate.** The `OracleRate` doc
  comment in `lib.rs` records the reasoning: decimals "is a property of the
  oracle, not of this commitment." Pinning a copy at `initialize` would only
  move the trust — a pinned value could then disagree with the live one — and
  would add a field (and storage cost) to every commitment for no behavioral
  gain against the current invariant.
- **Re-checking `decimals()` against a stored copy would add a branch for a
  case the provider contract makes unreachable.** If Reflector ever shipped a
  value-changing migration, the right response is a product decision (reject
  and require re-initialization? accept the new scale and document it?), not a
  silent guard invented here.

**Flip condition:** if the `oracle_contract` this contract points at is ever
replaced or upgraded in a way that changes `decimals()` (a new oracle
deployment, not an in-place value change — the latter is impossible in the
current source), this assumption is void and the conversion should be
re-examined, ideally by storing the expected scale in `OracleConfig` and
rejecting a mismatch with a dedicated error.

## References

- SEP-40 — Oracle Price Feeds:
  <https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0040.md>
- Reflector contract source, `oracle/src/settings.rs` and
  `oracle/src/price_oracle.rs`:
  <https://github.com/reflector-network/reflector-contract>
- Reflector integration docs: <https://reflector.network/docs/interface>
- This repo: `contracts/escrow/src/reflector.rs` (the copied client
  interface), `lib.rs` (`oracle_rate`, `OracleRate`, `resolve_fx_shortfall`),
  `contracts/escrow/tests/oracle_fault_injection.rs` (the fault-injection
  harness and this issue's test).
