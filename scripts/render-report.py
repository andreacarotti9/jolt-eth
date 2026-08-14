#!/usr/bin/env python3
"""Fill the report's tables in from bench/results/.

`report/REPORT.md` carries `<!-- NAME-TABLE -->` markers; this replaces the block
between each marker and its `<!-- /NAME-TABLE -->` with the freshly generated
table. Idempotent, so it is safe to run after every campaign - which is the
point: a table in the report can never be older than the JSON beside it.

Usage: render-report.py [--check]
"""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import summarize  # noqa: E402

REPORT = pathlib.Path(__file__).resolve().parent.parent / "report" / "REPORT.md"

TABLES = {
    "BLOCK": summarize.block_table,
    "REGION": summarize.region_table,
    "PROVE": summarize.prove_table,
    "CROSS": summarize.cross_prover_table,
    "LADDER": summarize.trace_ladder,
    "KECCAK": summarize.keccak_table,
}


def render(text):
    for name, builder in TABLES.items():
        table = builder()
        if not table:
            continue
        open_marker = f"<!-- {name}-TABLE -->"
        close_marker = f"<!-- /{name}-TABLE -->"
        if open_marker not in text:
            continue
        start = text.index(open_marker)
        end = text.find(close_marker, start)
        end = end + len(close_marker) if end != -1 else start + len(open_marker)
        text = text[:start] + f"{open_marker}\n{table}\n{close_marker}" + text[end:]
    return text


def main():
    original = REPORT.read_text()
    rendered = render(original)
    if "--check" in sys.argv:
        if rendered != original:
            print("report tables are stale; run scripts/render-report.py", file=sys.stderr)
            return 1
        return 0
    REPORT.write_text(rendered)
    print(f"rendered {REPORT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
