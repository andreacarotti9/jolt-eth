# jolt-eth

Stateless Ethereum block execution inside a [Jolt](https://github.com/a16z/jolt)
guest, benchmarked against SP1 on identical blocks and hardware, through the
Ethereum Foundation's own zkEVM harness.

An independent integration and measurement. Not affiliated with a16z, Succinct
or the Ethereum Foundation, and not a maintained Ethereum client; the numbers are
tied to the pinned commits in [`UPSTREAM.md`](UPSTREAM.md) and go stale as those
projects move.

The findings are in [`report/`](report/), starting with
[`REPORT.md`](report/REPORT.md); every number in them is regenerated from
[`bench/results/`](bench/results/) by `scripts/render-report.py`, and
`scripts/check.sh` fails if a table has drifted from the JSON it came from.

## What is here

| path | what |
|---|---|
| `crates/ere-platform-jolt` | Ere `Platform` for Jolt guests: cycle scopes, host printing, and the `critical-section` impl a zkVM guest needs to link |
| `crates/ere-compiler-jolt` | Ere `Compiler`: drives the `jolt` CLI, which owns Jolt's linker script |
| `crates/ere-verifier-jolt` | Ere `zkVMVerifier`: `JoltProgramVk`, `JoltProof` |
| `crates/ere-prover-jolt` | Ere `zkVMProver`: execute, prove, verify |
| `crates/jolt-zkvm-accel` | the eth-act zkvm-standards accelerator C ABI on Jolt inlines, including an `ecrecover` Jolt does not ship |
| `crates/jolt-eth-host` | driver that runs one EEST block through the guest, both crypto configurations |
| `guests/stateless-validator-jolt` | the EF stateless validator as a Jolt guest |
| `spikes/` | the Phase 0 feasibility spikes, each with its own runnable driver |
| `sp1-baseline/` | the SP1 column, through Ere's own SP1 backend (separate workspace on purpose) |
| `patches/` | the diff that registers the Jolt backend in `eth-act/ere` |
| `scripts/corpus.sh` | the corpus: one workload per line, each naming the EEST case it means |
| `docker/jolt/` | the backend image, mirroring `ere/docker/sp1/` |

## Reproducing

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
python3 scripts/manifest.py                      # what produced each result file

scripts/check.sh                                 # tests + "tables match the JSON"
```

Spikes are separate binaries, one per question:

```bash
cargo run --release -p s1-keccak                   # keccak inline vs software
cargo run --release -p s2-advice -- 67108864       # witness through untrusted advice
cargo run --release -p s6-trace  -- 24             # proving cost at a 2^24 bound
```

## The three claims this repo is built to support

1. **On Jolt, the unit of optimisation is rungs, not percent.** Proving cost is
   sized for the trace rounded up to a power of two, so what an optimisation is
   worth depends entirely on whether it crosses a rung. Accelerating ecrecover
   removes 62% of its cycles and makes the proof 2.3x cheaper; accelerating the
   bn254 pairing block removes 16% and makes it cheaper by nothing at all. Within
   a rung, an 11-24% difference in cycles moves proving time by 3-10% — and in
   one pair, the block with 11% *more* cycles proves 1.1 s *faster*.
2. **The EF's stateless validator runs in Jolt.** Unmodified
   `stateless-validator-reth`, `no_std`, on Jolt's `riscv64imac` target, proving
   real EEST blocks whose output matches the fixture's `statelessOutputBytes` —
   and behind an Ere backend, so it is reachable through the same interface the
   other zkVMs are.
3. **The numbers are honest about where Jolt stands.** About 2x from acceleration
   on a real block, not an order of magnitude. A flat ~2.6x trace-length ratio
   against SP1's instruction count on six of seven workloads — different units,
   not a verdict — and a 5.5x outlier on the one workload where Jolt has no
   substrate at all. Filling that bn254 gap is the single change that would move
   these numbers most.
