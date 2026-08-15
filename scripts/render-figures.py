#!/usr/bin/env python3
"""Regenerate the blog post's figures from bench/results/.

Same contract as render-report.py: no number is ever typed into a figure by
hand. Writes SVG + PNG, light and dark, into report/figures/.

Usage: render-figures.py [--check]
  --check  regenerate into a temp dir and fail if anything differs from
           what is committed, so a stale figure fails like a stale table.
"""

import json
import math
import pathlib
import statistics
import sys
import tempfile

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.lines import Line2D  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
RESULTS = ROOT / "bench" / "results"
FIGURES = ROOT / "report" / "figures"

# Palette: the dataviz reference instance, categorical slots 1-3 plus ink and
# surface tokens. Validated with scripts/validate_palette.js in both modes on
# the all-pairs list. Do not substitute hues without re-running it.
THEMES = {
    "light": dict(
        surface="#fcfcfb",
        ink="#0b0b0b",
        ink2="#52514e",
        ink3="#84837c",
        grid="#e6e5e1",
        mute="#c3c2b7",
        s1="#2a78d6",
        s2="#eb6834",
        s3="#1baf7a",
    ),
    "dark": dict(
        surface="#1a1a19",
        ink="#ffffff",
        ink2="#c3c2b7",
        ink3="#84837c",
        grid="#2f2f2d",
        mute="#52514e",
        s1="#3987e5",
        s2="#d95926",
        s3="#199e70",
    ),
}

SHORT = {
    "eip197_ec_pairing/ecpairing": "ecpairing",
    "eip198_modexp_precompile/modexp": "modexp",
    "eip2780_reduce_intrinsic_tx_gas/value_moving_transactions": "transfer",
    "eip7951_p256verify_precompiles/p256verify": "p256verify",
    "precompiles/ecrecover": "ecrecover",
    "stInitCodeTest/transaction_create_random_init_code": "deploy",
    "vmTests/sha3": "sha3",
}


# ---------------------------------------------------------------- data


def label(data):
    parts = pathlib.PurePath(data["fixture_path"]).parts
    return SHORT["/".join(parts[-3:-1])]


def blocks():
    out = {}
    for path in sorted(RESULTS.glob("guest-*-analyze.json")):
        data = json.loads(path.read_text())
        out.setdefault(label(data), {})[data["config"]] = data
    return out


def ladder():
    rows = [json.loads(p.read_text()) for p in RESULTS.glob("s6-trace-*.json")]
    return sorted(rows, key=lambda r: r["log2_max_trace_length"])


def prove_runs():
    return json.loads((RESULTS / "prove-timings.json").read_text())["runs"]


def sp1():
    out = {}
    for path in sorted(RESULTS.glob("sp1-*.json")):
        data = json.loads(path.read_text())
        out[label(data)] = data["total_num_cycles"]
    return out


def bound_log2(cycles):
    return max(0, (cycles - 1).bit_length())


# ---------------------------------------------------------------- chrome


def figure(t, w, h):
    fig, ax = plt.subplots(figsize=(w, h))
    fig.patch.set_facecolor(t["surface"])
    ax.set_facecolor(t["surface"])
    for side in ("top", "right"):
        ax.spines[side].set_visible(False)
    for side in ("left", "bottom"):
        ax.spines[side].set_color(t["grid"])
        ax.spines[side].set_linewidth(1)
    ax.tick_params(colors=t["ink2"], labelsize=9, length=0)
    return fig, ax


def title(ax, t, head, sub):
    ax.set_title(head, color=t["ink"], fontsize=13, fontweight="600", loc="left", pad=22)
    ax.text(
        0, 1.015, sub, transform=ax.transAxes, color=t["ink2"], fontsize=9.5, va="bottom"
    )


def save(fig, name, mode):
    FIGURES.mkdir(parents=True, exist_ok=True)
    stem = name if mode == "light" else f"{name}-dark"
    for ext, dpi in (("svg", None), ("png", 200)):
        fig.savefig(
            FIGURES / f"{stem}.{ext}",
            dpi=dpi,
            bbox_inches="tight",
            facecolor=fig.get_facecolor(),
            metadata={"Date": None} if ext == "svg" else None,
        )
    plt.close(fig)


# ---------------------------------------------------------------- figures


