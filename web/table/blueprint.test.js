// node web/table/blueprint.test.js
// The browser's card abstraction must agree with the Rust bucket tables, and its
// evaluator with core.js. Fixture deals come from `holdem-cfr export-web`.
const assert = require("assert");
const path = require("path");
const fs = require("fs");
const { evaluate } = require("./core.js");
const bpjs = require("./blueprint.js");
const { rankWith, stateOf, metric, bucketOf, Blueprint, contributionsAlong, payoff } = bpjs;

const parse = s => s.split(" ").map(t => "23456789TJQKA".indexOf(t[0]) * 4 + "cdhs".indexOf(t[1]));

// 1. Fast evaluator equals core.js on random 5-, 6- and 7-card hands.
let seed = 12345;
const rnd = n => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed % n; };
for (let i = 0; i < 200000; i++) {
  const d = [];
  while (d.length < 7) { const c = rnd(52); if (!d.includes(c)) d.push(c); }
  const cs = d.slice(0, 5 + (i % 3));
  assert.strictEqual(rankWith(stateOf(cs.slice(2)), cs[0], cs[1]), evaluate(cs), cs.join(","));
}
console.log("fast evaluator matches core.js on 200,000 hands");

// 2. Buckets match the Rust tables.
const data = JSON.parse(fs.readFileSync(path.join(__dirname, "blueprint.json"), "utf8"));
const bp = new Blueprint(data);
const deals = JSON.parse(fs.readFileSync(path.join(__dirname, "fixtures", "buckets.json"), "utf8"));
const FLOP_DEALS = 60; // the flop metric costs ~50 ms per hand
let checked = 0, valueOff = 0, bucketOff = 0;
deals.forEach((d, i) => {
  const cards = parse(d.cards);
  const hole = cards.slice(0, 2);
  assert.strictEqual(bp.bucket(hole, []), d.buckets[0], `preflop ${d.cards}`);
  for (const [street, len] of [[1, 3], [2, 4], [3, 5]]) {
    if (street === 1 && i >= FLOP_DEALS) continue;
    const board = cards.slice(2, 2 + len);
    const v = metric(hole, board);
    const want = Math.fround(d.values[street - 1]);
    checked++;
    if (v !== want) {
      valueOff++;
      // Iso-class representatives sum in a different suit order: at most 1 f32 ulp.
      assert.ok(Math.abs(v - want) <= 2 ** -23, `${d.cards} street ${street}: ${v} vs ${want}`);
    }
    if (bucketOf(bp.thresholds[street - 1], v) !== d.buckets[street]) bucketOff++;
  }
});
console.log(`metric: ${checked - valueOff}/${checked} bit-identical to Rust (rest within 1 ulp); buckets: ${checked - bucketOff}/${checked} identical`);
assert.ok(bucketOff <= 1, `${bucketOff} bucket mismatches`);
assert.ok(valueOff / checked < 0.05, "too many 1-ulp differences");

// 3. Tree and strategy are well-formed.
let actionNodes = 0;
for (let n = 0; n < bp.nodes.length; n++) {
  if (bp.isTerminal(n)) continue;
  actionNodes++;
  const k = bp.actions(n).length;
  const buckets = data.counts[bp.street(n)];
  for (const b of [0, buckets - 1]) {
    const p = bp.probs(n, b);
    assert.strictEqual(p.length, k);
    assert.ok(Math.abs(p.reduce((a, x) => a + x, 0) - 1) < 1e-9);
  }
}
assert.strictEqual(bp.strategy.length, data.nodes.filter(n => n.length === 5)
  .reduce((a, n) => a + n[3].length * data.counts[n[1]], 0));
console.log(`tree: ${bp.nodes.length} nodes, ${actionNodes} action nodes, ${bp.strategy.length} strategy entries`);

// 4. Replaying random lines gives zero-sum, bounded payoffs and the tree's contributions.
for (let i = 0; i < 20000; i++) {
  let node = 0;
  const p = [];
  while (!bp.isTerminal(node)) {
    const a = rnd(bp.actions(node).length);
    p.push(a);
    node = contributionsAlong(bp, p).node;
  }
  const deck = [];
  while (deck.length < 9) { const c = rnd(52); if (!deck.includes(c)) deck.push(c); }
  const x = payoff(bp, node, [deck.slice(0, 2), deck.slice(2, 4)], deck.slice(4));
  const [c0, c1] = bp.contributed(node);
  const replay = contributionsAlong(bp, p.slice(0, -1));
  assert.deepStrictEqual(contributionsAlong(bp, p).contributed, [c0, c1], "replayed chips match the tree");
  assert.ok(Math.abs(x) <= Math.max(c0, c1) && c0 <= bp.params.stack && c1 <= bp.params.stack);
  if (bp.folded(node) < 0) assert.strictEqual(c0, c1);
  assert.ok(replay.contributed[0] <= c0 && replay.contributed[1] <= c1);
}
console.log("20,000 random lines: replayed chips match the tree, payoffs bounded, showdowns have equal stakes");
console.log("all blueprint tests passed");
