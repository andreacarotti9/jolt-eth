#!/usr/bin/env python3
"""Regenerate the report's tables from the raw JSON in bench/results/.

Every number in report/ comes from here, so a stale table is impossible: re-run
this and diff. Prints markdown to stdout.
"""

import json
import pathlib
import statistics
import sys

RESULTS = pathlib.Path(__file__).resolve().parent.parent / "bench" / "results"


def load(pattern):
    for path in sorted(RESULTS.glob(pattern)):
        with path.open() as handle:
            yield path, json.load(handle)


def block_table():
    """Per-fixture trace length, accelerated against the software baseline."""
    rows = {}
    for _, data in load("guest-*-analyze.json"):
        rows.setdefault(short(data), {})[data["config"]] = data
    if not rows:
        return ""

    out = [
        "| fixture | witness B | baseline cycles | accelerated cycles | speedup | output verified |",
        "|---|---:|---:|---:|---:|:--:|",
    ]
    for fixture, configs in sorted(rows.items()):
        base, accel = configs.get("baseline"), configs.get("accel")
        if not (base and accel):
            continue
        speedup = base["trace_len"] / accel["trace_len"]
        ok = "yes" if base["output_matches"] and accel["output_matches"] else "NO"
        out.append(
            f"| {fixture} | {accel['witness_bytes']:,} | {base['trace_len']:,} "
            f"| {accel['trace_len']:,} | {speedup:.2f}x | {ok} |"
        )
    return "\n".join(out)


def region_table():
    """Where the cycles go, averaged over the corpus, per configuration."""
    per_config = {}
    for _, data in load("guest-*-analyze.json"):
        bucket = per_config.setdefault(data["config"], {})
        for region in data["regions"]:
            bucket.setdefault(region["label"], []).append(region)
    if not per_config:
        return ""

    labels = [
        "deserialize_input",
        "new_payload_request_root",
        "validate_chain_config",
        "to_reth_input",
        "run_validation",
        "serialize_output",
    ]
    out = [
        "| region | baseline median cycles | accelerated median cycles | speedup | share of accelerated trace |",
        "|---|---:|---:|---:|---:|",
    ]
    accel_total = sum(
        statistics.median(m["total"] for m in per_config.get("accel", {}).get(label, [{"total": 0}]))
        for label in labels
        if label in per_config.get("accel", {})
    )
    for label in labels:
        base = per_config.get("baseline", {}).get(label)
        accel = per_config.get("accel", {}).get(label)
        if not (base and accel):
            continue
        base_median = statistics.median(m["total"] for m in base)
        accel_median = statistics.median(m["total"] for m in accel)
        speedup = base_median / accel_median if accel_median else float("nan")
        share = 100 * accel_median / accel_total if accel_total else 0
        out.append(
            f"| `{label}` | {base_median:,.0f} | {accel_median:,.0f} | {speedup:.2f}x | {share:.1f}% |"
        )
    return "\n".join(out)


def trace_ladder():
    """S6: proving cost against the declared trace-length bound."""
    rows = sorted(
        (data for _, data in load("s6-trace-*.json")),
        key=lambda d: d["log2_max_trace_length"],
    )
    if not rows:
        return ""
    out = [
        "| bound | real cycles | preprocess s | prove s | verify s | peak GB | verified |",
        "|---|---:|---:|---:|---:|---:|:--:|",
    ]
    for row in rows:
        peak = row.get("peak_memory_footprint_bytes") or row.get("max_rss_bytes")
        peak = f"{peak / 1e9:.1f}" if peak else "-"
        out.append(
            f"| 2^{row['log2_max_trace_length']} | {row['actual_trace_len']:,} "
            f"| {row['preprocess_seconds']:.1f} | {row['prove_seconds']:.1f} "
            f"| {row['verify_seconds']:.2f} | {peak} "
            f"| {'yes' if row['verified'] else 'NO'} |"
        )
    return "\n".join(out)