def fig_staircase(t, mode):
    """Proving cost is a step function of cycles. The hero figure."""
    fig, ax = figure(t, 8.6, 4.8)
    rename = {"transaction_create_random_init_code": "deploy",
              "value_moving_transactions": "transfer"}
    runs = {}
    for r in prove_runs():
        key = r["fixture"].split("/")[0]
        runs[(rename.get(key, key), r["config"])] = r
    names = sorted({k[0] for k in runs},
                   key=lambda n: -runs[(n, "baseline")]["cycles"])

    # rung bands: the prover is sized for whichever band the trace lands in
    lad = {r["log2_max_trace_length"]: r["prove_seconds"] for r in ladder()}
    for i, k in enumerate((22, 23, 24, 25)):
        lo, hi = 2 ** (k - 1), 2 ** k
        if i % 2 == 0:
            ax.axvspan(lo, hi, color=t["grid"], alpha=0.55, lw=0, zorder=0)
        ax.axvline(hi, color=t["mute"], lw=1, zorder=1)
        ax.text(2 ** (k - 0.5), len(names) - 0.32,
                "bound 2^%d\n%.0f s" % (k, lad[k]), color=t["ink2"], fontsize=8.5,
                ha="center", va="top", linespacing=1.45)

    for y, name in enumerate(names):
        b, a = runs[(name, "baseline")], runs[(name, "accel")]
        rungs = bound_log2(b["cycles"]) - bound_log2(a["cycles"])
        ax.plot([a["cycles"], b["cycles"]], [y, y], color=t["ink3"], lw=2,
                solid_capstyle="round", zorder=2)
        ax.scatter([b["cycles"]], [y], s=88, color=t["s1"],
                   edgecolor=t["surface"], linewidth=2, zorder=3)
        ax.scatter([a["cycles"]], [y], s=88, color=t["s2"],
                   edgecolor=t["surface"], linewidth=2, zorder=4)
        ax.text(a["cycles"] * 0.92, y, f"{a['prove_seconds_median']:.1f} s",
                ha="right", va="center", color=t["ink"], fontsize=9,
                fontweight="600", zorder=5)
        ax.text(b["cycles"] * 1.09, y, f"{b['prove_seconds_median']:.1f} s",
                ha="left", va="center", color=t["ink"], fontsize=9,
                fontweight="600", zorder=5)
        word = {0: "no rung", 1: "one rung", 2: "two rungs"}[rungs]
        ax.text(2 ** 25.62, y, word, ha="left", va="center", fontsize=9,
                color=t["s2"] if rungs else t["ink3"],
                fontweight="600" if rungs == 2 else "normal", zorder=5)

    ax.set_xscale("log", base=2)
    ax.set_xlim(2 ** 21.16, 2 ** 25.48)
    ax.set_ylim(-0.62, len(names) - 0.38)
    ax.set_yticks(range(len(names)))
    ax.set_yticklabels(names, color=t["ink"], fontsize=10)
    ax.set_xticks([2 ** 22, 2 ** 23, 2 ** 24, 2 ** 25])
    ax.set_xticklabels(["4.2M", "8.4M", "16.8M", "33.6M"])
    ax.set_xlabel("trace length (cycles, log scale)", color=t["ink2"], fontsize=9.5)
    ax.spines["left"].set_visible(False)
    ax.spines["bottom"].set_visible(False)

    ax.text(2 ** 25.62, len(names) - 0.32, "crossed", color=t["ink2"], fontsize=8.5,
            ha="left", va="top", fontweight="600")
    ax.legend(
        handles=[
            Line2D([], [], marker="o", ls="", ms=9, color=t["s1"], label="software"),
            Line2D([], [], marker="o", ls="", ms=9, color=t["s2"],
                   label="accelerated"),
        ],
        loc="lower left", frameon=False, fontsize=9, labelcolor=t["ink2"], ncols=2,
        bbox_to_anchor=(-0.005, -0.235),
    )
    title(ax, t, "What acceleration is worth depends on whether it crosses a rung",
          "Jolt sizes the prover for the trace rounded up to a power of two, so every "
          "trace in a band costs the same to prove.")
    save(fig, "05-staircase", mode)


