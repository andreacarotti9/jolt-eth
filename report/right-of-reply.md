# Right of reply

Three audiences, three different things worth checking. Each note is short on
purpose: the report is the artifact, these say which parts most deserve an
expert's scepticism.

Sent openly rather than privately, through the issue and spec threads linked
below, so that a correction is visible to everyone who reads the claim it
corrects.

Corrections of fact go in. Disagreements of interpretation are recorded with
attribution rather than resolved unilaterally.

---

## To a16z / Jolt

You are the audience most able to tell me I have measured Jolt badly, so here is
where I would look first.

**The claim I most want checked — a baseline, not a framing.** I measure
keccak-256 at **24.5 cycles/byte inline against 60.5 in software**, for 2.47x.
[JoltBook](https://jolt.a16zcrypto.com/how/optimizations/inlines.html) reports 26
against 78, for 3.01x on 32 KB inputs versus the `sha3` crate. The *inline*
numbers agree within about 6%, which is the reassuring part. The *software*
numbers do not: 60.5 against 78 is a 29% gap, and the entire difference between
2.47x and 3.01x lives in that denominator.

I do not know which baseline is right. The candidates I can think of are the
`sha3` version, the target (I am on `riscv64imac`; JoltBook does not say), the
optimisation level, or a difference in how the driver excludes setup from the
measured region. `spikes/s1-keccak` is a standalone binary and runs in seconds if
you want to point it at your configuration. Whatever the answer, it is a
correction to one of two published numbers, and I would rather it be mine and
early than mine and cited.

**A documentation defect, offered rather than filed.** Jolt's keccak256 API
reference page describes the inline as a "~10-100x reduction in VM cycles." That
is inconsistent with JoltBook's own 3.01x by more than an order of magnitude, and
I think it is the origin of a fairly widespread expectation that Jolt's inlines
behave like SP1's precompiles. My report spends a section correcting that
expectation; a one-line docs fix would do more good than the section will. Happy
to open the PR if useful.

**Three more things I may have got wrong.**

1. Every number is `jolt-prover-legacy`, because that is what `#[jolt::provable]`
   wires up at `915faf4`. If `jolt-prover` is reachable a different way, or if
   the refactor is expected to move these numbers materially, that belongs in the
   report.
2. Memory and scaling, where I am least confident. **2^25 peaks at 23.5 GB and
   2^26 at 31.3 GB**, both fitting in 36 GiB. Proving is linear in trace length,
   so a mainnet-sized block (~10^8 cycles, 1.66x the top rung) extrapolates
   cleanly to **about 4.3 minutes**. Memory does not: the measured per-doubling
   growth is 1.73x early in the ladder and 1.33x at the top, which carried
   forward give 38.6 GB and 46.6 GB. I report the range rather than a point, but
   the real question is **whether that flattening at the top is a property of the
   prover or an artifact of my last measurement.** You would know; I would only be
   guessing. (An earlier draft of this note claimed 2^26 would not fit at all. It
   does — the difference is that I ran it instead of extrapolating it, which is
   why I am reluctant to extrapolate the next one.)
3. `ecrecover`. Jolt ships `ecdsa_verify` and no `recover`, so I wrote one on the
   public inline API (`crates/jolt-zkvm-accel/src/secp256k1.rs`), recovering the
   y-coordinate with a fixed-exponent square root since `p = 3 (mod 4)`. It is
   differential-tested against `k256` on 64 signatures plus edge cases. If you
   want it upstream I will send a PR; if the approach is wrong for reasons the
   tests do not catch, please say so before anyone builds on it.

**One request.** `div_assume_nonzero` is private on the RISC-V path for
`Secp256k1Fr`, so callers outside the crate pay for `div`'s zero check even when
the operand is known non-zero. Making it public is a one-word diff.

**And one thing I would value your reaction to more than any correction.** The
report's organising claim is that on Jolt the unit of optimisation is *rungs of
the power-of-two trace bound*, not percent of cycles — so that removing 62% of
ecrecover's cycles is worth 2.3x and removing 16% of ecpairing's is worth
literally nothing. I believe that follows from how the prover is sized rather
than from anything subtle, but it reframes which optimisations are worth building,
and if you think it is overstated I would rather hear that before the blog post
than after.

---

## To Succinct / SP1

You appear here as the reference point, and the comparison is deliberately
conservative toward SP1: same guest crate, same fixtures, and no proving-time
comparison at all, because SP1's CPU prover on an Apple laptop is not a
configuration anyone runs and a number from it would flatter Jolt.

**What the report says about SP1.** On six of seven workloads Jolt's trace is
2.5-2.9x longer than SP1's instruction count. On the bn254 pairing block it is
5.5x, and the report attributes that specifically to SP1 having a bn254
precompile where Jolt has nothing — not to anything about the provers otherwise.

**The measurement caveat I want checked.** I state that SP1's instruction count
charges a precompile as a syscall rather than as the instructions the operation
would otherwise take, with the real work in separate AIRs the count does not
show, so a Jolt "cycle" and an SP1 "cycle" are not the same unit. If that is
imprecise, the correction matters more than the numbers — the whole §4 rests on
it, and I would rather state the difference in your words than in mine.

**Two bugs, offered rather than filed.**

1. `ere-prover-sp1` v0.15.0 cannot execute on aarch64: it pools
   `MinimalExecutorEnum`s and calls `reset()`, which SP1 v6.3.1 leaves as
   `todo!()` in the portable executor — the one every non-x86_64 host gets. I
   worked around it by using the SDK executor directly.
2. The released Succinct toolchain reports rustc 1.93, and `ere-guests`' reth pin
   declares 1.95, so `cargo prove build` needs `--ignore-rust-version`. It builds
   and runs correctly with it.

Happy to file either properly if useful.

---

## To eth-act

Your harness is the neutrality argument in this work, and both the guest and the
fixtures are yours unmodified.

**What I would like reviewed** is the backend contract — specifically the three
places Jolt does not fit the existing mould (no ambient guest IO, a compiler that
shells out to the `jolt` CLI, and a verifying key that carries verifier
preprocessing rather than a digest). I would rather agree the shape before a PR
than after.

**One thing I got wrong about your harness**, recorded because I nearly filed it
as an issue against you. Benchmarking one fixture at a time, my own driver took
the first test case in a file and the first block within it. That put me on
`ecpairing/valid.json`'s `empty-ecpairing` case, which performs no pairing at
all, and I reported 2.00x for "the bn254 workload" on a block containing no
bn254 work. The real number is 1.19x, and it turned out to be the most
interesting result in the study.

Reading `load_eest_benchmark_fixtures` afterwards showed the fault was entirely
mine. You iterate every case in a file, and deliberately take the *last* block
per case, with the comment that earlier blocks are setup. My driver now follows
the same rule. I mention it only so the trap is on record for anyone else
writing a single-fixture driver against your corpus, and because a note in the
docs might save them the day it cost me.
