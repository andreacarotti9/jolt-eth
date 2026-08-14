# Pinned upstream commits

Every number in this repo is produced against exactly these revisions. Clone
them into `upstream/` with `scripts/clone-upstream.sh`; the directory is
gitignored on purpose (multi-GB, and the pins below are the real record).

| repo | commit | date | role |
|---|---|---|---|
| [a16z/jolt](https://github.com/a16z/jolt) | `915faf453f36871249615a7fdf2704d77a88f259` | 2026-08-13 | prover under test |
| [eth-act/ere](https://github.com/eth-act/ere) | `552595938b48884d87cfada24f29d39223695c1e` (v0.15.0) | 2026-08-06 | unified zkVM interface; Track A target |
| [eth-act/ere-guests](https://github.com/eth-act/ere-guests) | `817fae8d693f72297c6785fff47acaca5855a6ef` (v0.15.0) | 2026-08-06 | stateless validator guest; Track B source |
| [eth-act/zkvm-standards](https://github.com/eth-act/zkvm-standards) | `282cd356c3a0498416bb0619f9c8a347ce9933fb` | 2026-06-05 | guest IO + accelerator C ABI |
| [eth-act/zkevm-benchmark-workload](https://github.com/eth-act/zkevm-benchmark-workload) | `228a9228b1424c2216c78352763558ce1a7a268c` | 2026-07-30 | fixtures + runner; Track C |
| [succinctlabs/rsp](https://github.com/succinctlabs/rsp) | `44401e52ecd3df81ee603355c0f09f9d64e030e4` | 2026-08-13 | patch-surface reference, prior art |

Jolt pins the `1.95` Rust toolchain (`rust-toolchain.toml`) with the
`riscv32imac-unknown-none-elf` and `riscv64imac-unknown-none-elf` targets.
Jolt is mid-refactor between `jolt-prover` and `jolt-prover-legacy`; every
table in the report states which one produced the number.