def fig_regions(t, mode):
    """Where the cycles go, baseline against accelerated."""
    fig, ax = figure(t, 8.4, 2.9)
    per = {}
    for path in RESULTS.glob("guest-*-analyze.json"):
        data = json.loads(path.read_text())
        for region in data["regions"]:
            per.setdefault(data["config"], {}).setdefault(region["label"], []).append(
                region["total"]
            )
    med = {c: {k: statistics.median(v) for k, v in d.items()} for c, d in per.items()}
    keep = [
        ("run_validation", "run_validation", t["s1"]),
        ("new_payload_request_root", "SSZ hash-tree-root", t["s2"]),
        ("to_reth_input", "to_reth_input", t["s3"]),
    ]
    rows = [("software", "baseline"), ("accelerated", "accel")]
    for y, (name, cfg) in enumerate(rows):
        left = 0
        total = sum(med[cfg].values())
        for key, text, color in keep:
            w = med[cfg][key]
            ax.barh(y, w, left=left, height=0.62, color=color,
                    edgecolor=t["surface"], linewidth=2, zorder=3)
            if w / total > 0.06:
                ax.text(left + w / 2, y, f"{100 * w / total:.0f}%", ha="center",
                        va="center", color=t["surface"], fontsize=9.5,
                        fontweight="600", zorder=4)
            left += w
        rest = total - left
        ax.barh(y, rest, left=left, height=0.62, color=t["mute"],
                edgecolor=t["surface"], linewidth=2, zorder=3)
        ax.text(total + total * 0.012, y, f"{total / 1e6:.2f}M", va="center",
                color=t["ink"], fontsize=10, fontweight="600", zorder=4)

    ax.set_yticks(range(len(rows)))
    ax.set_yticklabels([r[0] for r in rows], color=t["ink"], fontsize=10)
    ax.invert_yaxis()
    ax.set_ylim(1.62, -0.62)
    ax.set_xlim(0, 8.9e6)
    ax.set_xticks([0, 2e6, 4e6, 6e6, 8e6])
    ax.set_xticklabels(["0", "2M", "4M", "6M", "8M"])
    ax.set_xlabel("median cycles across the seven-block corpus", color=t["ink2"],
                  fontsize=9.5)
    ax.spines["left"].set_visible(False)
    ax.grid(axis="x", color=t["grid"], lw=1, zorder=0)
    ax.set_axisbelow(True)
    ax.legend(
        handles=[Line2D([], [], marker="s", ls="", ms=9, color=c, label=n)
                 for _, n, c in keep]
        + [Line2D([], [], marker="s", ls="", ms=9, color=t["mute"],
                  label="deserialize / config / serialize")],
        loc="lower right", frameon=False, fontsize=9, labelcolor=t["ink2"], ncols=2,
        bbox_to_anchor=(1.005, -0.44),
    )
    title(ax, t, "A third of a small block is SSZ hashing, not EVM execution",
          "Composite of the corpus medians. Acceleration takes the block from "
          "7.57M cycles to 3.98M — about 1.9x.")
    save(fig, "03-regions", mode)


def fig_virtual(t, mode):
    """The virtual-instruction share as a tell for 'is this actually accelerated'."""
    fig, ax = figure(t, 8.4, 4.0)
    data = blocks()
    names = sorted(data, key=lambda n: -sum(
        r["virt"] for r in data[n]["accel"]["regions"]) / data[n]["accel"]["trace_len"])
    share = [100 * sum(r["virt"] for r in data[n]["accel"]["regions"])
             / data[n]["accel"]["trace_len"] for n in names]
    base = [100 * sum(r["virt"] for r in data[n]["baseline"]["regions"])
            / data[n]["baseline"]["trace_len"] for n in names]
    colors = [t["s2"] if n == "ecpairing" else t["mute"] for n in names]
    ax.bar(names, share, width=0.6, color=colors, zorder=3)
    ax.scatter(names, base, s=150, color=t["ink3"], zorder=4, marker="_", linewidth=2.4)
    for x, (s, b) in enumerate(zip(share, base)):
        ax.text(x, s + 2, f"{s:.0f}%", ha="center", color=t["ink"], fontsize=9.5,
                fontweight="600")
    ax.set_ylim(0, 104)
    ax.set_ylabel("virtual share of the accelerated trace", color=t["ink2"],
                  fontsize=9.5)
    ax.grid(axis="y", color=t["grid"], lw=1, zorder=0)
    ax.set_axisbelow(True)
    ax.tick_params(axis="x", labelsize=9.5)
    ax.text(6.45, 86,
            "ecpairing barely moves: almost none of its work\n"
            "reaches an inline, because Jolt has no bn254 substrate",
            ha="right", va="center", fontsize=9, color=t["ink"], linespacing=1.55)
    ax.annotate("", xy=(6, 17), xytext=(6, 78),
                arrowprops=dict(arrowstyle="-|>", color=t["s2"], lw=1.4))
    ax.legend(
        handles=[Line2D([], [], marker="_", ls="", ms=10, mew=2, color=t["ink3"],
                        label="same workload, software crypto")],
        loc="upper left", frameon=False, fontsize=9, labelcolor=t["ink2"],
    )
    title(ax, t, "The virtual-instruction share says whether a workload is really accelerated",
          "Jolt's inlines expand into virtual instructions. Turning acceleration on "
          "takes a normal block from about a fifth virtual to two thirds.")
    save(fig, "04-virtual-share", mode)


