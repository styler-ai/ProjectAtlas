#!/usr/bin/env python3
"""Render README figures from retained evidence; previews stay in ignored tmp/."""

from __future__ import annotations

import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "docs/assets"
PREVIEWS = ROOT / "tmp/readme-figures"
INK = "#101210"
PAPER = "#f1ecdf"
MUTED = "#b1b5ab"
GOLD = "#e0b75b"
BLUE = "#83beca"
LINE = "#363c36"
GRAY = "#60675e"
plt.rcParams.update({
    "font.family": "DejaVu Sans",
    "text.color": PAPER,
    "svg.hashsalt": "projectatlas-readme",
    "svg.fonttype": "path",
})


def canvas(title: str, kicker: str, height: int):
    """Use one readable visual style for the four current README figures."""
    fig = plt.figure(figsize=(11, height / 100), dpi=160, facecolor=INK)
    ax = fig.add_axes((0, 0, 1, 1), xlim=(0, 1000), ylim=(height, 0))
    ax.axis("off")
    ax.add_patch(FancyBboxPatch(
        (1, 1), 998, height - 2, boxstyle="round,pad=0,rounding_size=20",
        facecolor=INK, edgecolor=LINE, linewidth=1,
    ))
    ax.text(46, 44, kicker, fontsize=12, color=MUTED, fontfamily="DejaVu Sans Mono")
    ax.text(46, 104, title, fontsize=29, fontweight="bold")
    return fig, ax


def save(fig, stem: str, description: str):
    """Save a standalone accessible SVG and full/narrow raster review copies."""
    ASSETS.mkdir(parents=True, exist_ok=True)
    PREVIEWS.mkdir(parents=True, exist_ok=True)
    destination = ASSETS / f"{stem}.svg"
    fig.savefig(destination, metadata={"Date": None, "Title": description})
    namespace = "http://www.w3.org/2000/svg"
    ET.register_namespace("", namespace)
    ET.register_namespace("xlink", "http://www.w3.org/1999/xlink")
    tree = ET.parse(destination)
    svg = tree.getroot()
    svg.set("role", "img")
    svg.set("aria-labelledby", "figure-title figure-description")
    title = ET.Element(f"{{{namespace}}}title", {"id": "figure-title"})
    title.text = stem.replace("-", " ")
    desc = ET.Element(f"{{{namespace}}}desc", {"id": "figure-description"})
    desc.text = description
    svg.insert(0, title)
    svg.insert(1, desc)
    tree.write(destination, encoding="utf-8", xml_declaration=True)
    rendered = destination.read_text(encoding="utf-8")
    destination.write_text("\n".join(line.rstrip() for line in rendered.splitlines()) + "\n", encoding="utf-8", newline="")
    fig.savefig(PREVIEWS / f"{stem}.png", dpi=160)
    fig.savefig(PREVIEWS / f"{stem}-narrow.png", dpi=480 / 11)
    plt.close(fig)
    print(destination.relative_to(ROOT))


def navigation():
    """Show progressive orientation without pretending to be runtime output."""
    fig, ax = canvas("Know where to look.", "THE REPOSITORY MAP", 440)
    stages = [
        ("01", "The task", "What needs\nto change?"),
        ("02", "The right area", "Folders and files\nwith a purpose."),
        ("03", "Connected code", "Symbols, callers,\nand dependencies."),
        ("04", "Exact source", "The current lines\nyou need to read."),
    ]
    for index, (number, title, detail) in enumerate(stages):
        x = 46 + index * 231
        ax.add_patch(FancyBboxPatch((x, 158), 210, 180,
            boxstyle="round,pad=0,rounding_size=9", facecolor="#191e19", edgecolor=LINE))
        ax.text(x + 16, 192, number, fontsize=16, color=GOLD, fontfamily="DejaVu Sans Mono")
        ax.text(x + 16, 230, title, fontsize=16, fontweight="bold")
        ax.text(x + 16, 258, detail, fontsize=15, color=MUTED, linespacing=1.6, va="top")
        if index < 3:
            ax.annotate("", xy=(x + 231, 247), xytext=(x + 211, 247),
                arrowprops={"arrowstyle": "->", "color": GOLD, "linewidth": 1.5})
    ax.text(46, 391, "atlas next  →  atlas summary / symbols  →  atlas slice", fontsize=17,
        color=BLUE, fontfamily="DejaVu Sans Mono")
    save(fig, "atlas-navigation", "Start with a task, select the responsible area, follow connected code, and inspect exact current source using the native atlas CLI.")


def parallel_work():
    """Keep host guidance and each worktree's independent map visible."""
    fig, ax = canvas("Each worktree keeps its own map.", "WORKTREES · AGENTS · SUBAGENTS", 505)
    ax.add_patch(FancyBboxPatch((46, 150), 908, 76,
        boxstyle="round,pad=0,rounding_size=9", facecolor="#20251d", edgecolor=LINE))
    ax.text(65, 181, "Session and subagent hooks", fontsize=19, fontweight="bold", color=GOLD)
    ax.text(65, 208, "Version-matched guidance. Exact checkout. Readiness checks.", fontsize=15)
    for index, (label, branch) in enumerate((("Agent A", "feature worktree"), ("Agent B / subagent", "fix worktree"))):
        x = 46 + index * 466
        ax.add_patch(FancyBboxPatch((x, 253), 442, 160,
            boxstyle="round,pad=0,rounding_size=9", facecolor="#191e19", edgecolor=LINE))
        ax.text(x + 22, 292, label, fontsize=21, fontweight="bold")
        ax.text(x + 22, 329, branch, fontsize=18, color=BLUE, fontfamily="DejaVu Sans Mono")
        ax.text(x + 22, 377, "Own source. Own SQLite map.", fontsize=17, color=MUTED)
    ax.text(46, 465, "MCP aliases route calls to a checkout. Git manages the worktrees.", fontsize=16, color=MUTED)
    save(fig, "atlas-parallel-work", "Host agents and subagents receive packaged Codex hook guidance. Each Git worktree retains its own source and SQLite map; per-call MCP aliases select the exact checkout.")


