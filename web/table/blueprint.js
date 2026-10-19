// Heads-up play against the trained holdem-cfr blueprint.
//
// The blueprint was exported by `holdem-cfr export-web`: the abstract betting
// tree, the average strategy (quantized to bytes), and the percentile
// thresholds of the card abstraction. Buckets are recomputed here from the
// cards, with the same definitions, summation order and f32 roundings as
// crates/hunl/src/strength.rs:
//   river  HS     = (wins + ties/2) / 990 against every opponent hand, quantized to u16
//   turn   E[HS²] = mean of HS² over the 46 rivers
//   flop   E[HS²] = mean of turn E[HS²] over the 47 turns
// Cards are ints 0..51 (rank = c >> 2, suit = c & 3), the same encoding as Rust.

const f32 = Math.fround;

// ---------- a fast 5-7 card evaluator (same values as core.js `evaluate`) ----------
const HI = new Int8Array(8192);   // highest set bit
const POP = new Uint8Array(8192); // popcount
const STRAIGHT = new Int8Array(8192); // rank of the top card of the best straight, or -1
for (let m = 1; m < 8192; m++) {
  HI[m] = 31 - Math.clz32(m);
  POP[m] = POP[m >> 1] + (m & 1);
}
for (let m = 0; m < 8192; m++) {
  const x = (m << 1) | ((m >> 12) & 1);
  STRAIGHT[m] = -1;
  for (let hi = 13; hi >= 4; hi--) {
    if (((x >> (hi - 4)) & 31) === 31) { STRAIGHT[m] = hi - 1; break; }
  }
}
HI[0] = -1;

// Top `n` ranks of `mask`, packed 4 bits each starting at `shift` (like core.js `pack`).
function topRanks(mask, n, shift) {
  let v = 0;
  for (let i = 0; i < n && mask; i++) {
    const h = HI[mask];
    v |= h << (shift - 4 * i);
    mask ^= 1 << h;
  }
  return v;
}

// Board state: rank masks by multiplicity (m1: at least one, m2: at least two...)
// and one rank mask per suit. Cards are folded in with `addCard`.
function emptyState() { return { m1: 0, m2: 0, m3: 0, m4: 0, s: [0, 0, 0, 0] }; }
function addCard(st, c) {
  const b = 1 << (c >> 2);
  if (st.m3 & b) st.m4 |= b; else if (st.m2 & b) st.m3 |= b; else if (st.m1 & b) st.m2 |= b; else st.m1 |= b;
  st.s[c & 3] |= b;
}
function stateOf(cards) { const st = emptyState(); for (const c of cards) addCard(st, c); return st; }