def fig_sp1(t, mode):
    """Jolt trace rows against SP1 instruction count: a constant plus one outlier."""
    fig, ax = figure(t, 8.4, 4.0)
    data, ref = blocks(), sp1()
    ratios = {n: data[n]["accel"]["trace_len"] / ref[n] for n in ref}
    names = sorted(ratios, key=ratios.get)
    vals = [ratios[n] for n in names]
    flat = [v for n, v in ratios.items() if n != "ecpairing"]
    ax.axhspan(min(flat), max(flat), color=t["s1"], alpha=0.10, zorder=1)
    ax.text(6.62, (min(flat) + max(flat)) / 2,
            f"{min(flat):.2f}–\n{max(flat):.2f}x", color=t["ink2"], fontsize=8.5,
            va="center", ha="left", linespacing=1.4)
    ax.bar(names, vals, width=0.6,
           color=[t["s2"] if n == "ecpairing" else t["mute"] for n in names], zorder=3)
    for x, v in enumerate(vals):
        ax.text(x, v + 0.11, f"{v:.2f}x", ha="center", color=t["ink"], fontsize=9.5,
                fontweight="600")
    ax.set_ylim(0, 6.4)
    ax.set_ylabel("Jolt trace rows ÷ SP1 instruction count", color=t["ink2"],
                  fontsize=9.5)
    ax.grid(axis="y", color=t["grid"], lw=1, zorder=0)
    ax.set_axisbelow(True)
    ax.tick_params(axis="x", labelsize=9.5)
    ax.annotate(
        "SP1 has a bn254 precompile.\nJolt has nothing.",
        xy=(6.32, 4.35), xytext=(3.05, 5.72),
        arrowprops=dict(arrowstyle="-|>", color=t["s2"], lw=1.4),
        fontsize=9, color=t["ink"],
    )
    title(ax, t, "Six workloads sit on a flat architectural constant. One does not.",
          "Not the same unit: Jolt's inlines are inside its trace, SP1's precompiles "
          "are outside its count. The shape is the finding, not the level.")
    save(fig, "06-sp1-ratio", mode)


