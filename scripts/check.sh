#!/usr/bin/env bash
# Everything that must hold before a number leaves this repo.
set -euo pipefail
cd "$(dirname "$0")/.."

export PATH="$PWD/.cargo-target-jolt/release:$PATH"

echo "== build =="
cargo build --release --workspace

echo "== unit tests =="
# Includes the ecrecover differential against k256 and the inline-hash
# agreement tests, which gate every published number.
cargo test --release -p jolt-zkvm-accel -p spike-util

echo "== ere backend contract =="
# Compile, execute, prove and verify an EEST block through Ere's unified API.
cargo test --release -p ere-prover-jolt

echo "== repo integrity =="
# Pins are full shas, every result is from the pinned Jolt build and one
# machine, every fixture has both configurations, no run recorded a wrong
# output, figures are neither missing nor orphaned, links resolve, and every
# corpus entry names a case that exists.
python3 scripts/validate-repo.py

echo "== report tables match bench/results =="
python3 scripts/render-report.py --check

echo "== figures match bench/results =="
# Needs matplotlib; see the prerequisites in README.md.
python3 scripts/render-figures.py --check

echo "all green"