def keccak_table():
    """S1: cycles per byte, inline against the software implementation."""
    try:
        with (RESULTS / "s1-keccak.json").open() as handle:
            data = json.load(handle)
    except FileNotFoundError:
        return ""
    out = [
        "| input | inline cycles/B | software cycles/B | speedup | inline cycles per keccak-f |",
        "|---:|---:|---:|---:|---:|",
    ]
    for row in data["rows"]:
        perms = row["size_bytes"] / 136
        out.append(
            f"| {row['size_bytes']:,} B | {row['inline_cycles_per_byte']:.2f} "
            f"| {row['soft_cycles_per_byte']:.2f} | {row['speedup']:.2f}x "
            f"| {row['inline']['total'] / perms:,.0f} |"
        )
    return "\n".join(out)


def prove_table():
    """Proving cost for the corpus.

    Prefers `prove-timings.json` (every repetition, from the campaign log) over
    the per-run files, which hold the last repetition only.
    """
    timings = RESULTS / "prove-timings.json"
    if timings.exists():
        with timings.open() as handle:
            runs = json.load(handle)["runs"]
        out = [
            "| fixture | config | cycles | bound | prove s (median of N) | verify s | verified |",
            "|---|---|---:|---:|---:|---:|:--:|",
        ]
        for row in sorted(runs, key=lambda r: (r["fixture"], r["config"])):
            bound = f"2^{row['bound'].bit_length() - 1}" if row.get("bound") else "-"
            out.append(
                f"| {row['fixture'].split('/')[0]} | {row['config']} | {row['cycles']:,} "
                f"| {bound} | {row['prove_seconds_median']:.1f} (N={row['reps']}) "
                f"| {row['verify_seconds_median']:.2f} "
                f"| {'yes' if row['verified'] else 'NO'} |"
            )
        return "\n".join(out)

    rows = [data for _, data in load("guest-*-prove.json")]
    if not rows:
        return ""
    out = [
        "| fixture | config | cycles | prove s | verify s | verified |",
        "|---|---|---:|---:|---:|:--:|",
    ]
    for row in sorted(rows, key=lambda d: (short(d), d["config"])):
        out.append(
            f"| {short(row)} | {row['config']} | {row['trace_len']:,} "
            f"| {row['prove_seconds']:.1f} | {row['verify_seconds']:.2f} "
            f"| {'yes' if row['verified'] else 'NO'} |"
        )
    return "\n".join(out)


def cross_prover_table():
    """Jolt against SP1 on the same fixtures, when the SP1 column exists."""
    sp1 = {short(data): data for _, data in load("sp1-*.json")}
    if not sp1:
        return ""
    jolt = {short(data): data for _, data in load("guest-*-accel-analyze.json")}
    out = [
        "| fixture | Jolt cycles | SP1 cycles | ratio (Jolt/SP1) |",
        "|---|---:|---:|---:|",
    ]
    for name, sp1_row in sorted(sp1.items()):
        jolt_row = jolt.get(name)
        if not jolt_row:
            continue
        ratio = jolt_row["trace_len"] / sp1_row["total_num_cycles"]
        out.append(
            f"| {name} | {jolt_row['trace_len']:,} | {sp1_row['total_num_cycles']:,} | {ratio:.2f}x |"
        )
    return "\n".join(out)


def short(data):
    """A label a reader can match to a workload.

    Prefer the corpus path: EEST test ids do not name the workload (the ecrecover
    case is literally `test_precompiles`), the directory it lives in does.
    """
    if isinstance(data, dict) and data.get("fixture_path"):
        parts = pathlib.PurePath(data["fixture_path"]).parts
        return "/".join(parts[-3:-1]) if len(parts) >= 3 else parts[-1]
    fixture = data["fixture"] if isinstance(data, dict) else data
    path, _, rest = fixture.partition("::")
    case = rest.split("[")[0].removeprefix("test_")
    parent = path.rstrip("/").split("/")[-2] if "/" in path else ""
    return f"{parent}/{case}" if parent and parent not in case else case


def main():
    sections = [
        ("S1 keccak256 inline", keccak_table()),
        ("S6 trace-length ladder", trace_ladder()),
        ("Block corpus: accelerated vs software", block_table()),
        ("Where the cycles go", region_table()),
        ("Proving", prove_table()),
        ("Jolt vs SP1", cross_prover_table()),
    ]
    for title, table in sections:
        if table:
            print(f"\n### {title}\n")
            print(table)
    if not any(table for _, table in sections):
        print("no results in bench/results/", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
