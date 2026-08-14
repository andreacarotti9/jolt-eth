#!/usr/bin/env bash
# The SP1 column of the campaign, over the same corpus, the same cases and the
# same block-selection rule as scripts/campaign.sh.
set -uo pipefail
cd "$(dirname "$0")/.."
. scripts/corpus.sh

export PATH="$HOME/.sp1/bin:$PATH"
export SPIKE_HOST="${SPIKE_HOST:-$(sysctl -n machdep.cpu.brand_string) / $(sysctl -n hw.ncpu) cores / $(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 )) GiB}"

ACTION="${1:-execute}"

while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  fixture="${entry%%::*}"
  case_name="${entry#*::}"
  [ -f "$fixture" ] || { echo "MISSING $fixture"; continue; }
  case_args=()
  [ -n "$case_name" ] && case_args=(--case "$case_name")

  echo "=== $(basename "$(dirname "$fixture")")/${case_name:-default} ==="
  ( cd sp1-baseline && ./target/release/sp1-baseline "$ACTION" "../$fixture" \
      "${case_args[@]+"${case_args[@]}"}" 2>/dev/null | grep -E "SP1 " ) || echo "  FAILED"
done < <(corpus bench/fixtures/eest/fixtures/blockchain_tests/for_amsterdam)
