#!/usr/bin/env python3
"""Integrity checks a published measurement has to pass.

`render-report.py --check` proves the tables match the JSON. This proves the
JSON itself is coherent: produced by the pinned prover, on one machine, over a
corpus that resolves, and referenced by documents whose links are not broken.

Standard library only, no build, no fixtures required - so it runs in CI on a
clean checkout in seconds.

Usage: validate-repo.py
Exit code is the number of failures.
"""

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RESULTS = ROOT / "bench" / "results"
FIXTURE_ROOT = ROOT / "bench/fixtures/eest/fixtures/blockchain_tests/for_amsterdam"

FAILURES = []
SKIPS = []


def fail(check, detail):
    FAILURES.append(f"{check}: {detail}")


def report(check, detail=""):
    print(f"  ok    {check}" + (f" ({detail})" if detail else ""))


def skip(check, why):
    SKIPS.append(check)
    print(f"  skip  {check} ({why})")


def results():
    for path in sorted(RESULTS.glob("*.json")):
        try:
            yield path, json.loads(path.read_text())
        except json.JSONDecodeError as exc:
            fail("results parse", f"{path.name}: {exc}")


# ------------------------------------------------------------------ checks


def check_pins():
    """Every commit in UPSTREAM.md is a full 40-hex sha."""
    text = (ROOT / "UPSTREAM.md").read_text()
    pins = re.findall(r"`([0-9a-f]{7,})`", text)
    if not pins:
        return fail("upstream pins", "no commits found in UPSTREAM.md")
    short = [p for p in pins if len(p) != 40]
    if short:
        return fail("upstream pins", f"not full 40-hex: {short}")
    report("upstream pins are full shas", f"{len(pins)} pins")


def jolt_pin():
    text = (ROOT / "UPSTREAM.md").read_text()
    match = re.search(r"a16z/jolt\)[^`]*`([0-9a-f]{40})`", text)
    return match.group(1) if match else None


def check_prover_pin():
    """No result may claim a Jolt commit other than the pinned one.

    This is the check that catches a campaign half-run against an older build,
    which is invisible in the tables and fatal to the conclusions.
    """
    pin = jolt_pin()
    if not pin:
        return fail("prover pin", "could not find the a16z/jolt pin in UPSTREAM.md")
    seen, wrong = 0, []
    for path, data in results():
        if not isinstance(data, dict):
            continue
        commit = (data.get("env") or {}).get("jolt_commit")
        if commit is None:
            continue
        seen += 1
        if commit != pin:
            wrong.append(f"{path.name} -> {commit[:12]}")
    if wrong:
        return fail("prover pin", f"results from a different Jolt build: {wrong}")
    if not seen:
        return fail("prover pin", "no result records env.jolt_commit")
    report("every Jolt result is from the pinned build", f"{seen} files, {pin[:12]}")


def check_one_machine():
    """A comparison across machines is not a comparison."""
    hosts = set()
    for _, data in results():
        if not isinstance(data, dict):
            continue
        host = (data.get("env") or {}).get("host") or data.get("host")
        if host:
            hosts.add(host)
    if len(hosts) > 1:
        return fail("single machine", f"results span {len(hosts)} hosts: {sorted(hosts)}")
    if not hosts:
        return fail("single machine", "no result records a host")
    report("all results from one machine", next(iter(hosts)))


def check_config_pairs():
    """Every analyzed fixture must have both configurations, or the speedup lies."""
    pairs = {}
    for path, data in results():
        if not isinstance(data, dict) or "-analyze" not in path.name:
            continue
        pairs.setdefault(data.get("fixture_path"), set()).add(data.get("config"))
    missing = {k: sorted(v) for k, v in pairs.items() if v != {"baseline", "accel"}}
    if missing:
        return fail("configuration pairs", f"incomplete: {missing}")
    if not pairs:
        return fail("configuration pairs", "no analyze results found")
    report("every fixture has baseline and accel", f"{len(pairs)} fixtures")


