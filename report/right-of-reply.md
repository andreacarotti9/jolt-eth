# Right of reply

Three audiences, three different things worth checking. Send before publication,
not after. Each note is short on purpose: the report is the artifact, these say
which parts most deserve an expert's scepticism.

Corrections of fact go in. Disagreements of interpretation are recorded with
attribution rather than resolved unilaterally.

---

## To a16z / Jolt

You are the audience most able to tell me I have measured Jolt badly, so here is
where I would look first.

**The claim I most want checked.** A Jolt inline is a virtual instruction
sequence expanded into the main trace, not a precompile with its own AIR.
Measured on a guest, keccak256 costs 24.5 cycles/byte inline against 60.5 in
software - 2.47x - and ~3,259 of the 3,334 cycles per keccak-f permutation are
virtual instructions. If that framing misrepresents how you would characterise
inlines, I would rather fix the framing than defend it.

**Three specific things I may have got wrong.**

1. Every number is `jolt-prover-legacy`, because that is what `#[jolt::provable]`
   wires up at `915faf4`. If `jolt-prover` is reachable a different way, or if
   the refactor is expected to move these numbers materially, that belongs in the
   report.
2. Memory: peak footprint grows about 1.5x per doubling of `max_trace_length`,
   2^25 peaks at 21.8 GB, and 2^26 does not fit in 36 GiB. I extrapolate a
   mainnet-sized block to ~50 GB. That is an extrapolation across two orders of
   magnitude and I would like it challenged.
3. `ecrecover`. Jolt ships `ecdsa_verify` and no `recover`, so I wrote one on the
   public inline API (`crates/jolt-zkvm-accel/src/secp256k1.rs`), recovering the
   y-coordinate with a fixed-exponent square root since `p = 3 (mod 4)`. It is
   differential-tested against `k256` on 64 signatures plus edge cases. If you
   want it upstream I will send a PR; if the approach is wrong for reasons the
   tests do not catch, please say so before anyone builds on it.

**One request.** `div_assume_nonzero` is private on the RISC-V path for
`Secp256k1Fr`, so callers outside the crate pay for `div`'s zero check even when
the operand is known non-zero. Making it public is a one-word diff.

---

## To Succinct / SP1

You appear here as the reference point, and the comparison is deliberately
conservative toward SP1: same guest crate, same fixtures, and no proving-time
comparison at all, because SP1's CPU prover on an Apple laptop is not a
configuration anyone runs and a number from it would flatter Jolt.

**What the report says about SP1.** On six of seven workloads Jolt's trace is
2.5-2.9x longer than SP1's instruction count. On the bn254 pairing block it is
5.5x, and the report attributes that specifically to SP1 having a bn254
precompile where Jolt has nothing - not to anything about the provers otherwise.

**The measurement caveat I want checked.** I state that SP1's instruction count
charges a precompile as a syscall rather than as the instructions the operation
would otherwise take, with the real work in separate AIRs the count does not
show, so a Jolt "cycle" and an SP1 "cycle" are not the same unit. If that is
imprecise, the correction matters more than the numbers.

**Two bugs, offered rather than filed.**

1. `ere-prover-sp1` v0.15.0 cannot execute on aarch64: it pools
   `MinimalExecutorEnum`s and calls `reset()`, which SP1 v6.3.1 leaves as
   `todo!()` in the portable executor - the one every non-x86_64 host gets. I
   worked around it by using the SDK executor directly.
2. The released Succinct toolchain reports rustc 1.93, and `ere-guests`' reth pin
   declares 1.95, so `cargo prove build` needs `--ignore-rust-version`. It builds
   and runs correctly with it.

Happy to file either properly if useful.

---

## To eth-act

Your harness is the neutrality argument in this work, and both the guest and the
fixtures are yours unmodified.

**What I would like reviewed** is the backend contract, in
`report/ere-intent-issue.md` - specifically the three places Jolt does not fit
the existing mould (no ambient guest IO, a compiler that shells out to the `jolt`
CLI, and a verifying key that carries verifier preprocessing rather than a
digest). I would rather agree the shape before a PR than after.

**One finding about the harness itself**, which is not a bug but is a trap. An
EEST file holds many cases and the first is often degenerate:
`ecpairing/valid.json` opens with the empty input, `p256verify/wycheproof_valid.json`
with an invalid public key. Taking "the first block with canonical stateless
bytes" - which is what `zkevm-benchmark-workload`'s selection amounts to for a
single-fixture run - therefore benchmarks a block that does none of the work the
file is named for. My first campaign reported 2.00x for "the bn254 workload" on a
block containing no pairing. Naming the case fixed it, and the real number is
1.19x.

If the benchmark runner grew a way to name a case, or a warning when the selected
block does not touch the precompile its fixture is named for, it would stop the
next person making the same mistake more quietly than I did.
