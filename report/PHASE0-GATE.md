# Phase 0 gate: go

Six spikes, run against the pins in `../UPSTREAM.md` on one Apple M3 Max
(14 cores, 36 GiB). Raw JSON in `../bench/results/`; tables regenerate with
`python3 ../scripts/summarize.py`.

**Decision: go, on the full plan, with one scope correction and one risk
re-ranked.** The spike that was supposed to be the expensive one (S3) came back
free, and the one nominated as top risk (S6) turned out to be a constraint rather
than a ceiling: a 60-million-cycle trace proves on the dev box today.

## What each spike answered

### S1 — keccak inline reachability: reachable, and 2.5x, not 25x

The inline is trivially reachable: `jolt_inlines_keccak256::Keccak256::digest`
is ordinary safe Rust, no build plumbing beyond depending on the crate.

The interesting result is the size of the win. Over a 1 MiB input the inline
costs **24.52 cycles/byte against 60.54** for the `sha3` crate - **2.47x**. Per
keccak-f permutation that is 3,334 cycles, of which only ~75 are real RV64IMAC
instructions and ~3,259 are *virtual* instructions the tracer expands.

That is the single most important architectural fact in this repo. **A Jolt
inline is a virtual instruction sequence, not a precompile.** It still occupies
trace rows one for one, so it buys a constant factor, not an asymptote. Anyone
reading "Jolt has a keccak inline" as equivalent to "SP1 has a keccak precompile"
is reading it wrong, and the benchmark exists partly to make that legible.

Consequence for the plan: the v2 assumption that patching hashes would be the big
lever is only half right. It is a ~2x lever. The plan stands, the expected
magnitude does not.

### S2 — advice tape and heap ceiling: no ceiling worth worrying about

64 MiB through the untrusted advice region works, and the region is
memory-mapped below `RAM_START_ADDRESS` rather than read instruction by
instruction, so a `&[u8]` witness deserializes **by borrow** - no heap copy, no
per-byte read cost. Real EEST witnesses are 5-6 KiB and mainnet-sized ones are
low single-digit MiB, so the channel is three orders of magnitude clear.

Two hard walls, both structural and both cheap to respect:

- `max_untrusted_advice_size` must be a **power of two** (`MemoryLayout::new`
  asserts it).
- The whole I/O region is placed *below* `RAM_START_ADDRESS` = 2 GiB, which caps
  it.

Failure mode is clean and host-side, before anything is proven:

```
Untrusted advice too long: got 100663300 bytes, max is 67108864 bytes
(set by MemoryConfig.max_untrusted_advice_size).
```

Consuming advice costs 12.0 cycles/byte densely, which is why the guest should
keep borrowing rather than parsing eagerly.

**v1's "input ceiling" risk is closed.** It was the wrong risk.

### S3 — `no_std` compile: the spike that was supposed to hurt did not

`stateless-validator-reth` with `std` off, on `riscv64imac-unknown-none-elf`:
**386 crates, compiles clean.** Not "compiles with a damage list" - clean. The
only compile-level requirement is `--cfg getrandom_backend="custom"`, which the
`jolt` CLI already passes.

Linking needed exactly one thing: a `critical-section` implementation. Without
it, any guest reaching `once_cell::sync::OnceCell` - which
`reth_primitives_traits::SealedHeader::hash_ref` does on every header hash -
fails on `_critical_section_1_0_acquire`. A zkVM guest is single-threaded and
uninterruptible, so the correct implementation is a no-op; it lives in
`crates/ere-platform-jolt` because it is a platform fact, not a guest one.

Result: a 2.1 MB RISC-V ELF. The plan's fear that "reth-stateless' `no_std` claim
may be shallower than the `cfg_attr` suggests" was unfounded. The EF wrote that
guest to be portable and it is.

**This is the finding that de-risks the whole project**, and it arrived on day
one of Phase 0 rather than after a week of porting.

### S4 — ecrecover: built, correct, and the biggest single win in the corpus

`jolt-inlines-secp256k1` ships `ecdsa_verify` and no `recover`, and ecrecover is
the hottest precompile in block validation - every transaction pays for one. So
it had to be written.

It is built entirely on the inline crate's **public** API (`Secp256k1Fq`,
`Secp256k1Fr`, `AffinePoint`, GLV decomposition), so it works against an
unmodified Jolt checkout and can be upstreamed as an addition rather than a fork.

One deviation from the plan, taken deliberately. The plan specified hinting R's
y-coordinate through the advice tape and checking it. Since secp256k1 has
`p = 3 (mod 4)`, the square root is a **single fixed-exponent power** - about 500
inline field operations, no advice plumbing, no second emulation pass, and no new
soundness surface to argue about. The advice variant remains open as an
optimisation with a measurable target to beat; it is not a prerequisite.

Correctness is gated, as the plan requires, before any published number:
differential against `k256` on 64 deterministic signatures, plus wrong-recovery-id
(must never return the signer), malformed inputs, and the `r + n > p` case.