def check_outputs_verified():
    """A performance number from a run that produced the wrong answer is noise."""
    bad = [p.name for p, d in results()
           if isinstance(d, dict) and d.get("output_matches") is False]
    if bad:
        return fail("output correctness", f"output_matches false in: {bad}")
    report("every recorded run matched the fixture output")


def check_figures():
    """Referenced figures exist, and no unreferenced figures are committed."""
    figures = ROOT / "report" / "figures"
    if not figures.is_dir():
        return skip("figures", "report/figures/ absent")
    docs = list((ROOT / "report").glob("*.md")) + [ROOT / "README.md"]
    referenced = set()
    for doc in docs:
        if doc.exists():
            referenced |= {m for m in re.findall(r"figures/([\w.-]+\.(?:png|svg))",
                                                 doc.read_text())}
    on_disk = {p.name for p in figures.iterdir() if p.suffix in (".png", ".svg")}
    # a referenced light-mode figure implies its dark and vector siblings
    implied = set()
    for name in referenced:
        stem = name.rsplit(".", 1)[0]
        implied |= {f"{stem}.png", f"{stem}.svg",
                    f"{stem}-dark.png", f"{stem}-dark.svg"}
    missing = sorted(referenced - on_disk)
    orphans = sorted(on_disk - implied)
    if missing:
        fail("figures referenced", f"missing from disk: {missing}")
    if orphans:
        fail("figures orphaned", f"committed but referenced by nothing: {orphans}")
    if not (missing or orphans):
        report("figures match their references", f"{len(on_disk)} files")


def check_links():
    """Relative links in the published documents resolve."""
    broken = []
    for doc in [ROOT / "README.md"] + sorted((ROOT / "report").glob("*.md")):
        if not doc.exists():
            continue
        for target in re.findall(r"\]\((?!https?://|#|mailto:)([^)\s]+)\)",
                                 doc.read_text()):
            if not (doc.parent / target.split("#")[0]).exists():
                broken.append(f"{doc.name} -> {target}")
    if broken:
        return fail("relative links", f"{broken}")
    report("relative links resolve")


def check_corpus():
    """Every corpus entry names a fixture that exists and a case inside it.

    This guards the defect recorded in the report: selecting a workload by
    position rather than by meaning. If a case name silently stops matching,
    the driver measures something else and the numbers stay plausible.
    """
    if not FIXTURE_ROOT.is_dir():
        return skip("corpus resolves", "fixtures not fetched")
    try:
        out = subprocess.run(
            ["bash", "-c", f'. "{ROOT}/scripts/corpus.sh"; corpus "{FIXTURE_ROOT}"'],
            capture_output=True, text=True, check=True).stdout
    except subprocess.CalledProcessError as exc:
        return fail("corpus resolves", f"corpus.sh failed: {exc.stderr.strip()}")
    checked, problems = 0, []
    for line in filter(None, (l.strip() for l in out.splitlines())):
        path, _, case = line.partition("::")
        fixture = pathlib.Path(path)
        if not fixture.is_file():
            problems.append(f"missing fixture {fixture.name}")
            continue
        if case:
            cases = json.loads(fixture.read_text()).keys()
            if not any(case in name for name in cases):
                problems.append(f"{fixture.name}: no case matching {case!r}")
        checked += 1
    if problems:
        return fail("corpus resolves", f"{problems}")
    report("every corpus entry resolves to a real case", f"{checked} workloads")


CHECKS = [
    check_pins,
    check_prover_pin,
    check_one_machine,
    check_config_pairs,
    check_outputs_verified,
    check_figures,
    check_links,
    check_corpus,
]


def main():
    print("== repo integrity ==")
    for check in CHECKS:
        try:
            check()
        except Exception as exc:  # a check that crashes is a failed check
            fail(check.__name__, f"raised {exc!r}")
    print()
    for failure in FAILURES:
        print(f"  FAIL  {failure}", file=sys.stderr)
    if FAILURES:
        print(f"\n{len(FAILURES)} check(s) failed", file=sys.stderr)
    else:
        print("all green" + (f" ({len(SKIPS)} skipped)" if SKIPS else ""))
    return len(FAILURES)


if __name__ == "__main__":
    raise SystemExit(main())