// Value of `st` plus cards a and b, without allocating.
function rankWith(st, a, b) {
  let m1 = st.m1, m2 = st.m2, m3 = st.m3, m4 = st.m4;
  let s0 = st.s[0], s1 = st.s[1], s2 = st.s[2], s3 = st.s[3];
  for (let k = 0; k < 2; k++) {
    const c = k ? b : a;
    const bit = 1 << (c >> 2);
    if (m3 & bit) m4 |= bit; else if (m2 & bit) m3 |= bit; else if (m1 & bit) m2 |= bit; else m1 |= bit;
    switch (c & 3) { case 0: s0 |= bit; break; case 1: s1 |= bit; break; case 2: s2 |= bit; break; default: s3 |= bit; }
  }
  let fl = -1;
  if (POP[s0] >= 5) fl = s0; else if (POP[s1] >= 5) fl = s1; else if (POP[s2] >= 5) fl = s2; else if (POP[s3] >= 5) fl = s3;
  if (fl >= 0) {
    const sf = STRAIGHT[fl];
    if (sf >= 0) return (8 << 20) | (sf << 16);
    return (5 << 20) | topRanks(fl, 5, 16);
  }
  if (m4) {
    const q = HI[m4];
    return (7 << 20) | (q << 16) | (HI[m1 & ~(1 << q)] << 12);
  }
  const trips = m3 & ~m4, pairs = m2 & ~m3;
  if (trips) {
    const t0 = HI[trips];
    const rest = trips ^ (1 << t0);
    if (rest || pairs) {
      const p = rest ? Math.max(HI[rest], pairs ? HI[pairs] : -1) : HI[pairs];
      return (6 << 20) | (t0 << 16) | (p << 12);
    }
  }
  const st8 = STRAIGHT[m1];
  if (st8 >= 0) return (4 << 20) | (st8 << 16);
  if (trips) {
    const t0 = HI[trips];
    return (3 << 20) | (t0 << 16) | topRanks(m1 & ~(1 << t0), 2, 12);
  }
  if (POP[pairs] >= 2) {
    const p0 = HI[pairs], p1 = HI[pairs ^ (1 << p0)];
    return (2 << 20) | (p0 << 16) | (p1 << 12) | (HI[m1 & ~(1 << p0) & ~(1 << p1)] << 8);
  }
  if (pairs) {
    const p0 = HI[pairs];
    return (1 << 20) | (p0 << 16) | topRanks(m1 & ~(1 << p0), 3, 12);
  }
  return topRanks(m1, 5, 16);
}

// ---------- hand strength, exactly as the Rust tables ----------
// Unquantized river HS: (wins + ties / 2) / 990 against all opponent hands.
function riverHS(hole, board) {
  const st = stateOf(board);
  const me = rankWith(st, hole[0], hole[1]);
  const live = [];
  for (let c = 0; c < 52; c++) if (c !== hole[0] && c !== hole[1] && !board.includes(c)) live.push(c);
  let wins = 0, ties = 0;
  for (let i = 0; i < live.length; i++) {
    for (let j = i + 1; j < live.length; j++) {
      const v = rankWith(st, live[i], live[j]);
      if (v < me) wins++; else if (v === me) ties++;
    }
  }
  return f32((wins + 0.5 * ties) / 990);
}
const quantize = x => Math.round(f32(Math.min(1, Math.max(0, x)) * 65535));
const dequantize = q => f32(q / 65535);
// River metric: HS after the u16 round trip of the strength table.
const riverMetric = (hole, board) => dequantize(quantize(riverHS(hole, board)));

function turnEHS2(hole, board, river = riverMetric) {
  let s2 = 0, n = 0;
  for (let r = 0; r < 52; r++) {
    if (board.includes(r) || hole.includes(r)) continue;
    const hs = river(hole, [...board, r]);
    s2 += hs * hs;
    n++;
  }
  return f32(s2 / n);
}

function flopEHS2(hole, board) {
  // Each 5-card board appears twice (turn t then river r, and r then t): cache by pair.
  const cache = new Float64Array(52 * 52).fill(-1);
  const river = (h, b5) => {
    const t = b5[3], r = b5[4];
    const k = t < r ? t * 52 + r : r * 52 + t;
    if (cache[k] < 0) cache[k] = riverMetric(h, b5);
    return cache[k];
  };
  let s2 = 0, n = 0;
  for (let t = 0; t < 52; t++) {
    if (board.includes(t) || hole.includes(t)) continue;
    s2 += turnEHS2(hole, [...board, t], river);
    n++;
  }
  return f32(s2 / n);
}

// The bucketing metric on a board of 3, 4 or 5 cards.
function metric(hole, board) {
  if (board.length === 3) return flopEHS2(hole, board);
  if (board.length === 4) return turnEHS2(hole, board);
  return riverMetric(hole, board);
}

function bucketOf(thresholds, x) {
  let k = 0;
  while (k < thresholds.length && thresholds[k] <= x) k++;
  return k;
}