Block-level effect: the ecrecover fixture goes from 10.59M cycles to 3.99M -
**2.66x**, second only to p256verify's 2.84x among the corpus workloads.

### S5 — SP1 baseline

See `REPORT.md`; the SP1 column is produced by `sp1-baseline/` through Ere's own
SP1 backend on the same fixtures.

### S6 — trace length and RAM: a real constraint, but a smaller one than feared

Measured with `jolt-prover-legacy` - the prover `#[jolt::provable]` wires up at
this pin - on the 36 GiB dev box. The full ladder with peak memory is regenerated
into `REPORT.md` by `scripts/s6-ladder.sh` and `scripts/summarize.py`; the shape
is:

- proving time linear in the bound - 5.6 s at 2^20 to 155.6 s at 2^26;
- peak memory growing more slowly than the bound - 1.8 GB at 2^20 to 31.3 GB at
  2^26, which **does** fit on a 36 GiB machine;
- verification flat at ~0.07 s across a 64x range of trace length.

(Peak footprint and maximum resident set size are different numbers;
`scripts/s6-ladder.sh` folds both into every result file.)

Verification is flat at ~0.07 s across a 32x range of trace length, which is the
good news nobody was looking for.

**The headline feasibility question, restated with numbers.** A mainnet block is
~10^8 cycles by SP1's counts. A 60-million-cycle trace (2^26) already proves on
the 36 GiB dev box in 2.6 minutes at 31.3 GB peak, so 10^8 cycles is 0.7
doublings further: roughly **45 GB and about four minutes**. That is not a wall,
and it is not even much of a bigger machine - a 64 GiB box covers it.

The default `max_trace_length` of 2^24 reads like a limit and is not one; it is a
default, and S6 prices the alternatives.

So the gap between "2^24 default" and "a mainnet block" is a factor of about 4 in
memory over what this laptop has - not the factor of 100 the framing feared. The
default is a *default*, not a limit; the plan already knew it was configurable and
S6 prices the configuration.

## Scope correction

The plan's Track B called for `jolt-patches` mirroring `sp1-patches`: forks of
`sha2`, `sha3`, `k256`, `p256` swapped in through `[patch.crates-io]`.

Not built, and deliberately. eth-act's guest already has a supported seam for
exactly this - the `zkvm-interface` feature routes every hash, signature and
curve operation through the zkvm-standards C ABI, which is how SP1 gets its
acceleration in this harness via `libzkevm`. Implementing that ABI
(`crates/jolt-zkvm-accel`) reaches the same crypto with:

- no forked crates to rebase against upstream RustCrypto releases,
- acceleration of the **SSZ hash-tree-root** as well, which the patch route
  reaches only through `sha2` and which turns out to be ~36% of a small block,
- the identical configuration SP1 is measured in, which is the point of a
  comparison,
- and honest fallbacks: everything with no Jolt substrate (bn254, BLS12-381, KZG,
  modexp, ripemd160, blake2f) forwards to revm's own software backend, so
  enabling the seam can only ever remove cycles, never add them.

The reusable byproduct the plan wanted still exists; it is a C ABI implementation
rather than a crate-patch set. `patches/README.md` records the choice.

## Risks, re-ranked

| risk | v2 rank | now |
|---|---|---|
| trace-length / RAM ceiling | top | **demoted**: 2^26 (60M cycles) proves on the 36 GiB box at 31.3 GB peak in 2.6 min; mainnet-scale projects to ~45 GB |
| `reth-stateless` `no_std` shallower than advertised | high | **closed** - compiles clean, 386 crates |
| advice/input ceiling | medium | **closed** - 64 MiB fine, witnesses are 6 KiB |
| ecrecover soundness | high | **mitigated** - differential-tested against `k256`; the advice variant that carried the real soundness argument was not needed |
| Jolt prover refactor churn | medium | **unchanged** - `#[jolt::provable]` still wires `jolt-prover-legacy`; the new `jolt-prover` is not reachable from the guest macro at this pin, so every number here is legacy-prover |
| Ere backend API conformance | medium | **closed** - the four crates exist and pass an end-to-end compile/execute/prove/verify test |
| inline speedup smaller than assumed | not listed | **new, medium** - inlines are ~2x, not orders of magnitude; the report leads with this rather than burying it |

## What the pivot options turned out to be worth

The plan named two pivots if S6 went badly:

- *small-block corpus with the scaling curve as the finding* - this is what the
  campaign does anyway, because the public EEST corpus is small blocks. It is a
  legitimate result, not a consolation prize, and S6 supplies the curve.
- *execution-only benchmarking as an interim artifact* - unnecessary. Real blocks
  prove and verify on this hardware in seconds.

Neither pivot is needed. Proceed to Phases 1-4 as written, with the report
leading on the inline-versus-precompile distinction, which is the finding a
reader of the original plan would not have predicted.