def compact_responses():
    """Plot actual retained full/compact payload bytes on one linear scale."""
    data = json.loads((ROOT / "docs/benchmarks/v0.4-mcp-composition-evaluation.json").read_text(encoding="utf-8"))
    rows = data["formal_results"]
    assert len(rows) == 3 and all(row["all_six_questions_correct"] for row in rows)
    assert all(row["full_calls"] == row["compact_calls"] == 10 for row in rows)
    fig, ax = canvas("Same questions. Smaller answers.", "HISTORICAL GRAPH FIXTURES · v0.4 · OUTPUT BYTES", 660)
    scale = 400 / max(row["full_bytes"] for row in rows)
    names = {"clean": "Clean Git", "dirty": "Saved edits", "non_git": "No Git"}
    for index, row in enumerate(rows):
        y = 189 + index * 125
        reduction = 100 * (1 - row["compact_bytes"] / row["full_bytes"])
        assert abs(reduction - row["emitted_reduction_percent"]) < 0.1
        ax.text(46, y + 22, names[row["fixture"]], fontsize=18, fontweight="bold")
        for offset, field, color in ((0, "full_bytes", GRAY), (38, "compact_bytes", GOLD)):
            value = row[field]
            width = value * scale
            ax.barh(y + offset, width, left=247, height=24, color=color)
            ax.text(247 + width + 12, y + offset + 5, f"{value / 1000:.1f} kB", fontsize=16)
        ax.text(929, y + 20, f"−{reduction:.1f}%", ha="right", fontsize=24, color=GOLD, fontweight="bold")
    ax.plot([46, 70], [574, 574], color=GRAY, linewidth=10, solid_capstyle="butt")
    ax.text(82, 580, "Full response", fontsize=16)
    ax.plot([330, 354], [574, 574], color=GOLD, linewidth=10, solid_capstyle="butt")
    ax.text(366, 580, "Compact response", fontsize=16)
    ax.text(46, 626, "Six questions · Same calls · All answers correct · Not whole-session tokens", fontsize=13, color=MUTED)
    save(fig, "graph-context-comparison", "Historical v0.4 fixture comparison of full and compact graph output. Clean Git: 61,068 versus 25,959 bytes. Saved edits: 71,274 versus 31,418 bytes. Non-Git: 70,135 versus 30,895 bytes. Ten calls per fixture and all six answers correct.")


def historical_audit():
    """Keep the historical modeled denominator explicit, including in-image."""
    report = (ROOT / "docs/benchmarks/large-application-token-savings.md").read_text(encoding="utf-8")
    def value(label: str) -> int:
        match = re.search(rf"^- {re.escape(label)}: ([0-9,]+)", report, re.MULTILINE)
        assert match is not None, label
        return int(match.group(1).replace(",", ""))
    baseline = value("Estimated without ProjectAtlas")
    payload = value("Estimated with ProjectAtlas")
    calls = value("Token telemetry calls")
    assert baseline > payload > 0 and calls == 142
    fig, ax = canvas("One large-application audit.", "HISTORICAL MODELED WORKFLOW · 142 LOOKUPS", 540)
    ax.text(46, 193, f"{baseline / 1e6:.1f}M", fontsize=43, fontweight="bold")
    ax.text(546, 193, f"{payload / 1000:.1f}K", fontsize=43, fontweight="bold", color=GOLD)
    ax.text(46, 230, "Modeled broad-reading baseline", fontsize=16, color=MUTED)
    ax.text(546, 230, "Estimated Atlas response tokens", fontsize=16, color=MUTED)
    ax.barh(294, 908, left=46, height=35, color=GRAY)
    ax.barh(355, 908 * payload / baseline, left=46, height=35, color=GOLD)
    ax.text(46, 270, "Modeled baseline", fontsize=14, color=MUTED)
    ax.text(46, 331, "Atlas output", fontsize=14, color=GOLD)
    ax.text(46, 414, f"Linear scale. Atlas output was {100 * payload / baseline:.2f}% of this modeled baseline.", fontsize=16)
    ax.text(46, 467, "Local bytes/4 estimate. Includes modeled avoided reading.", fontsize=15, color=MUTED)
    ax.text(46, 496, "Historical gross accounting; not a provider bill or a universal result.", fontsize=14, color=MUTED)
    save(fig, "token-savings-bar", f"Historical audit over {calls} lookups: modeled broad-reading baseline {baseline:,} estimated tokens, Atlas payload {payload:,} estimated tokens. Local bytes/4 heuristic and historical gross accounting; not provider billing or universal savings.")


if __name__ == "__main__":
    navigation()
    parallel_work()
    compact_responses()
    historical_audit()
