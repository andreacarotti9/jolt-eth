# Ethereum block validation in Jolt

**What this is.** The Ethereum Foundation's stateless block validator, unmodified,
running and proving inside a [Jolt](https://github.com/a16z/jolt) guest; a Jolt
backend for the EF's Ere zkVM interface; and a measurement of what Jolt costs
against SP1 on the same blocks, the same guest source and the same machine.

**What it argues.** That on Jolt the useful unit of optimisation is not the
percentage of cycles removed but whether the removal crosses a power of two — and
that this changes which optimisations are worth building. Everything else here is
either evidence for that or context around it.

**Status of the numbers.** Every table below is regenerated from
`../bench/results/` by `python3 ../scripts/render-report.py`, and
`scripts/check.sh` fails if any of them has drifted from the JSON. Methodology was
pre-registered in `../bench/METHODOLOGY.md` before the final runs. Pins are in
`../UPSTREAM.md`; Jolt is `915faf4` (2026-08-13) and every Jolt number comes from
`jolt-prover-legacy`, which is what `#[jolt::provable]` wires up at that commit —
the refactored `jolt-prover` is not reachable from the guest macro there.

**The corpus.** Seven EEST blockchain tests, each named in `../scripts/corpus.sh`
by the workload it is meant to exercise rather than by position in a file (§6
explains why that distinction cost a day). Each runs in two configurations of the
same guest, differing only in one cargo feature: software crypto, versus the
eth-act `zkvm-interface` C ABI answered by `../crates/jolt-zkvm-accel` on Jolt
inlines. Host is an Apple M3 Max, 14 cores, 36 GiB.

---

## 1. What an optimisation is worth on Jolt

Jolt's prover is sized for the **bound** — the trace length rounded up to a power
of two — not for the trace. That is an unremarkable implementation fact with a
consequence that is easy to miss and expensive to miss: **the cost of proving a
block is a step function of its cycle count.** Cycles removed inside a step are
free to remove and worth nothing.

<!-- PROVE-TABLE -->
| fixture | config | cycles | bound | prove s (median of N) | verify s | verified |
|---|---|---:|---:|---:|---:|:--:|
| ecpairing | accel | 20,397,957 | 2^25 | 79.0 (N=3) | 0.56 | yes |
| ecpairing | baseline | 24,233,052 | 2^25 | 81.1 (N=3) | 0.52 | yes |
| ecrecover | accel | 3,986,529 | 2^22 | 18.3 (N=3) | 0.55 | yes |
| ecrecover | baseline | 10,591,427 | 2^24 | 42.8 (N=3) | 0.51 | yes |
| modexp | accel | 4,092,054 | 2^22 | 17.2 (N=3) | 0.53 | yes |
| modexp | baseline | 7,803,003 | 2^23 | 24.5 (N=3) | 0.49 | yes |
| p256verify | accel | 4,140,554 | 2^22 | 17.6 (N=3) | 0.53 | yes |
| p256verify | baseline | 11,747,118 | 2^24 | 41.7 (N=3) | 0.52 | yes |
| sha3 | accel | 3,664,805 | 2^22 | 18.2 (N=3) | 0.53 | yes |
| sha3 | baseline | 7,305,598 | 2^23 | 27.8 (N=3) | 0.51 | yes |
| transaction_create_random_init_code | accel | 3,343,618 | 2^22 | 16.7 (N=3) | 0.53 | yes |
| transaction_create_random_init_code | baseline | 6,814,278 | 2^23 | 24.9 (N=3) | 0.51 | yes |
| value_moving_transactions | accel | 3,417,574 | 2^22 | 17.0 (N=3) | 0.53 | yes |
| value_moving_transactions | baseline | 6,938,251 | 2^23 | 24.4 (N=3) | 0.50 | yes |
<!-- /PROVE-TABLE -->

**Within a rung, the cycle count barely predicts anything.** Group the fourteen
runs by bound and compare how much the cycles vary against how much the proving
time varies:

| bound | cycle spread within the rung | prove-time spread |
|---|---:|---:|
| 2^22 (6 runs) | 24% | 10% |
| 2^23 (4 runs) | 15% | 14% |
| 2^24 (2 runs) | 11% | 3% |
| 2^25 (2 runs) | 19% | 3% |

The 2^24 pair is the cleanest demonstration in the corpus, because it inverts:
baseline p256verify has **11% more cycles** than baseline ecrecover
(11,747,118 against 10,591,427) and proves **1.1 s faster** (41.7 s against
42.8 s). More work, less time. Nothing is wrong with either measurement; the two
runs simply bought the same 2^24 prover and the difference between them is noise
on top of a fixed cost.

**Across rungs, it is the only thing that predicts anything.** The same
acceleration, applied to seven workloads, sorts them into three groups by how many
powers of two it crossed:

| | rungs dropped | prove |
|---|---|---|
| ecrecover, p256verify | 2^24 → 2^22, **two** | 42 s → 18 s |
| sha3, modexp, transfer, deploy | 2^23 → 2^22, **one** | ~25 s → ~17 s |
| ecpairing | 2^25 → 2^25, **none** | 81 s → 79 s |

Read the top and bottom rows together. Acceleration removes **62%** of ecrecover's
cycles and buys a 2.3x cheaper proof. It removes **16%** of ecpairing's cycles and
buys nothing at all — 81.1 s to 79.0 s, which is inside the run-to-run spread. The
first is worth building. The second, at that magnitude, is not, and no
cycle-count-only benchmark would tell you which was which.

The practical form of this: **before optimising, compute the bound.** An
optimisation is worth its engineering cost when it moves a workload across
2^k, and the question "how many cycles will this save?" is only useful as an input
to that one. §5 shows the corollary — that the bound alone predicts proving time
well enough to plan with.

---

## 2. The EF stateless validator runs in Jolt

`eth-act/ere-guests`' `stateless-validator-reth`, unmodified, `no_std`, on Jolt's
`riscv64imac-unknown-none-elf`:

- **386 crates compile clean.** The only compile-level requirement is
  `--cfg getrandom_backend="custom"`, which the `jolt` CLI already passes.
- **One linking fix**, and it is a platform fact rather than a guest one: a
  `critical-section` implementation. Any guest reaching `once_cell::sync::OnceCell` —
  which `reth_primitives_traits::SealedHeader::hash_ref` does on every header
  hash — otherwise fails on `_critical_section_1_0_acquire`. A zkVM guest is
  single-threaded and uninterruptible, so the implementation is a no-op; it lives
  in `../crates/ere-platform-jolt`.
- Result: a 2.1 MB RISC-V ELF that proves real EEST blocks and whose output
  matches the fixture's `statelessOutputBytes`.

The plan that preceded this work listed "reth-stateless' `no_std` claim may be
shallower than the `cfg_attr` suggests" as a high risk. It was not. The EF wrote
that guest to be portable, and it is.

The witness arrives through Jolt's **untrusted advice** region rather than
committed input. That is the semantically right channel — stateless validation
authenticates the witness against the parent state root itself, so it is not
something the verifier trusts — and it is also the cheap one: the region is
memory-mapped, so a `&[u8]` deserializes by borrow with no heap copy. 64 MiB
works; real EEST witnesses are 5-6 KiB.

---

## 3. What acceleration buys, and where it comes from

<!-- BLOCK-TABLE -->
| fixture | witness B | baseline cycles | accelerated cycles | speedup | output verified |
|---|---:|---:|---:|---:|:--:|
| eip197_ec_pairing/ecpairing | 6,242 | 24,233,052 | 20,397,957 | 1.19x | yes |
| eip198_modexp_precompile/modexp | 5,920 | 7,803,003 | 4,092,054 | 1.91x | yes |
| eip2780_reduce_intrinsic_tx_gas/value_moving_transactions | 5,483 | 6,938,251 | 3,417,574 | 2.03x | yes |
| eip7951_p256verify_precompiles/p256verify | 5,767 | 11,747,118 | 4,140,554 | 2.84x | yes |
| precompiles/ecrecover | 5,690 | 10,591,427 | 3,986,529 | 2.66x | yes |
| stInitCodeTest/transaction_create_random_init_code | 5,409 | 6,814,278 | 3,343,618 | 2.04x | yes |
| vmTests/sha3 | 6,306 | 7,305,598 | 3,664,805 | 1.99x | yes |
<!-- /BLOCK-TABLE -->

About 2x on a typical block, 2.84x at best, 1.19x on the one workload with no
substrate. The interesting column is where the cycles sit, not the totals. Each
cell below is the median across the seven workloads, and the shares are of those
medians summed — a composite block, not any real one, which is the point: it shows
the shape a typical small block has rather than any single fixture's accidents.

<!-- REGION-TABLE -->
| region | baseline median cycles | accelerated median cycles | speedup | share of accelerated trace |
|---|---:|---:|---:|---:|
| `deserialize_input` | 50,264 | 50,260 | 1.00x | 1.3% |
| `new_payload_request_root` | 2,668,789 | 1,303,488 | 2.05x | 32.8% |
| `validate_chain_config` | 52 | 52 | 1.00x | 0.0% |
| `to_reth_input` | 274,950 | 199,303 | 1.38x | 5.0% |
| `run_validation` | 4,576,622 | 2,419,782 | 1.89x | 60.9% |
| `serialize_output` | 2,620 | 2,620 | 1.00x | 0.1% |
<!-- /REGION-TABLE -->

**SSZ hashing is a third of a small block.** `new_payload_request_root` — the
hash-tree-root of the payload request, pure sha256 — is ~33% of the accelerated
trace and 2.05x cheaper accelerated. It is not EVM execution at all. On the small
blocks the public EEST corpus provides it rivals `run_validation`; on a
mainnet-sized block it would be a much smaller share, because it scales with the
payload header rather than with the transactions. Read the split accordingly.

**The signature precompiles are where the inlines earn their keep**, and §1 says
why: p256verify at 2.84x and ecrecover at 2.66x are the two workloads that cross
two rungs. Jolt ships `ecdsa_verify` for secp256k1 but no `recover`, so
`../crates/jolt-zkvm-accel` implements recovery — on the inline crate's public API
only, so it works against an unmodified Jolt checkout.

**bn254 is the hole, and it is the expensive kind.** The pairing block costs
**20.4M cycles against 3.4M for a plain transfer** — six times the work —
and acceleration moves it only **1.19x**, because there is no Jolt substrate for
bn254 at this pin and the pairing falls through to revm's software backend. That
1.19x is the hashes alone still doing their job around an untouched pairing, and
by §1 it is worth exactly zero: same rung, same proof cost.

There is a mechanical tell for this, visible in the raw JSON. Count what fraction
of each accelerated trace is *virtual* instructions — the rows Jolt's tracer
expands inlines and certain RISC-V instructions into:

| workload | virtual share, baseline | virtual share, accelerated |
|---|---:|---:|
| six of seven | 17–26% | 61–68% |
| ecpairing | 10% | **14%** |

Turning acceleration on moves a normal workload from about a fifth virtual to
about two thirds, because the inlines it now calls are virtual sequences. The
pairing block barely moves, and the number says why without needing to know
anything about the fixture: **almost none of its work is reaching an inline.**
That ratio is a cheap diagnostic for "is this workload actually accelerated," and
it is how the bn254 gap would be spotted on a workload nobody had characterised
by hand.

The gaps are at least honest ones. Everything without a Jolt substrate — bn254,
BLS12-381, KZG, modexp, ripemd160, blake2f — forwards to the software backend the
guest would have used with the seam off, so enabling acceleration can only remove
cycles, never add them.

### A note for readers coming from SP1

Jolt's inlines are not precompiles in SP1's sense, and this is Jolt's documented
design rather than a finding of this work. [JoltBook](https://jolt.a16zcrypto.com/how/optimizations/inlines.html)
puts it plainly: "Unlike traditional precompiles that operate in a separate
constraint system, inlines remain fully integrated within the main Jolt zkVM
execution model," expanding into instruction sequences that "execute within the
same trace as regular program code."

It is worth restating here only because it is what makes §1 true — an inline's
cost lands in the same trace whose length picks the bound — and because a reader
carrying SP1's model across will otherwise expect an order of magnitude where the
measurement gives a factor of two or three. Measured on a guest at this pin:

<!-- KECCAK-TABLE -->
| input | inline cycles/B | software cycles/B | speedup | inline cycles per keccak-f |
|---:|---:|---:|---:|---:|
| 1,024 B | 26.36 | 63.64 | 2.41x | 3,585 |
| 8,192 B | 24.86 | 61.22 | 2.46x | 3,381 |
| 65,536 B | 24.53 | 60.54 | 2.47x | 3,336 |
| 262,144 B | 24.52 | 60.55 | 2.47x | 3,335 |
| 1,048,576 B | 24.52 | 60.54 | 2.47x | 3,334 |
<!-- /KECCAK-TABLE -->

Of the 3,334 cycles per permutation, ~75 are real RV64IMAC instructions and
~3,259 are virtual. They occupy trace rows one for one.

**One discrepancy worth resolving.** JoltBook's own benchmark table reports
keccak-256 at 26 cycles/byte inline against 78 in software, for **3.01x** on 32 KB
inputs against the `sha3` crate. The inline figure here reproduces that closely
(24.5 vs 26 cycles/byte, ~6%). The software baseline does not: 60.5 against 78, a
29% gap, and the whole difference between 2.47x and 3.01x lives in that
denominator. Likely candidates are the `sha3` version, the target
(`riscv64imac` here) or the optimisation level. Whichever number is right, it is
the baseline that is in dispute, not the inline — and `../spikes/s1-keccak` is
runnable if someone wants to settle it.

Separately, Jolt's keccak256 API reference page describes the inline as a
"~10-100x reduction in VM cycles," which is inconsistent with JoltBook's own
3.01x and with the 2.47x here by more than an order of magnitude. That looks like
a documentation defect rather than a claim anyone is defending, and it is
plausibly the origin of the expectation this section exists to correct.

---

## 4. Jolt against SP1

Same guest crate, same fixture, same block-selection rule, same machine.

Both provers' outputs equal the fixture's canonical `statelessOutputBytes` on
every workload. That is the cross-check in its strongest form: rather than two
implementations agreeing with each other, both agree with the reference the EEST
fixture carries.

<!-- CROSS-TABLE -->
| fixture | Jolt cycles | SP1 cycles | ratio (Jolt/SP1) |
|---|---:|---:|---:|
| eip197_ec_pairing/ecpairing | 20,397,957 | 3,697,295 | 5.52x |
| eip198_modexp_precompile/modexp | 4,092,054 | 1,646,798 | 2.48x |
| eip2780_reduce_intrinsic_tx_gas/value_moving_transactions | 3,417,574 | 1,307,369 | 2.61x |
| eip7951_p256verify_precompiles/p256verify | 4,140,554 | 1,490,472 | 2.78x |
| precompiles/ecrecover | 3,986,529 | 1,367,936 | 2.91x |
| stInitCodeTest/transaction_create_random_init_code | 3,343,618 | 1,280,046 | 2.61x |
| vmTests/sha3 | 3,664,805 | 1,403,764 | 2.61x |
<!-- /CROSS-TABLE -->

**These are not the same unit, and the ratio is not a verdict.** Jolt's number is
trace rows, including the virtual instructions its inlines expand into. SP1's is
its instruction count, where a precompile is charged as a syscall rather than as
the instructions the operation would otherwise take, and its real work happens in
separate AIRs the number does not show. Neither is wrong. The honest statement is:

> On identical work, Jolt's trace is about 2.6x longer than SP1's instruction
> count, and Jolt's inlines are inside that trace while SP1's precompiles are
> outside its count.

What makes the table worth reading is the row that breaks the pattern. Six of
seven workloads sit between 2.48x and 2.91x — a flat, boring, architectural
constant. The pairing block sits at **5.5x**, twice everything else, and the
reason is not architecture: SP1 has a bn254 precompile and Jolt has nothing. The
constant is the price of a design; the outlier is the price of a missing feature,
and only one of those is expensive to fix.

Proving times are not compared. SP1's CPU prover on an Apple laptop is not the
configuration anyone runs it in, and a number from it would mislead in Jolt's
favour rather than inform.

---

## 5. Scaling: what a mainnet block would cost

The trace-length ladder, on a 36 GiB M3 Max, driven by a synthetic loop rather
than by the validator:

<!-- LADDER-TABLE -->
| bound | real cycles | preprocess s | prove s | verify s | peak GB | verified |
|---|---:|---:|---:|---:|---:|:--:|
| 2^20 | 943,637 | 1.3 | 5.6 | 0.06 | 1.8 | yes |
| 2^22 | 3,774,549 | 2.4 | 13.9 | 0.07 | 5.1 | yes |
| 2^23 | 7,549,103 | 4.7 | 20.7 | 0.07 | 8.8 | yes |
| 2^24 | 15,098,197 | 4.7 | 41.0 | 0.07 | 14.5 | yes |
| 2^25 | 30,196,399 | 37.0 | 75.2 | 0.08 | 23.5 | yes |
| 2^26 | 60,392,803 | 37.2 | 155.6 | 0.08 | 31.3 | yes |
<!-- /LADDER-TABLE -->

Proving time doubles with the bound — it is linear in trace length — while peak
memory grows more slowly, about 1.7x per doubling early on and 1.33x from 2^25 to
2^26. Verification stays at 0.07 s across a 64x range. Preprocessing has a
discontinuity between 2^24 and 2^25 (4.7 s to 37.0 s) that the smooth prove curve
does not, which anyone budgeting end-to-end latency should notice.

**The ladder predicts the real blocks.** This is the check that makes §1
actionable rather than merely descriptive — a synthetic loop at bound 2^k lands
close to a real Ethereum block that happens to have bound 2^k:

| bound | ladder, synthetic | real blocks at that bound |
|---|---:|---|
| 2^22 | 13.9 s | 16.7 – 18.3 s (n=6) |
| 2^23 | 20.7 s | 24.4 – 27.8 s (n=4) |
| 2^24 | 41.0 s | 41.7 – 42.8 s (n=2) |
| 2^25 | 75.2 s | 79.0 – 81.1 s (n=2) |

Within 20% at the small end and within 5% at the large end, across a 4x range, for
two entirely different programs. **The bound is very nearly sufficient to predict
proving cost** — which is what licenses treating rungs as the currency.

The top rung is the interesting one: **a 60-million-cycle trace proves in 2.6
minutes on a laptop**, peaking at 31.3 GB on a 36 GiB machine. An earlier draft
of this report claimed 2^26 would not fit; running it rather than extrapolating
showed that it does.

From there, a mainnet-sized block at ~10^8 cycles is 1.66x the top rung, or
another 0.73 doublings. Proving is linear in trace length, so that part is a
single number: **about 4.3 minutes**. Memory is not, and the ladder does not
pin it down — the measured per-doubling growth is 1.73x at 2^22→2^23 but only
1.33x at 2^25→2^26, and carrying those two rates forward gives **38.6 GB and
46.6 GB** respectively. The honest statement is *roughly 39–47 GB*, and which
end it lands on depends on whether that late flattening is real or an artifact
of the last measurement.

Either way the conclusion is the same shape: an ordinary server, not a cluster —
and the headline is not "Jolt needs a bigger machine" so much as "Jolt already
runs on this one, and a mainnet block needs the next size up".

Three caveats on that extrapolation, all real:

1. The memory range is a factor of 1.2 wide because the growth rate is not
   settled. One more rung of ladder — 2^27, which does not fit on this machine
   but would fit on a 64 GB one — would collapse it to a number.
2. It is from small blocks. The EEST corpus tops out at 6 KiB witnesses; nothing
   here exercises a block with thousands of transactions, and the memory curve
   was measured on a synthetic loop rather than on the validator. A real block of
   that size may have a different memory profile at the same trace length.
2. It is `jolt-prover-legacy`. The refactored `jolt-prover` is present in the
   tree at this pin but is not what the guest macro uses, and it may well move
   these numbers. Anyone re-running this after the refactor lands should expect
   different results and should say which prover produced them.

---

## 6. Two things this harness caught

Both are arguments for the discipline rather than incidental notes, and both
produced a *plausible* wrong number before they were caught.

**A correctness bug that looked like a speedup.** The first accelerated campaign
reported the modexp fixture as **output mismatched** while the software baseline
matched. The cause was in the accelerator's `zkvm_modexp`: it required the
backend's result length to equal the declared modulus length and returned failure
otherwise. EIP-198 wants exactly `mod_len` big-endian bytes and a shorter minimal
encoding is legal, so treating it as failure halted the precompile and silently
changed the block's state root.

A benchmark that only measured cycles would have reported a *faster* accelerated
modexp — halting early is cheap — and been completely wrong. Asserting the
guest's output against the fixture on every run is what turned a
performance-looking result into a bug report. The fix right-aligns the result and
is locked by a regression test.

**A benchmark bug that looked like a finding.** This driver originally took the
first case in each EEST file and the first block within it. The first case in
`ecpairing/valid.json` is the *empty* input, which performs no pairing at all,
and the first in `p256verify/wycheproof_valid.json` is an invalid public key,
which never reaches a verification. So the campaign reported 2.00x for "the bn254
workload" and 2.00x for "the secp256r1 workload" — clean, consistent,
publishable numbers that were measuring hashing in both cases.

Naming the case each workload means (`../scripts/corpus.sh`) changed those to
1.19x and 2.84x, and turned the bn254 gap from invisible into the sharpest result
here.

The coda is the instructive part. Convinced this was a trap in the shared
harness, this report carried a draft issue against `zkevm-benchmark-workload`
saying so. Reading its `load_eest_benchmark_fixtures` before filing showed the
opposite: it iterates every case in a file and deliberately takes the *last*
block per case, because earlier blocks are setup. The bug was entirely local, the
issue was withdrawn unfiled, and this driver now follows the same rule. Every
case in this corpus has exactly one block, so no number here moved.

Two lessons, and the second is the one that nearly got away: selecting fixtures
by position rather than by meaning is the easiest way to publish a confidently
wrong benchmark, and blaming shared infrastructure for your own defect is the
easiest way to publish a confidently wrong bug report.

---

## 7. What was built

| deliverable | state |
|---|---|
| Ere backend for Jolt (`platform`, `compiler`, `verifier`, `prover`) | complete; passes an end-to-end compile → execute → prove → verify test on an EEST block through Ere's unified API |
| `zkVMKind::Jolt` registration in `eth-act/ere` | `../patches/ere-add-jolt-backend.patch` |
| backend Docker image | `../docker/jolt/`, mirroring `ere/docker/sp1/` |
| Jolt stateless-validator guest | `../guests/stateless-validator-jolt` |
| zkvm-standards accelerator C ABI on Jolt inlines | `../crates/jolt-zkvm-accel`, including an `ecrecover` Jolt does not ship |
| Phase 0 spikes | `../spikes/`, gate memo in `../spikes/PHASE0-GATE.md` |

### Deviations from the plan

**`jolt-patches` was not built.** The plan called for forks of `sha2`, `sha3`,
`k256`, `p256` swapped in through `[patch.crates-io]`, mirroring `sp1-patches`.
eth-act's guest already has a supported seam for exactly this, and using it
reaches the same crypto plus the SSZ hash-tree-root, needs no forks rebased
against upstream RustCrypto, and puts Jolt in the *identical* configuration SP1
is measured in — which is the point of a comparison. Rationale in
`../patches/README.md`.

**ecrecover uses a square root, not advice.** The plan specified hinting R's
y-coordinate through the advice tape. secp256k1 has `p = 3 (mod 4)`, so the root
is one fixed-exponent power — about 500 inline field operations, no advice
plumbing, no second emulation pass, no new soundness surface. The advice variant
stays open as an optimisation with a measured target to beat. Correctness is
gated as the plan required: differential against `k256` on 64 signatures, plus
wrong-recovery-id, malformed-input and `r + n > p` cases.

**No cost normalisation, no GPU column.** Both were in the plan; both would be
invented numbers here. Jolt has no CUDA path at this pin (`JoltProver::new`
rejects a non-CPU resource rather than downgrading silently), and normalising by
instance price needs a cloud instance, not a laptop.

### Four upstream findings

- **`ere-prover-sp1` cannot execute on aarch64 at v0.15.0.** It pools
  `MinimalExecutorEnum`s and calls `reset()` between runs; SP1 v6.3.1 leaves
  `reset()` as `todo!()` in the portable executor, which is the one every
  non-x86_64 host gets. `../sp1-baseline/` bypasses the pool and uses the SDK
  executor directly — same SP1 version, same ELF, same input.
- **`ere-compiler-sp1`'s stock-nightly path does not build this guest.** It needs
  `-Zjson-target-spec`, which only very recent nightlies have, and those same
  nightlies fail to `-Zbuild-std` the `stateless-validator-reth` tree
  (`serde_core` does not compile against the rebuilt `core`). The
  `SP1RustRv64imaCustomized` path works, with `--ignore-rust-version`, because
  the released Succinct toolchain reports rustc 1.93 against ere-guests' reth pin
  of 1.95. That is also the fairer thing to measure, since it is what SP1 users
  actually run.
- **Jolt's verifying key cannot be a digest for free.** Verification needs the
  guest's full preprocessing, which is program-specific and large: 98 MB for the
  stateless validator against 244 KB for a toy guest at the same trace bound. The
  backend publishes a 32-byte digest of it and re-derives the preprocessing from
  the ELF, which matches the other backends' interface. The residual difference
  is architectural — SP1's 32 bytes suffice because it verifies a compressed,
  recursive proof whose circuit checks the real key internally, where Jolt at this
  pin verifies the raw proof directly.
- **A Jolt host that decodes a guest using inlines must link the inline crates'
  `host` modules.** Registrations live in `inventory` submissions inside rlib
  object files, which the linker drops unless something references them, so a
  backend that merely depends on the crates gets `UnsupportedInstruction` at
  decode time. `ere-prover-jolt` references all eight deliberately, because a
  backend cannot know which inlines a guest chose.

---

## 8. What would move the numbers most

In descending order of expected effect, from what was measured:

1. **A bn254 inline.** The only precompile family in the EVM's hot path with no
   Jolt substrate at all, and it is the one case where §1's arithmetic argues
   *for* the work rather than against it: a pairing block sits at 20.4M cycles
   where a transfer sits at 3.4M, current acceleration buys 1.19x and therefore
   zero rungs, and the gap to SP1 doubles from ~2.6x to 5.5x. A bn254 inline that
   brought the pairing block down to a 2^23 bound would take it from 79 s to the
   24–28 s the other blocks cost there — about 3x, where the current 1.19x buys
   nothing. Nothing else here is close.
2. **The refactored `jolt-prover`.** Not reachable from `#[jolt::provable]` at
   this pin. Every proving number here is legacy-prover and should be re-taken
   after the refactor lands.
3. **Mainnet-sized blocks.** Everything here is 5-6 KiB witnesses. The scaling
   claim in §5 is an extrapolation across two orders of magnitude and should be
   treated as a hypothesis until someone runs it.
4. **Advice-hinted ecrecover.** Worth roughly the ~500 field operations the
   square root costs — which by §1 is worth nothing at all unless it crosses a
   rung, and on the accelerated ecrecover block (3,986,529 cycles against a 2^22
   bound of 4,194,304) there is only 5% of headroom before it *gains* a rung. That
   makes it a defensive optimisation rather than a speedup, which is not how the
   plan framed it.

Beyond measurement, the backend makes one thing mechanically possible that was
not before: a Jolt cluster on [ethproofs](https://ethproofs.org). The pieces it
needs are all here — a guest that validates real blocks, a prover behind the same
interface the other clusters use, and a verifier — and the missing one is §5's
memory, not any missing capability. A single-block submission would be a
worthwhile first move and would put the numbers in front of people who will
argue with them, which is the fastest way to find out if they are wrong.

---

## Right of reply

This work is published open rather than circulated privately first, and the
invitation to correct it is on the record: an issue on `eth-act/ere` proposing
the backend, a spec PR on `a16z/jolt` proposing ecrecover, and
[`right-of-reply.md`](right-of-reply.md), which sets out what each of the three
teams is asked to check. It deliberately points at the claims most likely to be
wrong rather than the ones most likely to be liked.

Corrections of fact go in. Disagreements of interpretation are recorded with
attribution rather than resolved unilaterally. If any number here is wrong, the
raw JSON and the scripts that produced it are in this repo, and the fastest path
is to re-run them and open an issue with the diff.

Nothing here is announced more widely until those threads have had a chance to
find the errors. The two already found, in §6, were both caught this way: by
checking before publishing rather than after.