// ---------- the exported blueprint ----------
function decodeBase64(s) {
  if (typeof atob === "function") {
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  return new Uint8Array(Buffer.from(s, "base64"));
}

const RANK_CHARS = "23456789TJQKA";
function classOf(a, b) {
  let hi = a, lo = b;
  if ((b >> 2) > (a >> 2)) { hi = b; lo = a; }
  const h = RANK_CHARS[hi >> 2], l = RANK_CHARS[lo >> 2];
  if ((hi >> 2) === (lo >> 2)) return h + l;
  return h + l + ((hi & 3) === (lo & 3) ? "s" : "o");
}

class Blueprint {
  constructor(data) {
    this.data = data;
    this.params = data.params;
    this.nodes = data.nodes;
    this.strategy = decodeBase64(data.strategy);
    this.thresholds = data.thresholds.map(t => t.map(f32));
    this.preflop = new Map(data.preflop.map((name, i) => [name, i]));
    this.iterations = data.iterations;
  }
  isTerminal(node) { return this.nodes[node].length === 3; }
  // Action node accessors.
  player(node) { return this.nodes[node][0]; }
  street(node) { return this.nodes[node][1]; }
  children(node) { return this.nodes[node][2]; }
  actions(node) { return this.nodes[node][3]; }
  // Terminal accessors.
  contributed(node) { const n = this.nodes[node]; return [n[0], n[1]]; }
  folded(node) { return this.nodes[node][2]; }

  bucket(hole, board) {
    if (board.length === 0) return this.preflop.get(classOf(hole[0], hole[1]));
    const street = board.length - 2;
    return bucketOf(this.thresholds[street - 1], metric(hole, board));
  }

  // Average strategy at (node, bucket) as probabilities.
  probs(node, bucket) {
    const k = this.actions(node).length;
    const base = this.nodes[node][4] + bucket * k;
    let total = 0;
    const p = [];
    for (let a = 0; a < k; a++) { p.push(this.strategy[base + a]); total += this.strategy[base + a]; }
    return p.map(x => (total ? x / total : 1 / k));
  }

  // How to show action `a` at `node` given the chips each seat has in.
  label(node, a, contributed) {
    const x = this.actions(node)[a];
    const p = this.player(node);
    const toCall = contributed[1 - p] - contributed[p];
    if (x === -1) return { kind: "fold", text: "Fold" };
    if (x === 0) return toCall > 0 ? { kind: "call", text: "Call", amount: toCall } : { kind: "check", text: "Check" };
    if (x >= this.params.stack) return { kind: "allin", text: "All-in", to: x };
    // Preflop the big blind is the first bet, so every raise is a raise.
    const kind = toCall > 0 || this.street(node) === 0 ? "raise" : "bet";
    return { kind, text: kind === "bet" ? "Bet" : "Raise to", to: x };
  }
}

// Contributions at an action node: replay the actions from the root.
function contributionsAlong(bp, path) {
  const c = [bp.params.sb, bp.params.bb];
  let node = 0;
  for (const a of path) {
    const p = bp.player(node);
    const x = bp.actions(node)[a];
    if (x === 0) c[p] = c[1 - p];
    else if (x > 0) c[p] = x;
    node = bp.children(node)[a];
  }
  return { node, contributed: c };
}

// Chips won by seat 0 at a terminal, given both hands and the full board.
function payoff(bp, node, holes, board) {
  const [c0, c1] = bp.contributed(node);
  const f = bp.folded(node);
  if (f === 0) return -c0;
  if (f === 1) return c1;
  const v0 = rankWith(stateOf(board), holes[0][0], holes[0][1]);
  const v1 = rankWith(stateOf(board), holes[1][0], holes[1][1]);
  return v0 > v1 ? c0 : v0 < v1 ? -c0 : 0;
}

if (typeof module !== "undefined") {
  module.exports = { rankWith, stateOf, riverHS, riverMetric, turnEHS2, flopEHS2, metric, bucketOf, quantize, dequantize, Blueprint, contributionsAlong, payoff, classOf };
}
