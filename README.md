# jolt-eth

Stateless Ethereum block execution and proving inside a [Jolt](https://github.com/a16z/jolt)
guest, measured against SP1 on identical blocks and hardware, through the
Ethereum Foundation's own zkEVM harness.

> [!CAUTION]
> An independent integration and measurement. Not affiliated with a16z, Succinct
> or the Ethereum Foundation, not audited, and not a maintained Ethereum client.
> Every number is tied to the pinned commits in [`UPSTREAM.md`](UPSTREAM.md) and
> goes stale as those projects move.

## Overview

This project provides:

- A **Jolt backend for [Ere](https://github.com/eth-act/ere)**, the EF's unified
  zkVM interface — the four-crate contract (platform, compiler, verifier,
  prover), a registration patch, and a backend Docker image
- The **EF stateless validator as a Jolt guest**, unmodified, on Jolt's
  `riscv64imac` target, proving real EEST blocks
- The **eth-act zkvm-standards accelerator C ABI on Jolt inlines**, including an
  `ecrecover` that Jolt does not ship
- A **measurement campaign** over a named EEST corpus, in two crypto
  configurations, with an SP1 column through Ere's own SP1 backend
- A **reporting pipeline** in which every published number regenerates from raw
  JSON, and `scripts/check.sh` fails if a table has drifted from the data it
  came from

The guest receives a block plus a witness, executes it statelessly against the
parent state root, and returns the canonical output the EEST fixture carries — so
correctness is asserted against the reference on every run, not just performance.

## Findings

| document | what |
|---|---|
| [`report/REPORT.md`](report/REPORT.md) | the full technical report — every table regenerated from `bench/results/` |
| [`report/POST.md`](report/POST.md) | public writeup of the same results, with figures |
| [`report/right-of-reply.md`](report/right-of-reply.md) | what each upstream team is asked to check, including the claims most likely to be wrong |
| [`report/spec-secp256k1-ecrecover.md`](report/spec-secp256k1-ecrecover.md) | proposed spec for the missing Jolt inline |
| [`bench/METHODOLOGY.md`](bench/METHODOLOGY.md) | pre-registered before the final runs |

Headline: the EF validator runs and proves in Jolt; accelerated crypto is worth
about 2x on a real block; proving cost is a step function of the trace bound, so
an optimisation is worth something only if it crosses a power of two; and bn254
is the one precompile family with no Jolt substrate at all, which is where the
gap to SP1 doubles.

## Workspace Structure

```
jolt-eth/
├── crates/
│   ├── ere-platform-jolt/     # Ere Platform: cycle scopes, host printing, critical-section
│   ├── ere-compiler-jolt/     # Ere Compiler: drives the `jolt` CLI
│   ├── ere-verifier-jolt/     # Ere zkVMVerifier: JoltProgramVk, JoltProof
│   ├── ere-prover-jolt/       # Ere zkVMProver: execute, prove, verify
│   ├── jolt-zkvm-accel/       # zkvm-standards accelerator C ABI on Jolt inlines
│   └── jolt-eth-host/         # campaign driver: one EEST block, both configurations
├── guests/
│   └── stateless-validator-jolt/  # the EF stateless validator as a Jolt guest
├── spikes/                    # Phase 0 feasibility spikes, one runnable driver each
├── sp1-baseline/              # the SP1 column (separate workspace on purpose)
├── bench/                     # methodology, EEST fixtures, raw results
├── scripts/                   # campaign, corpus, rendering, checks
├── patches/                   # diff registering the Jolt backend in eth-act/ere
└── docker/jolt/               # backend image, mirroring ere/docker/sp1/
```

## Binary Crates

### crates/jolt-eth-host

The campaign driver. Runs one EEST block through the guest and reports trace
length, per-region cycle scopes, and output correctness against the fixture.

```bash
jolt-eth-host analyze <fixture.json> [--baseline] [--case SUBSTRING]
jolt-eth-host prove   <fixture.json> [--baseline] [--case SUBSTRING]
```

`--case` matters more than it looks. An EEST file holds many cases and the first
is often degenerate — `ecpairing/valid.json` opens with the *empty* input, which
performs no pairing at all. [`scripts/corpus.sh`](scripts/corpus.sh) therefore
names the case each workload means rather than selecting by position.

### sp1-baseline

The SP1 column, driven through Ere's own SP1 backend over the same corpus, the
same cases and the same block-selection rule. A separate workspace on purpose,
so SP1's toolchain pins cannot influence the Jolt build.

```bash
sp1-baseline <execute|prove> <fixture.json> [--case SUBSTRING]
```

### spikes/

Phase 0 feasibility spikes, one binary per question, each writing its own JSON.
Gate memo in [`spikes/PHASE0-GATE.md`](spikes/PHASE0-GATE.md).

```bash
cargo run --release -p s1-keccak                # keccak inline vs software, cycles/byte
cargo run --release -p s2-advice -- 67108864    # witness through untrusted advice
cargo run --release -p s6-trace  -- 24          # proving cost at a 2^24 bound
```

## Library Crates

### crates/ere-platform-jolt

Guest-side half of the Ere backend: cycle scopes and host printing for
`stateless-validator-*`. Unlike OpenVM, SP1 and ZisK, Jolt has no ambient guest
IO — input and output are regions of a memory layout that `#[jolt::provable]`
computes and bakes into the generated entrypoint.

**Features:**
- `critical-section-impl` *(default)* — a no-op `critical-section` implementation.
  A zkVM guest is single-threaded and uninterruptible, so this is trivially
  correct; without it, any guest reaching `once_cell::sync::OnceCell` (reth's
  `SealedHeader::hash_ref` does, on every header hash) fails to link.

### crates/ere-compiler-jolt

Shells out to `jolt build` rather than reimplementing it. The `jolt` CLI owns the
linker script, memory layout and runtime wiring, and the ELF is only valid if it
was produced that way — the alternative is a second copy of a linker script that
drifts silently. Requires `jolt` on `PATH`, or `JOLT_PATH` pointing at it.

### crates/ere-verifier-jolt

`JoltProgramVk` and `JoltProof`. Verifying a Jolt proof needs the guest's full
`JoltVerifierPreprocessing`, which is program-specific and large — 98 MB for the
stateless validator against 244 KB for a toy guest at the same bound. The crate
publishes a 32-byte digest over the preprocessing and re-derives it from the ELF,
so the interface matches the other backends.

### crates/ere-prover-jolt

`execute`, `prove`, `verify`. Note the guest contract: an Ere Jolt guest is a
program whose provable function takes its input as `jolt::UntrustedAdvice<&[u8]>`
and returns `Vec<u8>`. The crate also deliberately references all eight inline
crates' `host` modules — their registrations live in `inventory` submissions
inside rlib object files, which the linker drops unless something references
them, and a backend cannot know which inlines a guest chose.

### crates/jolt-zkvm-accel

The eth-act zkvm-standards accelerator C ABI implemented for Jolt — the
counterpart of SP1's `libzkevm`. Four families sit on Jolt inlines: keccak256,
sha256, secp256k1 (verify plus an `ecrecover` Jolt does not ship) and secp256r1
verify. Everything without a Jolt substrate — bn254, BLS12-381, KZG, modexp,
ripemd160, blake2f — forwards to `revm::precompile::DefaultCrypto`, so enabling
acceleration can only remove cycles, never add them.

**Features:**
- `host` — runs the inline reference implementations instead of emitting Jolt
  custom instructions, so host-side crates can execute the same code natively.
  Never enable for a guest build: it would silently drop the inlines.

### guests/stateless-validator-jolt

The EF's `stateless-validator-reth`, unmodified, as a Jolt guest. The witness
arrives as **untrusted advice** rather than committed input — stateless
validation already authenticates it against the parent state root, which is
exactly what untrusted advice is for, and the region is memory-mapped so a
`&[u8]` deserializes by borrow instead of copying onto the heap.

**Features:**
- `guest` — set by the `jolt` CLI when compiling for RISC-V
- `accel` — routes every hash, signature and curve operation through the
  zkvm-standards C ABI that `jolt-zkvm-accel` answers on Jolt inlines. The two
  measured configurations differ only by this flag.

## Getting Started

### Prerequisites

- Rust toolchain per Jolt's `rust-toolchain.toml` (1.95, with the
  `riscv32imac-unknown-none-elf` and `riscv64imac-unknown-none-elf` targets)
- The `jolt` CLI on `PATH`, or `JOLT_PATH` set
- Python 3 with `matplotlib`, for the report and figure renderers
- For the SP1 column: the Succinct toolchain (`cargo prove`)

### Reproducing

```bash
scripts/clone-upstream.sh                        # pins in UPSTREAM.md
cargo install --path upstream/jolt --bin jolt    # or set JOLT_PATH
scripts/fetch-fixtures.sh                        # EEST corpus
cargo build --release --workspace

scripts/campaign.sh analyze                      # cycles, both configurations
REPS=3 scripts/campaign.sh prove                 # proofs, both configurations
scripts/campaign-sp1.sh execute                  # the SP1 column
scripts/s6-ladder.sh                             # trace-length / memory ladder

python3 scripts/collect-prove-timings.py /tmp/campaign-prove3.log \
  > bench/results/prove-timings.json
python3 scripts/render-report.py                 # fills report/REPORT.md
python3 scripts/render-figures.py                # fills report/figures/
python3 scripts/manifest.py                      # what produced each result file

scripts/check.sh                                 # tests + "tables match the JSON"
```

`scripts/check.sh` is the gate: it builds, runs the `ecrecover` differential
against `k256` and the inline-hash agreement tests, exercises the Ere backend
end to end on a real EEST block, and fails if any published table or figure has
drifted from `bench/results/`.

## Continuous Integration

| workflow | when | what |
|---|---|---|
| [`ci.yml`](.github/workflows/ci.yml) | every push and PR | the published numbers, checked without a prover — `validate-repo.py`, `render-report.py --check`, `render-figures.py --check`, shellcheck |
| [`full-check.yml`](.github/workflows/full-check.yml) | weekly and on demand | clones the pinned upstreams, installs the `jolt` CLI, fetches fixtures and runs `scripts/check.sh` |

[`scripts/validate-repo.py`](scripts/validate-repo.py) is the integrity check
and needs no build: pins are full shas, **every result file comes from the Jolt
commit `UPSTREAM.md` pins and from one machine**, every fixture has both crypto
configurations, no recorded run produced a wrong output, figures are neither
missing nor orphaned, relative links resolve, and every `corpus.sh` entry names
a case that actually exists in its fixture.

CI never regenerates the measurements. The numbers come from one known machine
(see [`bench/METHODOLOGY.md`](bench/METHODOLOGY.md)); re-running the campaign on
a shared runner would produce different, unusable figures. CI checks that the
code still builds and is still correct — not how fast a cloud VM happens to be.

## Acknowledgements

- The zkVM under test is [Jolt](https://github.com/a16z/jolt), from a16z crypto.
- The unified zkVM interface, the guest and the accelerator ABI are
  [eth-act/ere](https://github.com/eth-act/ere),
  [ere-guests](https://github.com/eth-act/ere-guests) and
  [zkvm-standards](https://github.com/eth-act/zkvm-standards).
- Fixtures and the block-selection rule come from
  [zkevm-benchmark-workload](https://github.com/eth-act/zkevm-benchmark-workload).
- The SP1 column uses [SP1](https://github.com/succinctlabs/sp1), from Succinct.
- [rsp](https://github.com/succinctlabs/rsp) was the patch-surface reference and
  prior art for what a zkVM Ethereum integration has to cover.
