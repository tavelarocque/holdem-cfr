"""Build the project page (index.html at the repo root) from the exported blueprint and the Leduc curves."""
import json, csv, math, html
import base64, re, sys
from pathlib import Path
REPO = Path(__file__).resolve().parent.parent
bp = json.load(open(REPO / "web/table/blueprint.json"))
root = bp["nodes"][0]
strat = base64.b64decode(bp["strategy"])
k = len(root[3])
pf = {"probs": {}}
for b, name in enumerate(bp["preflop"]):
    row = list(strat[root[4] + b * k: root[4] + (b + 1) * k])
    pf["probs"][name] = [x / (sum(row) or 1) for x in row]
R = "AKQJT98765432"

# ---------- preflop grid ----------
cells = []
for i, a in enumerate(R):
    for j, b in enumerate(R):
        if i == j: name = a + b
        elif i < j: name = a + b + "s"
        else: name = b + a + "o"
        p = pf["probs"][name]  # fold, call, r2.5, r3.5, allin
        fold, call = p[0], p[1]
        raise_ = 1 - fold - call
        play = 1 - fold
        title = f"{name}: raise {raise_*100:.0f}%, limp {call*100:.0f}%, fold {fold*100:.0f}%"
        cls = "c" + (" pair" if i == j else "") + (" hi" if raise_ > 0.5 else "")
        cells.append(f'<div class="{cls}" style="--r:{raise_*100:.0f}%;--p:{play:.2f}" title="{title}"><span>{name}</span></div>')
grid = "\n".join(cells)

# ---------- Leduc chart (log-log) ----------
series = {}
for row in csv.DictReader(open(REPO / "results/leduc_curves.csv")):
    series.setdefault(row["algo"], []).append((int(row["iteration"]), float(row["exploitability"])))
W, H, L, Rm, T, B = 560, 300, 54, 92, 14, 40
x0, x1 = 0, 4  # log10 iterations 1..10^4
y0, y1 = -6, 1  # log10 exploitability
X = lambda it: L + (math.log10(it) - x0) / (x1 - x0) * (W - L - Rm)
Y = lambda e: T + (y1 - math.log10(e)) / (y1 - y0) * (H - T - B)
names = {"CFR": ("CFR", "s1"), "LCFR": ("Linear CFR", "s2"), "CFR+": ("CFR+", "s3"), "CFRPlus": ("CFR+", "s3"), "DCFR": ("DCFR", "s4")}
svg = [f'<svg viewBox="0 0 {W} {H}" role="img" aria-labelledby="ltitle"><title id="ltitle">Leduc exploitability by iteration for four CFR variants</title>']
for e in range(y0, y1 + 1):
    y = Y(10 ** e)
    svg.append(f'<line class="grid" x1="{L}" x2="{W-Rm}" y1="{y:.1f}" y2="{y:.1f}"/>')
    svg.append(f'<text class="tick" x="{L-8}" y="{y+4:.1f}" text-anchor="end">10<tspan dy="-6" font-size="9">{e}</tspan></text>')
for e in range(x0, x1 + 1):
    x = X(10 ** e)
    svg.append(f'<text class="tick" x="{x:.1f}" y="{H-B+18}" text-anchor="middle">{10**e:,}</text>')
svg.append(f'<text class="axis" x="{(L+W-Rm)/2:.0f}" y="{H-4}" text-anchor="middle">iterations</text>')
svg.append(f'<text class="axis" transform="translate(13 {(T+H-B)/2:.0f}) rotate(-90)" text-anchor="middle">exploitability (chips/hand)</text>')
ends = []
for algo, pts in series.items():
    label, cls = names.get(algo, (algo, "s1"))
    pts = [(it, e) for it, e in pts if e > 0]
    d = " ".join(f"{'M' if k == 0 else 'L'}{X(it):.1f},{Y(max(e, 1e-6)):.1f}" for k, (it, e) in enumerate(pts))
    svg.append(f'<path class="line {cls}" d="{d}"/>')
    it, e = pts[-1]
    ends.append([Y(e), label, cls, e])
