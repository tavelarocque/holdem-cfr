"""Plot postflop convergence logs: python3 scripts/plot_postflop.py out.png title label=log.csv ..."""

import csv
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

COLORS = ["#2563eb", "#6b7280", "#059669", "#d97706"]


def main(dst: str, title: str, *series: str) -> None:
    fig, axes = plt.subplots(1, 2, figsize=(11, 4.2))
    for i, spec in enumerate(series):
        label, path = spec.split("=", 1)
        rows = list(csv.DictReader(open(path)))
        it = [int(r["iteration"]) for r in rows]
        sec = [max(float(r["seconds"]), 1e-4) for r in rows]
        ex = [float(r["exploitability_pct"]) for r in rows]
        c = COLORS[i % len(COLORS)]
        axes[0].loglog(it, ex, label=label, color=c, lw=1.8, marker="o", ms=3)
        axes[1].loglog(sec, ex, label=label, color=c, lw=1.8, marker="o", ms=3)
    axes[0].set_xlabel("iterations")
    axes[1].set_xlabel("solve time (s)")
    for ax in axes:
        ax.set_ylabel("exploitability (% of pot)")
        ax.grid(True, which="both", alpha=0.25)
        ax.spines[["top", "right"]].set_visible(False)
    axes[0].legend(frameon=False)
    fig.suptitle(title)
    fig.tight_layout()
    fig.savefig(dst, dpi=160)
    print(f"wrote {dst}")


if __name__ == "__main__":
    main(*sys.argv[1:])
