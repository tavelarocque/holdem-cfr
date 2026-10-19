"""Plot exploitability curves written by `holdem-cfr curves`.

Usage: python3 scripts/plot_curves.py results/leduc_curves.csv results/leduc_exploitability.png "Leduc hold'em"
"""

import csv
import sys
from collections import defaultdict

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

COLORS = {"CFR": "#6b7280", "CFR+": "#2563eb", "LCFR": "#d97706", "DCFR": "#059669"}


def main(src: str, dst: str, title: str) -> None:
    series = defaultdict(lambda: ([], [], []))
    with open(src) as f:
        for row in csv.DictReader(f):
            it, ex, sec = series[row["algo"]]
            it.append(int(row["iteration"]))
            ex.append(float(row["exploitability"]))
            sec.append(float(row["seconds"]))

    fig, axes = plt.subplots(1, 2, figsize=(11, 4.2))
    for algo, (it, ex, sec) in series.items():
        c = COLORS.get(algo)
        axes[0].loglog(it, ex, label=algo, color=c, lw=1.8)
        axes[1].loglog([max(s, 1e-4) for s in sec], ex, label=algo, color=c, lw=1.8)
    axes[0].set_xlabel("iterations")
    axes[1].set_xlabel("solve time (s)")
    for ax in axes:
        ax.set_ylabel("exploitability (chips/hand)")
        ax.grid(True, which="both", alpha=0.25)
        ax.spines[["top", "right"]].set_visible(False)
    axes[0].legend(frameon=False)
    fig.suptitle(f"{title}: exploitability of the average strategy")
    fig.tight_layout()
    fig.savefig(dst, dpi=160)
    print(f"wrote {dst}")


if __name__ == "__main__":
    main(*sys.argv[1:4])
