# I ran Ethereum block validation inside Jolt. Here's what I found.

Jolt is a zero-knowledge virtual machine from a16z crypto. It has a striking
design — no hand-written circuit per instruction, just lookups — and a lot of
attention. What it did not have, until now, was any evidence about whether it can
execute and prove Ethereum blocks.

So I built the integration and measured it. This post walks through what came out,
in order, starting from what the problem even is.

---

## What is being measured, and why

Ethereum has a scaling idea that goes like this. Today, every node re-executes
every block to know it's valid. That's a lot of duplicated work. Instead: have
*one* machine execute the block and produce a **cryptographic proof** that it did
so correctly. Everyone else checks the proof, which takes milliseconds.

For this to work you need block validation to be a self-contained computation —
no database, no network. That's **stateless validation**: you hand the validator a
block plus a *witness* containing exactly the pieces of Ethereum's state that this
block touches, along with proofs those pieces are real. The validator checks
everything against the parent block's state root. Self-contained, so it can run
inside a zkVM.

The Ethereum Foundation maintains the reference version of this — a Rust program
called `stateless-validator-reth` — plus a harness so different zkVMs can be
measured on the same program and the same test blocks, instead of each vendor
publishing its own benchmark. SP1, from Succinct, already plugged into it. Jolt
did not.

![Diagram: Ethereum block plus witness feeds a stateless validator guest program, which runs on the Jolt prover, producing a proof; markers show cycles measured at the guest, prove time and memory at the prover, and output correctness at the proof.](figures/01-pipeline.png)

Two terms you need for the rest of this post:

**Cycles.** When the prover runs your program, it records every step in a big
table called a *trace*. One row per step. "Cycles" means rows in that table, and
it's the thing proving cost scales with. Fewer cycles, cheaper proof.

**The guest.** The program being proven, compiled to RISC-V. Here it's the EF's
stateless validator, unmodified.

---

## Getting It to Run

The first question was simply whether the EF's validator could be made to work on
Jolt at all. It's production Rust — `reth`, Ethereum's second-most used client —
compiled to a bare-metal RISC-V target with no operating system, no allocator
guarantees, and no standard library.

It worked, with one fix.

The one fix is a nice illustration of how these things break. Somewhere deep in
`reth`, hashing a block header caches the result in a `OnceCell`. `OnceCell` needs
a lock. Locks on bare metal need a `critical-section` implementation, and nobody
had written one for Jolt. So the link failed on a missing symbol with an
unhelpful name.

The fix is five lines and does nothing: a zkVM guest is single-threaded and can't
be interrupted, so "acquire the lock" is a no-op. But you have to know that. It
now lives in the platform crate so the next person doesn't lose an afternoon.

The result is a 2.1 MB RISC-V binary that proves real Ethereum test blocks and
produces output byte-identical to the answer the test fixture says is correct.

`ecrecover` also had to be implemented. Ethereum's `ecrecover` precompile — the
thing that turns a signature into the address that signed it — needs a *recovery*
operation, and Jolt ships signature *verification* but not recovery.

---

## Inlines and Precompiles

Ethereum blocks are full of cryptography — hashing, signature checks. Doing that
in plain software inside a zkVM is expensive, so zkVMs offer **accelerated
versions**. The EF's harness has a standard way for a guest to call them.

So: run the same block twice, once with software crypto, once with Jolt's
accelerated versions. How much faster?

![Bar chart: acceleration speedup per block. p256verify 2.84x, ecrecover 2.66x, deploy 2.04x, transfer 2.03x, sha3 1.99x, modexp 1.91x, ecpairing 1.19x highlighted.](figures/02-speedup.png)

**About 2x.** Best case 2.84x, worst case 1.19x.

If you were expecting 10x or 50x, that expectation is worth examining, because
it's common and it comes from a real difference between how Jolt and SP1 work.

In SP1, an accelerated operation is a **precompile**: the main program makes one
call, and the actual hashing happens in a *separate* piece of machinery with its
own circuit. The work leaves the main trace.

In Jolt, an accelerated operation is an **inline**: it expands into a sequence of
special instructions that run in the *same* trace as everything else. The work
gets cheaper, but it doesn't leave.

Concretely, I measured Jolt's keccak hash at 24.5 cycles per byte accelerated
against 60.5 in software. Of the ~3,334 cycles each hash permutation costs, about
75 are ordinary RISC-V instructions and about 3,259 are these expanded special
instructions. They're rows in the table just like everything else (this is Jolt's
documented design — [JoltBook](https://jolt.a16zcrypto.com/how/optimizations/inlines.html)
says inlines "remain fully integrated within the main Jolt zkVM execution model").

**Discrepancies:**

- Jolt's benchmark page reports keccak at 26 cycles/byte accelerated against 78 in
  software, for 3.01x.
