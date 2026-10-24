# Gas costs — measured on Stellar testnet with `soroban-cost-estimator`

Real resource-cost numbers for this contract's public methods, measured with
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
against freshly-deployed testnet instances. This is the **measured record**
companion to [`cost-estimation.md`](./cost-estimation.md), which covers the
separate question of *which tool* to standardize on — this doc doesn't
re-argue that, it just reports what the estimator actually returned.

Two measurement sessions are recorded here, kept deliberately separate
because they were taken against **different builds**:

| Session | Date | Methods | WASM | SDK |
| --- | --- | --- | --- | --- |
| **1** | 17 Oct 2026 | `release_advance_1`/`_2`, `claim_advance_1`/`_2`, `reclaim_advance_1`/`_2` | 52,119 bytes | 28.0.0 |
| **2** | 20 Oct 2026 | **`mark_checkpoint`, `confirm_delivery`** + same-session controls | 60,043 bytes | 29.0.1 |

Session 2 also re-measured `release_advance_1` and `claim_advance_1` on the
same build as a **control pair**, so the two sessions' numbers can be
compared without guessing at the SDK-version delta — see
[Same-session controls](#same-session-controls).

---

## Results — delivery group (`mark_checkpoint`, `confirm_delivery`)

Session 2, measured against instance
`CCTQTECYATFFE3GJWYSND6CJOGB6KLGSF3JKK5KW3HRBNTKLFBFJEUWJ`
([stellar.expert](https://stellar.expert/explorer/testnet/contract/CCTQTECYATFFE3GJWYSND6CJOGB6KLGSF3JKK5KW3HRBNTKLFBFJEUWJ)),
ledgers ~5290578–5290585 (`mark_checkpoint`) and ~5290754–5290757
(`confirm_delivery`).

One row per method. "Fee" columns are stroops (1 XLM = 10,000,000 stroops);
`Fee (total)` is the estimator's non-refundable + refundable sum — read the
caveats section before treating it as "what a transaction costs".

| Method | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) | Fee (total XLM) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `mark_checkpoint` | 1,523,347 | 0 | 2 | 2 | 144 | 1,584 | 148 | 4,579 | 901,791 | 906,370 | 0.0906370 |
| `confirm_delivery` | 1,530,217 | 0 | 2 | 2 | 144 | 1,580 | 164 | 4,584 | 902,392 | 906,976 | 0.0906976 |

`confirm_delivery` was measured with `delivered_quantity=1000`,
`grade_index=0` — full delivery at the top grade, against a commitment
initialized with `contracted_quantity: 1000` and
`grade_price_bps: [10000, 9000, 7500]`. `settlement_bps` came back as
`10000` on a fresh `get_commitment` read after the call, so the arguments
really did drive the contract's arithmetic path, not a rejected call.

**Reproducibility:** `confirm_delivery` was re-measured on a second,
independent instance
(`CDKP2KUIDUJ4MS3G4GDS372BK3HMRZZEMJGM4C5GULTGAISI2FFGZAXK`, ledger
5291028) and returned **identical** resource figures — 1,530,217 CPU, 2 read
/ 2 write entries, 144 / 1,580 bytes, tx size 164, 4,584 non-refundable
stroops. Only the refundable rent half moved (902,392 → 903,157).

## Same-session controls

Measured on instance
`CBTFFKL24HYNUUG4K26FBB3B47T5KROWR3FTMBH4SST5NA3NCT5IFATV`
([stellar.expert](https://stellar.expert/explorer/testnet/contract/CBTFFKL24HYNUUG4K26FBB3B47T5KROWR3FTMBH4SST5NA3NCT5IFATV)),
ledgers ~5290798–5290808, **same WASM, same SDK, same day** as the two rows
above. These exist so the delivery-group numbers are readable against a
baseline rather than against a table measured on a different build.

| Method | Auth | Moves money? | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `release_advance_1` | none | no | 996,645 | 0 | 1 | 1 | 0 | 1,508 | 152 | 2,359 | 194,633 | 196,992 |
| `mark_checkpoint` | warehouse | no | 1,523,347 | 0 | 2 | 2 | 144 | 1,584 | 148 | 4,579 | 902,337 | 906,916 |
| `claim_advance_1` | cooperative | yes (15,000,000) | 1,671,867 | 0 | 2 | 4 | 144 | 1,952 | 148 | 7,123 | 902,922 | 910,045 |

Two things fall out of this table, and both are reproducible:

- **`mark_checkpoint` measured on three independent instances**
  (`CCTQTE…`, `CDG2V5…` and the control instance `CBTFFK…`) **returned
  byte-identical resource figures** — 1,523,347 CPU, 2 read / 2 write
  entries, 144 / 1,584 bytes, 4,579 non-refundable stroops. Only the
  refundable (rent) half moved, by ~550 stroops, because the simulated TTL
  horizon differed by a couple of ledgers.
- **The auth-gated call costs ~53% more CPU than the permissionless one it
  sits next to**, and buys one extra read entry + one extra write entry.
  Everything else is held constant: same instance, same build, same
  contract balance, same shape of `Commitment` written back. The only
  differences between the `release_advance_1` row and the `mark_checkpoint`
  row are the `require_auth()` call and the status value being written —
  and a status discriminant cannot account for 527k CPU instructions or a
  whole extra ledger entry. The extra read+write entry appears only on the
  auth-gated path.

## ⚠️ Flag — this group moves zero tokens, yet costs ~53% more than the permissionless equivalent, because `require_auth()` adds a ledger entry

`mark_checkpoint` and `confirm_delivery` are pure bookkeeping — `mark_checkpoint`
flips one status, `confirm_delivery` records a quantity, a grade and a
`settlement_bps` — and **neither moves a single stroop**. Even so, at
1.52M–1.53M CPU instructions they land within 9% of `claim_advance_1`
(1.67M), which *does* move 15,000,000 stroops to the cooperative. The gap
is entirely the auth-gated path: `require_auth()` on the warehouse
operator's address adds one read entry, one write entry and ~527k CPU
instructions (~53% over `release_advance_1`'s 996,645) on top of a state
transition that is otherwise identical in shape. The auth is load-bearing
here — `mark_checkpoint` *is* the warehouse operator's attestation, and
`confirm_delivery`'s grade/quantity call is authoritative per PRD §7 — so
this is a cost to budget for, not a saving to bank.

## ⚠️ Flag — 99.5% of each `Fee (total)` is simulated storage rent, not execution

The estimator's `Fee (total)` for this group (~906,000 stroops / ~0.0906
XLM) is **not** what a transaction costs: 901,791 of the 906,370 is the
*refundable* rent half, which is an artifact of the simulation extending
entry TTLs to the simulation horizon. The **real** submitted transactions
were charged **4,181 stroops** (`mark_checkpoint`) and **4,242 stroops**
(`confirm_delivery`) in total — see the CLI cross-check below. For
budgeting, use the non-refundable column (4,579 / 4,584 stroops) plus the
CLI's real-submission numbers, not the total.

---

## Results — advance-tranche group (session 1, retained)

Real resource-cost numbers for the six advance-tranche methods
(`release_advance_1`/`_2`, `claim_advance_1`/`_2`, `reclaim_advance_1`/`_2`),
measured with the same tool against freshly-deployed testnet instances of
this contract.

| | |
| --- | --- |
| Tool | `soroban-cost-estimator` **v0.1.0** (crates.io), `estimate --wasm … --id … --fn …` |
| Network | Stellar **testnet** (`https://soroban-testnet.stellar.org`), **protocol 29** |
| When | 17 Oct 2026, across ledgers ~5243848–5243875 (each row is one `simulateTransaction` RPC at that moment) |
| WASM | `harvestlock_escrow.wasm`, hash `39b99c184c943b29707d9bf65ee4c34bcd2ae2704e37e805b1183c8eca03a14b`, 52,119 bytes, built from this branch's HEAD via `stellar contract build` |
| Instance A (claim path) | `CDL3RTC6A52B4XJTD7YQ4SWL6RVLZQ44NMOSYC2ENIR3DPKEINB7RC2Q` ([stellar.expert](https://stellar.expert/explorer/testnet/contract/CDL3RTC6A52B4XJTD7YQ4SWL6RVLZQ44NMOSYC2ENIR3DPKEINB7RC2Q)) — `claim_window_secs: 3600` |
| Instance B (reclaim path) | `CCXI4MFIHALCMFPX2Y3G5V2WMBCXR46JDWLUW7KBYXCR6WJQQQ2MATUC` ([stellar.expert](https://stellar.expert/explorer/testnet/contract/CCXI4MFIHALCMFPX2Y3G5V2WMBCXR46JDWLUW7KBYXCR6WJQQQ2MATUC)) — `claim_window_secs: 15` (demo-only, same reasoning as HANDOFF's Deployments 2/5) |
| Commitment shape | native XLM SAC (`CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`), `total_amount: 100,000,000` stroops, 15%/20% advance split |

**Why two instances, not one**: `claim_advance_*` requires the claim window to
still be *open*, `reclaim_advance_*` requires it to have *passed* — mutually
exclusive in wall-clock time on a single instance. Each instance was walked to
the required on-chain state with real transactions before the corresponding
simulations were run (a simulation against the wrong state just errors — the
contract's guards are enforced in the simulator too, which is the point of
measuring against a deployed instance rather than a local harness). All six
functions take no arguments, so no `--arg` was needed.

| Method | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) | Fee (total XLM) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `release_advance_1` | 936,397 | 0 | 1 | 1 | 0 | 1,476 | 152 | 2,341 | 13,233 | 15,574 | 0.0015574 |
| `release_advance_2` | 939,162 | 0 | 1 | 1 | 0 | 1,476 | 152 | 2,342 | 6,456 | 8,798 | 0.0008798 |
| `claim_advance_1` | 1,611,598 | 0 | 2 | 4 | 144 | 1,920 | 148 | 7,105 | 839,761 | 846,866 | 0.0846866 |
| `claim_advance_2` | 1,611,619 | 0 | 2 | 4 | 144 | 1,920 | 148 | 7,105 | 839,761 | 846,866 | 0.0846866 |
| `reclaim_advance_1` | 1,609,708 | 0 | 2 | 4 | 144 | 1,920 | 152 | 7,105 | 839,778 | 846,883 | 0.0846883 |
| `reclaim_advance_2` | 1,609,708 | 0 | 2 | 4 | 144 | 1,920 | 152 | 7,105 | 839,778 | 846,883 | 0.0846883 |

### ⚠️ Flag — claim/reclaim fees look ~54–96× a release, but 99.2% of that is simulated storage rent, not execution

`claim_advance_*`/`reclaim_advance_*` each report ~846.9k stroops against a
release's 8.8k–15.6k — but the refundable (rent) half is 839.8k of it, and a
**real submitted** `reclaim_advance_2` on the same instance was charged
**8,189 stroops** total (4,908 of the proposed fee refunded); the
non-refundable/execution half the estimator reports (~7.1k) is the comparable
number, and it's in the same league as a release. The disproportionate part is
the storage-rent assumption baked into the simulation's footprint (4 written
entries: contract instance + SAC balance entries, extended to the simulation's
TTL horizon), not anything the contract's logic burns — worth remembering if
footprint entries ever multiply, not an active problem today.

---

## Reading the numbers

- **Memory bytes = 0 on every row, in both sessions** — the tool found no
  memory-bytes figure in the current-protocol `simulateTransaction` response
  to decode; use CPU instructions as the compute metric. It is *not* a claim
  that these calls use no memory.
- **The auth-gated calls are the expensive ones**, not the ones that move
  money. `mark_checkpoint`/`confirm_delivery` (no money) cost ~53% more CPU
  than `release_advance_1` (no money, no auth), and ~91% of what
  `claim_advance_1` costs while moving 15,000,000 stroops. The cost is the
  authorization entry, not the arithmetic and not the transfer.
- **`release_advance_1`/`release_advance_2` are the cheapest calls in the
  contract** (~996k CPU session 2, ~936–939k session 1) — they only move a
  deadline in instance storage. The whole 15,574-vs-8,798 stroop gap between
  the two in session 1 sits in the *refundable* half (13,233 vs 6,456); the
  execution halves differ by 1 stroop. That's a fresh-instance TTL artifact of
  when the simulation ran, not a difference in contract logic — the two
  functions share one private `open_tranche` implementation by design (see
  HANDOFF's design decisions).
- **The claims and reclaims are all the same cost within noise** (~1.610–1.612M
  CPU session 1, 1.672M session 2): tranche 1 and 2 route through the same
  private `claim_tranche`/`reclaim_tranche`, and claim-vs-reclaim only flips a
  deadline comparison. The extra CPU over a release is the SEP-41 SAC
  `transfer` cross-contract call (and its events) that releases don't make,
  plus the auth entry both of them carry.
- **The two sessions' absolute numbers are not directly comparable** — SDK
  28.0.0 → 29.0.1 and a 52,119 → 60,043-byte WASM. `claim_advance_1` moved
  1,611,598 → 1,671,867 CPU (+3.7%) between them; the *deltas* between
  methods moved almost not at all (session 1's claim-minus-release delta was
  675,201 CPU; session 2's was 675,222). That stability is the reason the
  same-session control table above is worth more than the absolute numbers.
- **Session 2's WASM is 60,043 bytes** vs session 1's 52,119 — the delta is
  the SDK bump plus the `status_changed` event surface, not a change to the
  methods measured here.

---

## Cross-check against the CLI (the source of record, per `cost-estimation.md`)

Both methods in this group were **actually submitted** (not just simulated)
with `stellar contract invoke … --cost`, as an independent check on the
estimator's fee math.

### `mark_checkpoint`

Real submission:
tx [`6c30cd83…`](https://stellar.expert/explorer/testnet/tx/6c30cd83099a717733cac0e5ac31a80e783893f4bc1626270a1e3b01546b1657)
(ledger 5290588), instance `CCTQTE…`.

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 4,579 | 3,680 charged |
| Refundable / rent | 901,791 proposed-in-footprint | 5,300 proposed → 401 charged, 4,899 refunded |
| Total resource fee | 906,370 | 4,081 charged |
| Inclusion fee | — | 100 charged |
| **Total charged** | — | **4,181** |

### `confirm_delivery`

Real submission:
tx [`a42c212a…`](https://stellar.expert/explorer/testnet/tx/a42c212a5a124c9adcc55c8c944a5d6d1da4959373db52fd04f07b10b58d3ec6)
(ledger 5290760), instance `CCTQTE…`, `delivered_quantity=1000`,
`grade_index=0`.

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 4,584 | 3,751 charged |
| Refundable / rent | 902,392 proposed-in-footprint | 5,288 proposed → 391 charged, 4,897 refunded |
| Total resource fee | 906,976 | 4,142 charged |
| Inclusion fee | — | 100 charged |
| **Total charged** | — | **4,242** |

The ~830–900-stroop non-refundable difference is envelope shape (the CLI's real
transaction carries auth/footprint details the estimator's synthetic envelope
doesn't); the rent gap is the story flagged above. Note that
`stellar contract invoke --send=no --cost` prints **no** cost block on
stellar-cli v28 — cost only prints on a submitted transaction.

For completeness, session 1's cross-check on `reclaim_advance_2` was:

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 7,105 | 7,620 charged |
| Refundable / rent | 839,778 proposed-in-footprint | 5,377 proposed → 469 charged, 4,908 refunded |
| Total resource fee | 846,883 | 8,089 charged |
| Inclusion fee | — | 100 charged |

---

## Tooling finding — `soroban-cost-estimator` v0.1.0 cannot encode a `u32` argument

This is the one thing in this session that needed working around, and it is
an upstream limitation, not a local misconfiguration — worth reporting to the
upstream project.

`confirm_delivery` takes `delivered_quantity: u32` and `grade_index: u32`. The
estimator's `--arg` infers the argument's type **from the literal alone** —
`parse_arg_scval` in `src/xdr_helper.rs` returns `ScVal::I64` for any integer
that fits in an `i64` — and ignores the contract spec the tool has already
parsed. The host then rejects the call before the contract body runs:

```text
$ soroban-cost-estimator estimate --wasm … --id CCTQTE… \
    --fn confirm_delivery --arg delivered_quantity=1000 --arg grade_index=0 --network testnet
Error: Simulation failed: HostError: Error(WasmVm, InvalidAction)

Event log (newest first):
   0: [Diagnostic Event] contract:CCTQTE…, topics:[error, Error(WasmVm, InvalidAction)],
      data:["VM call trapped: UnreachableCodeReached", confirm_delivery]
   1: [Diagnostic Event] topics:[fn_call, CCTQTE…, confirm_delivery], data:[1000, 0]
```

The SDK's macro-generated wrapper converts each incoming argument with
`try_from_val_for_contract_fn(...).unwrap_optimized(...)` (see
`soroban-sdk-macros`' `derive_fn.rs`), and a `u32` parameter's `ScVal` is the
`U32` tag of Soroban's tagged union — an `ScVal::I64` is not one, so the
conversion fails, `unwrap_optimized` panics, and the contract crate's
`panic = "abort"` release profile turns that into the
`UnreachableCodeReached` the host reports. `stellar-cli` does not hit this
because it encodes arguments from the contract spec.

**This is not fixed on upstream `main` either.** `main` (commit `9627919`)
adds `validate_args_against_spec`, which *accepts* `1000` for a `u32`
parameter — and then still encodes the value as `ScVal::I64`, so it fails the
same way, only later. The upstream README's claim that "the inferred type
drives the simulation's serialization size, which is what the fee math
actually depends on" is true for the fee math, but the inference also has to
survive the contract's own argument decoding, and it doesn't.

The limitation is wider than `u32`. The estimator's `--arg` can express
`bool`, integers and strings, and nothing else — so it cannot invoke a
function that takes an `Address`, a `Vec<_>` or an `Option<_>` either.
`initialize` takes all three, which means the estimator cannot measure the
*instantiation* of a new commitment at all (see
[WASM Upload vs Instantiation](#wasm-upload-vs-instantiation) below). That
is a real gap in the tooling, and worth reporting upstream alongside the
`u32` one. It does not affect the measurements in this document: what is
being measured is an invocation **against an instance that is already
deployed and already initialized**.

### The patch used here

A two-hunk patch to the v0.1.0 source, applied on top of the upstream
`v0.1.0` tag (`6903d3e`), built with `cargo build --release --locked`:

```diff
--- a/src/main.rs
+++ b/src/main.rs
@@ -214,7 +214,24 @@
 
     let sc_vals: Vec<stellar_xdr::ScVal> = args
         .iter()
-        .map(|a| xdr_helper::parse_arg_scval(a))
+        .enumerate()
+        .map(|(i, a)| {
+            // Resolve the declared parameter type from the contract spec so
+            // the value is encoded the way a spec-aware client would encode
+            // it (an `ScVal::U32` for a `u32` parameter, not an `ScVal::I64`).
+            // The `--arg` key is matched against the parameter name first,
+            // falling back to positional order.
+            let key = a.split_once('=').map(|(k, _)| k).unwrap_or("");
+            let param = fn_name
+                .and_then(|name| wasm_info.functions.iter().find(|f| f.name == name))
+                .and_then(|f| {
+                    f.params
+                        .iter()
+                        .find(|p| p.name == key)
+                        .or_else(|| f.params.get(i))
+                });
+            xdr_helper::parse_arg_scval_typed(a, param.map(|p| p.type_name.as_str()))
+        })
         .collect();
```

```diff
--- a/src/xdr_helper.rs
+++ b/src/xdr_helper.rs
@@ -273,6 +273,64 @@
      stellar_xdr::ScVal::String(sc_string)
  }
+
+/// Parse a single `--arg` value into an `ScVal`, honouring the contract
+/// spec's declared parameter type when one is known.
+///
+/// Upstream v0.1.0 always infers the type from the literal (`i64` for any
+/// integer that fits), which traps inside the contract for a parameter the
+/// spec declares as `u32`/`i32`/…: the SDK's macro-generated wrapper
+/// converts the incoming `ScVal` with
+/// `try_from_val_for_contract_fn(...).unwrap_optimized(...)`, and a `u32`
+/// parameter's `ScVal` is the `U32` tag — an `ScVal::I64` is not one, so the
+/// conversion fails and the VM aborts with `UnreachableCodeReached` before
+/// the contract body runs.
+///
+/// This variant encodes the value with the declared type when the spec's type
+/// name is one of the modelled primitives, and otherwise falls back to the
+/// upstream inference. The measurement path (RPC simulation, footprint decode,
+/// fee math) is untouched — only the argument's `ScVal` encoding changes, and
+/// it changes to exactly what a spec-aware client such as `stellar-cli`
+/// sends.
+pub fn parse_arg_scval_typed(arg: &str, type_name: Option<&str>) -> stellar_xdr::ScVal {
+    let value = arg.split_once('=').map(|(_, v)| v).unwrap_or(arg);
+
+    match type_name {
+        Some("bool") => {
+            if let Ok(b) = value.parse::<bool>() {
+                return stellar_xdr::ScVal::Bool(b);
+            }
+        }
+        Some("u32") => {
+            if let Ok(n) = value.parse::<u32>() {
+                return stellar_xdr::ScVal::U32(n);
+            }
+        }
+        Some("i32") => {
+            if let Ok(n) = value.parse::<i32>() {
+                return stellar_xdr::ScVal::I32(n);
+            }
+        }
+        Some("u64") => {
+            if let Ok(n) = value.parse::<u64>() {
+                return stellar_xdr::ScVal::U64(n);
+            }
+        }
+        Some("i64") => {
+            if let Ok(n) = value.parse::<i64>() {
+                return stellar_xdr::ScVal::I64(n);
+            }
+        }
+        Some("u128") | Some("i128") => {
+            // `stellar-xdr` 27 models 128-bit ints as `Int128Parts`, not a
+            // bare `i128`/`u128`; leave those to the upstream inference
+            // rather than hand-rolling the parts encoding here.
+        }
+        _ => {}
+    }
+
+    parse_arg_scval(arg)
+}
 
 #[cfg(test)]
 mod tests {
     use super::*;
```

### Why the patch doesn't change any measured value

The patch touches **only** how an `--arg` literal becomes an `ScVal`. The
measurement path — RPC `simulateTransaction`, footprint decode, fee math — is
untouched. That was verified rather than asserted: the stock v0.1.0 binary and
the patched binary were both run against `mark_checkpoint` (a zero-argument
function, so the patched code path is a no-op for it) on the same instance and
produced **byte-identical JSON**:

```json
{"function":"mark_checkpoint","wasm_hash":"e097f1ff…","cpu_instructions":1523347,
 "memory_bytes":0,"tx_size":148,"read_entries":2,"write_entries":2,
 "read_bytes":144,"write_bytes":1584,
 "fee":{"non_refundable_stroops":4579,"refundable_stroops":902337,
        "total_stroops":906916,"total_xlm":"0.0906916"}}
```

Every zero-argument row in this document was produced by the **stock,
unpatched** v0.1.0 binary. `confirm_delivery` is the only row that required
the patch, and only because of its `u32` parameters.

---

## Reproduce

### Session 2 — delivery group

```bash
# 0. Toolchain: Rust stable + the wasm32v1-none target, stellar-cli v28.1.0,
#    and soroban-cost-estimator v0.1.0 (crates.io).
export PATH="$HOME/.cargo/bin:$PATH"

# 1. Build + upload the WASM (from repo root)
stellar contract build --locked
WASM_HASH=$(stellar contract install \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account deployer --network testnet)
# → e097f1ffe08f878007cd9511d5fc70da0e238ac64491181e158d58f91e4714c5

# 2. Deploy two instances: A for the walk-through, C for the controls.
A=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)
C=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)

# 3. Initialize both (identical parameters), then lock.
#    native XLM SAC, 100,000,000 stroops, 15%/20% advance split.
for id in "$A" "$C"; do
  stellar contract invoke --id "$id" --source-account buyer --network testnet -- \
    initialize \
    --buyer "$BUYER" --cooperative "$COOP" --warehouse-operator "$WAREHOUSE" \
    --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
    --total-amount 100000000 --advance1-bps 1500 --advance2-bps 2000 \
    --claim-window-secs 3600 --remainder-window-secs 3600 --delivery-window-secs 86400 \
    --contracted-quantity 1000 --grade-price-bps '[10000,9000,7500]' --oracle-config null
  stellar contract invoke --id "$id" --source-account buyer --network testnet -- lock
done

# 4. Control row: release_advance_1 on C, while C is Locked.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$C" --fn release_advance_1 --network testnet

# 5. mark_checkpoint: estimate FIRST (a simulation does not advance state),
#    then submit for the CLI cross-check.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$A" --fn mark_checkpoint --network testnet
stellar contract invoke --id "$A" --source-account warehouse --network testnet --cost -- \
  mark_checkpoint

# 6. Walk A to ReadyForDelivery: release tranche 2, claim both tranches,
#    open the remainder window, fund the remainder.
stellar contract invoke --id "$A" --source-account deployer     --network testnet -- release_advance_2
stellar contract invoke --id "$A" --source-account cooperative --network testnet -- claim_advance_1
stellar contract invoke --id "$A" --source-account cooperative --network testnet -- claim_advance_2
stellar contract invoke --id "$A" --source-account cooperative --network testnet -- ready_for_delivery
stellar contract invoke --id "$A" --source-account buyer        --network testnet -- fund_remainder

# 7. confirm_delivery: estimate (needs the spec-aware `--arg` patch — see above),
#    then submit for the CLI cross-check.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$A" --fn confirm_delivery \
  --arg delivered_quantity=1000 --arg grade_index=0 --network testnet
stellar contract invoke --id "$A" --source-account warehouse --network testnet --cost -- \
  confirm_delivery --delivered_quantity 1000 --grade_index 0

# 8. Prove the patch didn't move any number: run the STOCK binary and the
#    PATCHED binary against the same zero-argument call and diff the JSON.
#    (Build the patched binary from the upstream v0.1.0 tag with the two-hunk
#    patch above; `cargo build --release --locked`.)
soroban-cost-estimator estimate --wasm … --id "$C" --fn mark_checkpoint --network testnet --json
./soroban-cost-estimator-patched estimate --wasm … --id "$C" --fn mark_checkpoint --network testnet --json
# → byte-identical output

# 9. Independent reproduction: deploy a second instance, walk it to
#    ReadyForDelivery the same way, and re-measure confirm_delivery. The
#    resource figures come back identical; only the rent half moves.
```

### Session 1 — advance-tranche group

```bash
# 1. Build + upload + deploy two fresh instances (from repo root)
stellar contract build
WASM_HASH=$(stellar contract install --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account deployer --network testnet)
A=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)
B=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)

# 2. Initialize both (identical except the claim window) as buyer, then lock.
#    claim window 3600 on A (claims stay open), 15 on B (windows lapse fast).
stellar contract invoke --id "$A" --source-account buyer --network testnet -- \
  initialize --buyer <buyer G…> --cooperative <coop G…> --warehouse-operator <warehouse G…> \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --total-amount 100000000 --advance1-bps 1500 --advance2-bps 2000 \
  --claim-window-secs 3600 --remainder-window-secs 3600 --delivery-window-secs 86400 \
  --contracted-quantity 1000 --grade-price-bps '[10000,9000,7500]' --oracle-config null
# … same for "$B" with --claim-window-secs 15, then on each:
stellar contract invoke --id <id> --source-account buyer --network testnet -- lock

# 3. Measure on A (each estimate is a simulation only — state advances only via invoke)
soroban-cost-estimator estimate --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$A" --fn release_advance_1 --network testnet          # while Locked
stellar contract invoke --id "$A" --source-account deployer --network testnet -- release_advance_1
soroban-cost-estimator estimate --wasm … --id "$A" --fn claim_advance_1 --network testnet  # window open
stellar contract invoke --id "$A" --source-account warehouse --network testnet -- mark_checkpoint
soroban-cost-estimator estimate --wasm … --id "$A" --fn release_advance_2 --network testnet # while CheckpointPassed
stellar contract invoke --id "$A" --source-account deployer --network testnet -- release_advance_2
soroban-cost-estimator estimate --wasm … --id "$A" --fn claim_advance_2 --network testnet  # window open

# 4. Measure on B: after release_advance_1, wait out the 15 s window, then
#    estimate reclaim_advance_1 (status still Advance1Released, window passed);
#    mark_checkpoint, release_advance_2, wait again, estimate reclaim_advance_2.
```

## Caveats

- **Estimator version is v0.1.0**, the crates.io release current at this
  writing — the upstream README describes newer flags (`--compare`, snapshot
  retention) that this version doesn't have. The `estimate` path used here is
  stable across both.
- **`Fee (total)` is an upper bound**, not the charged fee — it includes
  simulated TTL-extension rent that a real submission largely gets refunded or
  never pays (see the cross-check). For budgeting, use the non-refundable
  column plus the CLI's real-submission numbers.
- **Every instance here is a validation artifact**, per HANDOFF's standing
  rule — and they've now been walked past these states (A's delivery was
  genuinely confirmed; B, C and D are controls and reproductions).
  Re-running the estimates against *these* IDs will hit state guards and
  error; deploy fresh per the reproduce steps.
- **`confirm_delivery` needs the spec-aware `--arg` patch** — see
  [Tooling finding](#tooling-finding--soroban-cost-estimator-v010-cannot-encode-a-u32-argument).
  Everything else in this document runs on the stock v0.1.0 binary.
- **The two sessions are not directly comparable** — different SDK, different
  WASM. Use the same-session control table for deltas.
- The SAC balance entries these claims/reclaims touched were created moments
  earlier by `lock` on the same instances. A nth-claim in a long-lived
  commitment may differ in the rent half; the execution half is state-size
  independent here.
- **Session 2's instances were funded from the testnet friendbot** and their
  keys are throwaways, regenerated for this session. Do not reuse them for
  anything.

## WASM Upload vs Instantiation

The deploy flow separates two distinct costs:

| Operation | Description | Cost (stroops) | Notes |
|-----------|-------------|----------------|-------|
| **WASM upload** | One-time upload of the compiled `.wasm` to the Soroban network | ~TBD | Paid once per contract version; hash pinned in deployment config |
| **Instantiation** | Creating a new commitment instance from an already-uploaded WASM hash | ~TBD | Marginal cost per commitment; scales with initial state size |

To reproduce these measurements locally:

```bash
# Upload cost (no --fn flag — measures pure upload)
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm

# Instantiation cost (with --id pointing to the deployed contract address).
# v0.1.0's `--arg` takes `key=value` pairs, one per flag, and type-infers each
# value — see the tooling finding above for what that can and cannot express.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id <DEPLOYED_CONTRACT_ID> \
  --fn initialize \
  --arg buyer=<BUYER> --arg cooperative=<SELLER> --arg warehouse_operator=<WAREHOUSE> \
  --arg token=<TOKEN> --arg total_amount=100000000 --arg advance1_bps=1500 \
  --arg advance2_bps=2000 --arg claim_window_secs=3600 --arg remainder_window_secs=3600 \
  --arg delivery_window_secs=86400 --arg contracted_quantity=1000 \
  --arg grade_price_bps=10000,9000,7500 --arg oracle_config=null
```

**This last invocation does not actually work, and the doc says so rather
than pretending.** The estimator's `--arg` can only express a handful of
types (bool, integers, strings) — it cannot express an `Address`, a
`Vec<u32>` or an `Option<OracleConfig>`, all of which `initialize` takes.
Measuring `initialize` this way is not possible with the tool as it stands.

It doesn't matter for cost measurement: the estimator's job here is to
measure an invocation **against an instance that is already deployed and
already initialized**, which is exactly how it is used throughout this
document. The deployment and initialization cost of a *new* instance is
therefore out of reach of this tool, and is left as `~TBD` above rather
than guessed at.

## Reproducibility

WASM builds are verified reproducible in CI (`.github/workflows/test.yml`, job "Verify WASM build reproducibility"). The SHA-256 hash of `target/wasm32v1-none/release/harvestlock_escrow.wasm` must match between two consecutive clean builds. Current hash: see latest CI run.

## Supply-Chain Health

`cargo deny check advisories bans` runs in CI to catch yanked or unmaintained dependencies. See `deny.toml` for configuration. As of this writing, the dependency tree is minimal (`soroban-sdk = "29.0.1"` only) and passes all checks cleanly.
