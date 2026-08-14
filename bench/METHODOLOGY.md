# Methodology

Pre-registered before the final numbers were taken, per the plan. Anything the
runs later contradicted is recorded in `report/REPORT.md` as a deviation rather
than edited away here.

## What is measured

**Cycles** are Jolt trace rows: real RV64IMAC instructions retired plus the
virtual instructions the tracer expands inlines into. That total is the quantity
`max_trace_length` bounds and the quantity proving cost scales with, so it is the
one that matters. Reporting only real instructions would flatter the inlines by
hiding their cost, which is exactly the thing under test.

Cycles are deterministic for a fixed (ELF, input), so they are reported from a
single run. Only **wall-clock and memory** need repetition.

**Wall clock** is measured around preprocessing, proving and verification
separately, because preprocessing is amortisable across blocks and proving is not.

**Peak memory** is `maximum resident set size` and `peak memory footprint` from
`/usr/bin/time -l`, captured for the trace-length ladder (`scripts/s6-ladder.sh`),
where memory is the binding constraint and each rung runs in its own process so a
crash at one cannot be attributed to another. The block campaign reports time
only: its memory is a function of the trace-length bound, which the ladder
already maps, so measuring it per fixture would add noise rather than
information.

## Configurations

Two, differing only in the guest's `accel` cargo feature:

- **baseline**: `stateless-validator-reth` with its own software crypto - the
  RustCrypto and revm implementations any Rust program would get.
- **accel**: the same guest with the eth-act `zkvm-interface` seam on, answered
  by `jolt-zkvm-accel` (keccak256, sha256, secp256k1 verify and ecrecover,
  secp256r1 on Jolt inlines; everything else forwarded to revm's software
  backend, so the seam can only remove cycles).

Both are built from one checkout by the same driver, which adds the feature per
run. Nothing else differs - same guest source, same fixture, same memory layout.

## Corpus

Seven EEST `blockchain_tests` fixtures from `tests-zkevm@v0.6.2`, one per
workload: value transfer, keccak-heavy, ecrecover, bn254 pairing, modexp,
secp256r1, contract deploy. `scripts/corpus.sh` is the single definition both
drivers read, so the columns line up by construction.

Within a case, the **last** block is used, matching
`zkevm-benchmark-workload`'s `load_eest_benchmark_fixtures`: earlier blocks are
setup and the last is the workload. Every case in this corpus has exactly one
block, so the rule changes nothing here, but taking the first would measure setup
on any fixture that had more.

**Each workload names its case, and that is not a detail.** An EEST file holds
many cases and the first is frequently degenerate: `ecpairing/valid.json` opens
with the *empty* input, which performs no pairing at all, and
`p256verify/wycheproof_valid.json` opens with an invalid public key, which never
reaches a verification. An earlier run of this campaign took the first case in
each file and would have reported "the bn254 workload" for a block that does no
bn254 work - with a 2.0x speedup that was really just the hashes. The corrected
corpus reports 1.19x for a real pairing. Selecting by position rather than by
meaning is the easiest way to publish a confidently wrong benchmark.

These are small blocks (5-6 KiB witnesses). They are what the public EEST corpus
provides; a mainnet-sized block is a different regime and the report says where
the extrapolation stops being safe.

## Correctness gate

A performance number from a guest that computed the wrong thing is worthless, so
every run asserts the guest's output equals the fixture's `statelessOutputBytes`,
and the tables carry that column. Runs that fail it are reported as failures, not
dropped: the modexp fixture failed exactly this check on the first accelerated
campaign and the bug it exposed is in the report.

The accelerated crypto has its own gate below the block level:
`crates/jolt-zkvm-accel` differential-tests ecrecover against `k256` on 64
signatures plus malformed, wrong-recovery-id and out-of-range-x cases, and both
inline hashes against RustCrypto across padding boundaries.

## Environment

Single machine, stated in every result file: Apple M3 Max, 14 cores, 36 GiB, macOS
15.6 (Darwin 25.6.0), Rust 1.95 for guests and 1.97 for hosts. Every upstream is
pinned in `UPSTREAM.md`; Jolt is mid-refactor between `jolt-prover` and
`jolt-prover-legacy`, so each table states which prover produced it.

No GPU column: Jolt has no CUDA path at the pinned commit, and
`JoltProver::new` rejects a non-CPU resource rather than silently downgrading.
An SP1-GPU column would therefore be comparing different hardware, and is
excluded rather than labelled.

## Cost normalisation

Deliberately omitted. Normalising by instance price needs a cloud instance both
provers actually run on, and these runs are on one laptop. Reporting a
dollars-per-proof derived from a laptop would be a made-up number.