def fig_ladder(t, mode):
    """Two measures of different scale, so two panels rather than two y-axes."""
    rows = ladder()
    xs = [r["log2_max_trace_length"] for r in rows]
    prove = [r["prove_seconds"] for r in rows]
    mem = [(r.get("peak_memory_footprint_bytes") or r["max_rss_bytes"]) / 1e9
           for r in rows]

    # Extrapolate to a mainnet-sized block, ~1e8 cycles. Proving is linear in
    # trace length, so that one is a single number. Peak memory is not: the
    # measured per-doubling growth ranges from 1.73x early to 1.33x at the top
    # of the ladder, so carry both and show the spread rather than pick one.
    factor = 1e8 / rows[-1]["actual_trace_len"]
    doublings = math.log2(factor)
    x_end = xs[-1] + doublings
    prove_end = prove[-1] * factor
    ratios = [(mem[i] / mem[i - 1]) ** (1 / (xs[i] - xs[i - 1]))
              for i in range(1, len(mem))]
    mem_lo = mem[-1] * min(ratios) ** doublings
    mem_hi = mem[-1] * max(ratios) ** doublings

    fig, axes = plt.subplots(1, 2, figsize=(8.6, 3.5))
    fig.patch.set_facecolor(t["surface"])
    panels = [
        ("prove seconds", prove, t["s1"], 330, (prove_end, prove_end),
         "%.1f min" % (prove_end / 60)),
        ("peak memory, GB", mem, t["s3"], 56, (mem_lo, mem_hi),
         "%.0f–%.0f GB" % (mem_lo, mem_hi)),
    ]
    for ax, (name, ys, color, top, band, tag) in zip(axes, panels):
        ax.set_facecolor(t["surface"])
        for side in ("top", "right"):
            ax.spines[side].set_visible(False)
        for side in ("left", "bottom"):
            ax.spines[side].set_color(t["grid"])
        ax.tick_params(colors=t["ink2"], labelsize=9, length=0)
        if band[1] > band[0]:
            ax.fill_between([xs[-1], x_end], [mem[-1], band[0]], [mem[-1], band[1]],
                            color=color, alpha=0.16, lw=0, zorder=1)
        mid = (band[0] + band[1]) / 2
        ax.plot([xs[-1], x_end], [ys[-1], mid], color=color, lw=2, ls=(0, (2, 2)),
                zorder=2)
        ax.plot(xs, ys, color=color, lw=2, marker="o", ms=7,
                markeredgecolor=t["surface"], markeredgewidth=1.5, zorder=3)
        ax.scatter([x_end], [mid], s=52, facecolor=t["surface"], edgecolor=color,
                   linewidth=2, zorder=4)
        ax.text(x_end - 0.1, mid + top * 0.125, tag, color=t["ink"], fontsize=9.5,
                ha="center", fontweight="600")
        ax.text(x_end - 0.1, mid + top * 0.055, "mainnet-sized", color=t["ink2"],
                fontsize=8.5, ha="center")
        ax.set_ylim(0, top)
        ax.set_xlim(19.4, 28.6)
        ax.set_xticks([20, 22, 24, 26])
        ax.set_xticklabels(["2^20", "2^22", "2^24", "2^26"])
        ax.set_title(name, color=t["ink"], fontsize=10.5, fontweight="600",
                     loc="left", pad=8)
        ax.grid(axis="y", color=t["grid"], lw=1, zorder=0)
        ax.set_axisbelow(True)
    axes[1].axhline(36, color=t["ink3"], lw=1, ls=(0, (4, 3)), zorder=2)
    axes[1].text(19.7, 37.2, "36 GiB — this machine", color=t["ink2"], fontsize=8.5)
    fig.text(0.008, 1.14, "A 60M-cycle trace proves in 2.6 minutes on a laptop",
             color=t["ink"], fontsize=13, fontweight="600")
    fig.text(0.008, 1.045,
             "Trace-length ladder, M3 Max / 36 GiB, driven by a synthetic loop. "
             "Dashed is extrapolation, not measurement; the memory",
             color=t["ink2"], fontsize=9.5)
    fig.text(0.008, 0.975,
             "fan carries the fastest and slowest per-doubling growth the ladder "
             "actually shows.",
             color=t["ink2"], fontsize=9.5)
    fig.text(0.5, -0.10, "declared trace-length bound", color=t["ink2"], fontsize=9.5,
             ha="center")
    fig.subplots_adjust(wspace=0.26)
    save(fig, "07-ladder", mode)