- The accelerated number here matches theirs closely: 24.5 vs 26.
- The *software baseline* does not: 60.5 vs 78, a 29% gap.
- That gap is the entire difference between 2.47x here and their 3.01x. Could be a
  library version, could be the target, could be the measurement — upstream has
  been asked.
- Separately, Jolt's API reference page for the same function claims "~10-100x
  reduction in VM cycles," more than an order of magnitude off Jolt's own
  benchmark, and probably where the inflated expectation comes from.

---

## A third of a small block isn't the EVM at all

If someone asks you where the time goes in Ethereum block validation, the obvious
answer is "executing transactions." The guest was instrumented to find this out.

![Stacked horizontal bars comparing software and accelerated blocks. run_validation is ~60% of both, SSZ hash-tree-root ~34%, total falls from 7.57M to 3.98M cycles.](figures/03-regions.png)

Transaction execution is about 60%. But **a third of the block is one hashing
step** — computing a Merkle root over the payload, pure SHA-256, before any
transaction runs.

Two things follow. First, if you're optimising a stateless validator guest and you
only looked at the EVM, you missed a third of the work. Second, a caveat on my own
numbers: this share is inflated by the fact that public test blocks are tiny. That
hashing step scales with the block *header*, not with the number of transactions,
so on a real mainnet block with hundreds of transactions it would be a much
smaller slice.

---

## The impact of unaccelerated bn254

Look back at the speedup chart. Six blocks get roughly 2x. One block — `ecpairing`
— gets 1.19x. It is also the most expensive block in the set by a wide margin:
20.4M cycles, against 3.4M for a plain transfer.

The reason is a missing feature. That block spends nearly all its time on **bn254
pairings**, the elliptic-curve cryptography behind most of Ethereum's
zero-knowledge and rollup infrastructure, and Jolt ships no accelerated version of
it. With nothing to call, the pairing runs in plain software exactly as it did
before. The 1.19x that remains is the block's *hashing* getting faster around a
pairing operation that acceleration never touched.

Two separate measurements say the same thing:

- **The speedup is flat.** 1.19x, where every other block gets ~2x.
- **The trace shows almost nothing was accelerated.** Jolt's accelerated
  operations expand into special "virtual" instructions, so the share of the trace
  that is virtual tells you how much of the work reached an accelerated path.
  Turning acceleration on moves a normal block from about a fifth virtual to about
  two thirds. The pairing block goes from 10% to 14% — it barely moves. That ratio
  is a cheap diagnostic for "is this workload actually accelerated," runnable on a
  workload nobody has characterised by hand.

![Bar chart of virtual-instruction share per workload. Six workloads at 61-68%; ecpairing at 14%, highlighted.](figures/04-virtual-share.png)

SP1 ships a bn254 precompile, so this is a missing feature rather than a cost
inherent to proving pairings. If you want one thing to build for Jolt's Ethereum
story, it's bn254. Nothing else I measured comes close.

---

## Proving cost is a staircase

Everything above has been about removing cycles. This section is about when that
is worth doing, and the answer is not "always."

Jolt's prover is not sized for your trace. It rounds the trace length **up to the
next power of two** and builds a prover for that. A 3.3-million-row trace and a
4.1-million-row trace both round up to 4,194,304 rows, so both get the same prover
and both cost the same to run — the extra 800,000 rows are free. Cross into the
next power of two and the cost roughly doubles.

So the cost curve is a staircase: flat between two powers of two, and a jump at
each one. Which means the question "how many cycles did I save?" is the wrong
question. The right one is **"did I land on a lower step?"**

![Dumbbell chart: seven blocks, software vs accelerated trace length on a log axis, with power-of-two bands shaded. ecrecover and p256verify cross two boundaries and drop from ~42s to ~18s; four cross one; ecpairing crosses none and stays at ~80s.](figures/05-staircase.png)

The same optimisation, applied to two different blocks, gives opposite answers:

- **`ecrecover`** — acceleration removed 62% of the cycles, enough to drop two
  steps. Proving went from 42.8 s to 18.3 s: **2.3x cheaper.**
- **`ecpairing`** — acceleration removed 16% of the cycles, not enough to drop
  even one. Proving went from 81.1 s to 79.0 s, which is inside the run-to-run
  noise: **the saving bought nothing at all.**

Here is the same effect from the other direction, and it looks like a broken
measurement until you know about the steps. Two blocks both landed on the 2^24
step. The one with **11% more cycles proved 1.1 seconds faster**. Nothing is wrong
with either number — both traces rounded up to the same size, so the prover did
identical work twice, and what's left is noise on a fixed cost.

That holds across the whole campaign. Grouping all fourteen runs by the step they
landed on, and asking how much the cycle counts varied within each group against
how much the proving times varied:

