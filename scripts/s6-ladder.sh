#!/usr/bin/env bash
# S6: the trace-length ladder, with peak memory folded into each result file.
#
# The driver cannot see its own peak RSS without a libc dependency, and peak
# memory is the binding constraint here - so run each rung under `/usr/bin/time -l`
# and merge the two numbers it reports into the JSON the driver wrote.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

export PATH="$PWD/.cargo-target-jolt/release:$PATH"
export SPIKE_HOST="${SPIKE_HOST:-$(sysctl -n machdep.cpu.brand_string) / $(sysctl -n hw.ncpu) cores / $(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 )) GiB}"

RUNGS="${*:-20 22 23 24 25}"

for log2t in $RUNGS; do
  echo "=== 2^$log2t ==="
  timing=$(mktemp)
  # A rung that dies of memory pressure is the result, not an error: keep going.
  /usr/bin/time -l ./target/release/s6-trace "$log2t" 2>"$timing" | tail -1
  rss=$(awk '/maximum resident set size/ {print $1}' "$timing")
  peak=$(awk '/peak memory footprint/ {print $1}' "$timing")
  grep -qE "panicked|Killed" "$timing" && echo "  rung failed: $(grep -m1 -E 'panicked|Killed' "$timing")"

  python3 - "$log2t" "${rss:-0}" "${peak:-0}" <<'PY'
import json, pathlib, sys
log2t, rss, peak = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
path = pathlib.Path("bench/results") / f"s6-trace-2p{log2t}.json"
if not path.exists():
    print(f"  no result file for 2^{log2t} (rung did not complete)")
    raise SystemExit(0)
data = json.loads(path.read_text())
data["max_rss_bytes"] = rss
data["peak_memory_footprint_bytes"] = peak or None
path.write_text(json.dumps(data, indent=2) + "\n")
print(f"  rss {rss/1e9:.2f} GB, peak footprint {peak/1e9:.2f} GB")
PY
  rm -f "$timing"
done
