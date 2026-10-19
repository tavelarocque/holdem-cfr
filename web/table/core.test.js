// node web/table/core.test.js [ranks.txt]
// Checks the JS evaluator against known 7-card category frequencies and, if a
// file of Rust-evaluated hands is given, against the Rust evaluator's ordering.
const assert = require("assert");
const fs = require("fs");
const { evaluate, category, buildPots, awardPots, handClass } = require("./core.js");

const parse = s => s.match(/../g).map(t => "23456789TJQKA".indexOf(t[0]) * 4 + "cdhs".indexOf(t[1]));
const ev = s => evaluate(parse(s.replace(/ /g, "")));

// Edge cases.
assert(ev("Ac2d3h4s5c") < ev("2d3h4s5c6d"), "wheel is the lowest straight");
assert.strictEqual(category(ev("Ac2d3h4s5c")), 4);
assert.strictEqual(category(ev("Ah2h3h4h5h9c9d")), 8, "steel wheel");
assert.strictEqual(category(ev("AcKdQhJs9c")), 0, "no straight");
assert(ev("AcAdKhQsJc") > ev("AcAdKhQsTc"), "kicker");
assert.strictEqual(ev("AsKsQdJcTh2c3d"), ev("AsKsQdJcTh4h5c"), "board plays");
assert.strictEqual(ev("AcAdKhKsQcQd2h"), ev("AcAdKhKsQc3d2h"), "three pairs");
assert.strictEqual(category(ev("9c9d9h5s5c5dAh")), 6, "two trips");
assert.strictEqual(ev("8c8d8h8sKcKdKh"), ev("8c8d8h8sKc2d3h"), "quads kicker");
assert.strictEqual(category(ev("4h5h6c7h8d9hKh")), 5, "flush over straight");
assert.strictEqual(handClass(parse("As")[0], parse("Ks")[0]), "AKs");
assert.strictEqual(handClass(parse("9h")[0], parse("Tc")[0]), "T9o");

// Category frequencies over random 7-card hands.
const known = [0.1741, 0.4382, 0.2350, 0.0483, 0.0462, 0.0303, 0.0260, 0.00168, 0.000311];
const counts = new Array(9).fill(0);
const N = 2_000_000;
const deck = [...Array(52).keys()];
for (let i = 0; i < N; i++) {
  for (let j = 0; j < 7; j++) {
    const k = j + Math.floor(Math.random() * (52 - j));
    [deck[j], deck[k]] = [deck[k], deck[j]];
  }
  counts[category(evaluate(deck.slice(0, 7)))]++;
}
counts.forEach((c, i) => {
  const f = c / N, sd = Math.sqrt(known[i] * (1 - known[i]) / N);
  assert(Math.abs(f - known[i]) < 5 * sd + 1e-4, `category ${i}: ${f} vs ${known[i]}`);
});

// Side pots: A all-in 100, B all-in 300, C calls 300, D folded after putting in 50.
const pots = buildPots([
  { committed: 100, folded: false },
  { committed: 300, folded: false },
  { committed: 300, folded: false },
  { committed: 50, folded: true },
]);
assert.deepStrictEqual(pots, [
  { amount: 350, eligible: [0, 1, 2] },
  { amount: 400, eligible: [1, 2] },
]);
// A has the best hand, B and C tie for the side pot.
const won = awardPots(pots, [9, 5, 5, 0], [0, 1, 2, 3]);
assert.deepStrictEqual(won, { 0: 350, 1: 200, 2: 200 });

// Ordering against the Rust evaluator: each line is "cards rustRank".
if (process.argv[2]) {
  const rows = fs.readFileSync(process.argv[2], "utf8").trim().split("\n").map(l => {
    const [cards, rank] = l.split(" ");
    return [evaluate(parse(cards)), Number(rank)];
  });
  // Same order: sort by JS value and check Rust ranks are non-decreasing with ties matching.
  rows.sort((a, b) => a[0] - b[0]);
  for (let i = 1; i < rows.length; i++) {
    const [jsA, rA] = rows[i - 1], [jsB, rB] = rows[i];
    if (jsA === jsB) assert.strictEqual(rA, rB, "JS tie but Rust differs");
    else assert(rA < rB, `JS orders ${jsA} < ${jsB} but Rust ${rA} >= ${rB}`);
  }
  console.log(`agrees with Rust on ${rows.length} random hands`);
}
console.log("core.js: all checks passed", counts.map(c => (c / N * 100).toFixed(3) + "%").join(" "));