| step | cycle counts varied by | proving times varied by |
|---|---:|---:|
| 2^22 (6 runs) | 24% | 10% |
| 2^23 (4 runs) | 15% | 14% |
| 2^24 (2 runs) | 11% | 3% |
| 2^25 (2 runs) | 19% | 3% |

Inside a step, cycles barely predict anything. Across steps, they're the only
thing that predicts anything.

The practical version: before optimising a Jolt guest, work out which step you're
on and how far you are from its edge. A 10% saving is worth nothing unless it
crosses. A 50% saving might be worth 2x or might be worth nothing, depending
entirely on where you started.

This also puts a price on the bn254 gap from the previous section. An accelerated
bn254 path that dropped the pairing block two steps would take it from 79 s to the
24–28 s the other blocks cost down there — about 3x — where today's acceleration
moves it nowhere.

---

## Time and Memory

> This was tested on a MacBook — Apple M3 Max, 14 cores, 36 GiB of RAM. Every
> prove and verify number below comes from that one machine.

![Two line charts: prove seconds and peak memory against trace-length bound from 2^20 to 2^26, with dashed extrapolation to a mainnet-sized block at 4.3 minutes and 39-47 GB.](figures/07-ladder.png)

A **60-million-cycle trace proves in 2.6 minutes**, peaking at 31.3 GB of the
36 GB available. An earlier draft of my report stated confidently that this would
not fit. It fits. I found that out by running it instead of extrapolating it,
which turns out to be the moral of the last section as well.

Verification stays at about 0.07 seconds across the whole 64x range. That's the
whole point of the exercise: minutes to prove, milliseconds to check.

Extrapolating to a mainnet-sized block (~100 million cycles) gives roughly **4.3
minutes** of proving. Memory is less certain — growth is 1.73x per doubling early
in the ladder but only 1.33x at the top, and carrying those forward gives 39 GB
and 47 GB respectively. So: **an ordinary server, not a cluster.** I've flagged
that memory range as the thing I'd most like someone with more Jolt experience to
check.

---

## Jolt vs SP1

SP1 is the zkVM already plugged into the EF's harness, so it's the obvious
reference point. I ran the same seven blocks through it — same guest source, same
fixtures, same machine.

The result worth reporting is a correctness one. **Both provers produce output
byte-identical to the fixture's canonical answer, on all seven blocks.** That is
the cross-check in its strongest form: not two implementations agreeing with each
other, but both agreeing with the reference the EEST fixture carries.

I am deliberately not publishing a cycle-count comparison. The two numbers don't
measure the same thing — Jolt's is trace rows, and its inlines expand into that
trace; SP1's is instructions, and its precompiles run in separate machinery the
number never shows. Dividing one by the other produces a figure that looks like a
verdict and isn't. Some of it would be a real architectural difference and some
of it bookkeeping, and cycle counts alone cannot separate the two.

**What a real comparison needs** is numbers in neither system's private units:
prove time, verify time, proof size, peak memory. All four are wall-clock seconds
and bytes, all four are directly comparable between any two provers, and I have
all four for Jolt and none for SP1 — I only ever ran SP1 in execute mode. That's a
gap in the study, not a limitation of the method, and it's the next thing I'll
fix.

When I do, I expect it to be more interesting than a single winner. Jolt should
look good on CPU proving speed. SP1 should look good on verification and proof
size, because it compresses its proof recursively where Jolt at this version
verifies the raw one — and it's a big raw one: Jolt's verifier needs 98 MB of
program-specific setup where SP1's needs 32 bytes. Each winning something for a
legible reason beats a scoreboard.

---

## What's next

In order of how much they'd move the numbers:

1. **A bn254 accelerated path.** Three independent measurements say it's the
   biggest gap, and the staircase says it's worth about 3x on pairing blocks.
2. **The SP1 prover comparison** — prove time, verify time, proof size, memory.
   The missing half of the SP1 section above.
3. **Jolt's refactored prover.** Every number here comes from the older prover,
   because that's what the guest macro reaches at this commit. Expect these to
   move.
4. **A real mainnet block.** Everything here is 5–6 KB witnesses; the scaling
   number is an extrapolation across two orders of magnitude and should be
   treated as a hypothesis.
5. **Joining [ethproofs](https://ethproofs.org) as a proving cluster.**

**Code, data and the full technical report:**
[github.com/andreacarotti9/jolt-eth](https://github.com/andreacarotti9/jolt-eth)

---

*Not affiliated with a16z, Succinct or the Ethereum Foundation. Numbers are tied to
pinned upstream commits and go stale as those projects move; Jolt is `915faf4`
(August 2026), and every proving number comes from `jolt-prover-legacy`.*