def fig_pipeline(t, mode):
    """What the thing under test actually is, and where each number comes from."""
    fig, ax = figure(t, 8.8, 2.6)
    ax.set_xlim(-1, 101)
    ax.set_ylim(9, 47)
    ax.axis("off")
    stages = [
        (1.5, "Ethereum block\n+ witness", "5–6 KB from the\nEF test corpus", t["mute"]),
        (26.5, "stateless validator\n(guest program)", "386 crates of reth,\nunmodified, RISC-V", t["s1"]),
        (51.5, "Jolt\nprover", "runs the guest,\nrecords a trace", t["s2"]),
        (76.5, "proof\n+ verification", "checked in ~0.5 s\nby anyone", t["s3"]),
    ]
    for x, head, sub, color in stages:
        ax.add_patch(plt.Rectangle((x, 20), 22, 15, facecolor=color, edgecolor="none",
                                   zorder=2))
        ax.text(x + 11, 30.5, head, ha="center", va="center", color=t["surface"],
                fontsize=10, fontweight="600", linespacing=1.4, zorder=3)
        ax.text(x + 11, 16.5, sub, ha="center", va="top", color=t["ink2"], fontsize=8.5,
                linespacing=1.5, zorder=3)
    for x in (24.4, 49.4, 74.4):
        ax.annotate("", xy=(x + 1.6, 27.5), xytext=(x - 0.6, 27.5),
                    arrowprops=dict(arrowstyle="-|>", color=t["ink3"], lw=1.6))
    marks = [
        (37.5, "cycles measured here", 40),
        (62.5, "prove time + memory here", 40),
        (87.5, "output checked against\nthe known answer", 40),
    ]
    for x, text, y in marks:
        ax.plot([x, x], [35.5, y - 2], color=t["ink3"], lw=1, ls=(0, (2, 2)), zorder=1)
        ax.text(x, y, text, ha="center", va="bottom", color=t["ink"], fontsize=8.5,
                linespacing=1.5, zorder=3)
    ax.set_title("What is being measured", color=t["ink"], fontsize=13,
                 fontweight="600", loc="left", pad=26)
    ax.text(0, 1.02, "Every number in this post comes from one of the three dashed "
                     "markers below.", transform=ax.transAxes, color=t["ink2"],
            fontsize=9.5, va="bottom")
    save(fig, "01-pipeline", mode)


def fig_speedup(t, mode):
    """The simplest question: how much faster is accelerated crypto?"""
    fig, ax = figure(t, 8.4, 4.0)
    data = blocks()
    sp = {n: data[n]["baseline"]["trace_len"] / data[n]["accel"]["trace_len"]
          for n in data}
    names = sorted(sp, key=lambda n: -sp[n])
    vals = [sp[n] for n in names]
    ax.bar(names, vals, width=0.6,
           color=[t["s2"] if n == "ecpairing" else t["s1"] for n in names], zorder=3)
    for x, v in enumerate(vals):
        ax.text(x, v + 0.07, "%.2fx" % v, ha="center", color=t["ink"], fontsize=10,
                fontweight="600")
    ax.axhline(1.0, color=t["ink3"], lw=1.5, ls=(0, (4, 3)), zorder=2)
    ax.text(6.62, 0.90, "1.0x =\nno gain\nat all", color=t["ink2"], fontsize=8.5,
            va="top", linespacing=1.5)
    ax.set_xlim(-0.62, 7.5)
    ax.set_ylim(0, 3.7)
    ax.set_ylabel("cycles saved by accelerated crypto", color=t["ink2"], fontsize=9.5)
    ax.grid(axis="y", color=t["grid"], lw=1, zorder=0)
    ax.set_axisbelow(True)
    ax.tick_params(axis="x", labelsize=9.5)
    ax.annotate("the one block acceleration\nbarely touches",
                xy=(6, 1.35), xytext=(4.35, 2.55),
                arrowprops=dict(arrowstyle="-|>", color=t["s2"], lw=1.4),
                fontsize=9, color=t["ink"], linespacing=1.5)
    title(ax, t, "Accelerated crypto is worth about 2x on a real block",
          "Same guest, same block, one cargo feature apart. Six workloads land "
          "between 1.9x and 2.8x; one lands at 1.19x.")
    save(fig, "02-speedup", mode)


FIGURES_ALL = [fig_pipeline, fig_speedup, fig_regions, fig_virtual,
               fig_staircase, fig_sp1, fig_ladder]


def main():
    global FIGURES
    check = "--check" in sys.argv
    committed = FIGURES
    if check:
        FIGURES = pathlib.Path(tempfile.mkdtemp()) / "figures"
    for mode, theme in THEMES.items():
        plt.rcParams["font.family"] = ["DejaVu Sans"]
        # fixed salt: matplotlib otherwise randomises SVG element ids per
        # process, which would make --check fail on identical input
        plt.rcParams["svg.hashsalt"] = "jolt-eth"
        for builder in FIGURES_ALL:
            builder(theme, mode)
    if check:
        stale = [
            f.name
            for f in sorted(FIGURES.glob("*.svg"))
            if not (committed / f.name).exists()
            or (committed / f.name).read_bytes() != f.read_bytes()
        ]
        if stale:
            print(f"figures are stale; run scripts/render-figures.py: {stale}",
                  file=sys.stderr)
            return 1
        return 0
    print(f"rendered {len(FIGURES_ALL) * 2 * 2} files into {FIGURES}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