ends.sort()
for k in range(1, len(ends)):
    if ends[k][0] - ends[k-1][0] < 14: ends[k][0] = ends[k-1][0] + 14
for y, label, cls, e in ends:
    svg.append(f'<text class="lbl {cls}" x="{W-Rm+8}" y="{y+4:.1f}">{label}</text>')
svg.append("</svg>")
leduc = "\n".join(svg)
finals = {names.get(a, (a,))[0]: p[-1][1] for a, p in series.items()}

# ---------- LBR chart ----------
lbr = [("34M", "6 min, f32", 1143, 138), ("408M", "f32", 1207, 133), ("924M", "f32 · on the table", 1100, 146), ("1,552M", "f64, average reset", 1389, 128)]
W2, H2, L2, R2, T2 = 560, 230, 128, 24, 18
row = 40
xmax = 1600
X2 = lambda v: L2 + v / xmax * (W2 - L2 - R2)
s2 = [f'<svg viewBox="0 0 {W2} {H2}" role="img" aria-labelledby="btitle"><title id="btitle">Local best response winnings against four checkpoints, with 95% intervals</title>']
for v in range(0, xmax + 1, 400):
    x = X2(v)
    s2.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="{T2-6}" y2="{T2+row*4}"/>')
    s2.append(f'<text class="tick" x="{x:.1f}" y="{T2+row*4+16}" text-anchor="middle">{v:,}</text>')
s2.append(f'<text class="axis" x="{(L2+W2-R2)/2:.0f}" y="{H2-6}" text-anchor="middle">LBR winnings, mbb/hand (lower is better)</text>')
for k, (name, note, v, ci) in enumerate(lbr):
    y = T2 + k * row + 8
    best = name == "924M"
    s2.append(f'<text class="rowlbl{" best" if best else ""}" x="{L2-10}" y="{y+9}" text-anchor="end">{name}</text>')
    s2.append(f'<text class="rownote" x="{L2-10}" y="{y+23}" text-anchor="end">{note}</text>')
    s2.append(f'<rect class="bar{" best" if best else ""}" x="{L2}" y="{y}" width="{X2(v)-L2:.1f}" height="16" rx="2"/>')
    s2.append(f'<line class="ci" x1="{X2(v-ci):.1f}" x2="{X2(v+ci):.1f}" y1="{y+8}" y2="{y+8}"/>')
    for e in (v - ci, v + ci):
        s2.append(f'<line class="ci" x1="{X2(e):.1f}" x2="{X2(e):.1f}" y1="{y+3}" y2="{y+13}"/>')
    s2.append(f'<text class="val" x="{X2(v+ci)+6:.1f}" y="{y+12}">+{v:,}</text>')
xf = X2(750)
s2.append(f'<line class="ref" x1="{xf:.1f}" x2="{xf:.1f}" y1="{T2-8}" y2="{T2+row*4}"/>')
s2.append(f'<text class="reflbl" x="{xf+4:.1f}" y="{T2-2}">folding every hand loses 750</text>')
s2.append("</svg>")
lbrsvg = "\n".join(s2)

tpl = open(REPO / "scripts/overview.tpl.html").read()
out = tpl.replace("{{GRID}}", grid).replace("{{LEDUC}}", leduc).replace("{{LBR}}", lbrsvg)
pub = re.sub(r"<!--RESUME-->.*?<!--/RESUME-->\n?", "", out, flags=re.S).replace("{{PLAY}}", "web/table/")
pub = pub.replace("walkthrough PDFs for every milestone are in <span class=\"mono\">docs/</span>", "walkthrough PDFs for every milestone are in <a href=\"https://github.com/tavelarocque/holdem-cfr/tree/main/docs\">docs/</a>")
pub = pub.replace("evidence charts set wide, and the resume bullets as the closing, copyable section.", "evidence charts set wide, and the findings at the close.")
HEAD = '<!doctype html>\n<html lang="en">\n<meta charset="utf-8">\n<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">\n'
(REPO / "index.html").write_text(HEAD + pub)
print("wrote", REPO / "index.html")
