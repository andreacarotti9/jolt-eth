#!/usr/bin/env python3
"""Collect every repetition of a prove campaign into one JSON.

`jolt-eth-host` writes one result file per (fixture, config), so with REPS>1 the
file holds the last repetition only. Wall-clock needs all of them - that is the
whole reason for repeating - and the campaign log has them, so parse it there
rather than making the driver invent filenames.

Usage: collect-prove-timings.py <campaign.log> [> bench/results/prove-timings.json]
"""

import json
import pathlib
import re
import statistics
import sys

HEADER = re.compile(r"^=== (?P<fixture>.+?) \[(?P<config>\w+)\] rep (?P<rep>\d+) ===")
TRACED = re.compile(r"^traced (?P<cycles>\d+) cycles, proving with bound (?P<bound>\d+)")
TIMING = re.compile(
    r"^prove (?P<prove>[\d.]+) s, verify (?P<verify>[\d.]+) s, "
    r"verified (?P<verified>\w+), output matches (?P<matches>\w+)"
)


def main(path):
    runs = {}
    current = None
    for line in pathlib.Path(path).read_text().splitlines():
        if match := HEADER.match(line):
            current = (match["fixture"], match["config"])
            runs.setdefault(current, {"prove": [], "verify": [], "cycles": None})
        elif current and (match := TRACED.match(line)):
            runs[current]["cycles"] = int(match["cycles"])
            runs[current]["bound"] = int(match["bound"])
        elif current and (match := TIMING.match(line)):
            runs[current]["prove"].append(float(match["prove"]))
            runs[current]["verify"].append(float(match["verify"]))
            runs[current]["verified"] = match["verified"] == "true"
            runs[current]["output_matches"] = match["matches"] == "true"

    out = []
    for (fixture, config), data in sorted(runs.items()):
        if not data["prove"]:
            continue
        out.append(
            {
                "fixture": fixture,
                "config": config,
                "cycles": data["cycles"],
                "bound": data.get("bound"),
                "reps": len(data["prove"]),
                "prove_seconds_median": statistics.median(data["prove"]),
                "prove_seconds_all": data["prove"],
                "verify_seconds_median": statistics.median(data["verify"]),
                "verified": data.get("verified"),
                "output_matches": data.get("output_matches"),
            }
        )
    json.dump({"runs": out}, sys.stdout, indent=2)
    print()
    return 0 if out else 1


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        raise SystemExit(2)
    raise SystemExit(main(sys.argv[1]))
