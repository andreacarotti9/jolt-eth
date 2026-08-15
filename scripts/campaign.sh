#!/usr/bin/env bash
# Phase 3 campaign: every workload in the stratified corpus, both crypto
# configurations, N repetitions. Results land in bench/results/ as JSON; the
# report's tables are regenerated from those files by scripts/render-report.py.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
. scripts/corpus.sh

export PATH="$PWD/.cargo-target-jolt/release:$PATH"
export SPIKE_HOST="${SPIKE_HOST:-$(sysctl -n machdep.cpu.brand_string) / $(sysctl -n hw.ncpu) cores / $(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 )) GiB}"

HOST=./target/release/jolt-eth-host
ACTION="${1:-analyze}"
REPS="${REPS:-1}"

# Every repetition's timing lives here; the per-run JSON only keeps the last one.
# `scripts/collect-prove-timings.py` reads this back.
mkdir -p bench/results
exec > >(tee "bench/results/campaign-$ACTION.log")

while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  fixture="${entry%%::*}"
  case_name="${entry#*::}"
  [ -f "$fixture" ] || { echo "MISSING $fixture"; continue; }
  case_args=()
  [ -n "$case_name" ] && case_args=(--case "$case_name")

  for config in accel baseline; do
    flags=()
    [ "$config" = baseline ] && flags=(--baseline)
    for rep in $(seq 1 "$REPS"); do
      echo "=== $(basename "$(dirname "$fixture")")/${case_name:-default} [$config] rep $rep ==="
      # A workload the guest cannot handle is a finding, not a reason to stop.
      "$HOST" "$ACTION" "$fixture" "${flags[@]+"${flags[@]}"}" "${case_args[@]+"${case_args[@]}"}" 2>&1 \
        | grep -E "^  |^trace |^prove |^traced |^fixture " || echo "  FAILED"
    done
  done
done < <(corpus bench/fixtures/eest/fixtures/blockchain_tests/for_amsterdam)
