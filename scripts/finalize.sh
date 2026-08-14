#!/usr/bin/env bash
# Turn a finished campaign into the report: collect timings, refresh the memory
# ladder, regenerate every table and manifest, then run the checks.
#
# Split out from the campaigns so the expensive measurement and the cheap
# bookkeeping can fail independently.
set -euo pipefail
cd "$(dirname "$0")/.."

LOG="${1:-bench/results/campaign-prove.log}"

if [ -f "$LOG" ]; then
  python3 scripts/collect-prove-timings.py "$LOG" > bench/results/prove-timings.json
  echo "collected prove timings from $LOG"
else
  echo "no campaign log at $LOG; keeping the existing prove-timings.json" >&2
fi

python3 scripts/render-report.py
python3 scripts/manifest.py
scripts/check.sh
