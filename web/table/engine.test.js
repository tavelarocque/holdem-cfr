// node web/table/engine.test.js — simulates bot-only hands and checks invariants.
const assert = require("assert");
const fs = require("fs");
const path = require("path");
Object.assign(global, require("./core.js"));
const PREFLOP = JSON.parse(fs.readFileSync(path.join(__dirname, "../../results/preflop_table.json"), "utf8"));
const { Game, decide, buildPercentiles, makeRng, START_STACK } = require("./engine.js");

const PCT = buildPercentiles(PREFLOP);
assert.strictEqual(Object.keys(PCT).length, 169);
assert(PCT["AA"] < 0.01 && PCT["72o"] > 0.9, "percentile order");

const styles = [
  { loose: 1, aggr: 1 }, { loose: 0.85, aggr: 1.2 }, { loose: 1.25, aggr: 0.7 },
  { loose: 1, aggr: 1 }, { loose: 1.2, aggr: 1.35 }, { loose: 0.8, aggr: 0.8 },
];
const rng = makeRng(12345);
const g = new Game(styles.map((s, i) => ({ name: `B${i}`, human: false, style: s })), rng);
const HANDS = 3000;
let showdowns = 0, sidePots = 0, actions = 0;
const vpip = new Array(6).fill(0);
for (let h = 0; h < HANDS; h++) {
  for (const p of g.seats) if (p.stack < 100) p.stack = START_STACK; // rebuy
  const before = g.seats.reduce((s, p) => s + p.stack, 0);
  g.startHand();
  const played = new Set();
  let guard = 0;
  while (g.phase === "betting") {
    const i = g.toAct;
    assert(i !== null, "betting with nobody to act");
    const { action } = decide(g, i, g.seats[i].style, rng, PCT);
    if (g.street === 0 && (action.type === "call" || action.type === "raise")) played.add(i);
    g.act(i, action);
    actions++;
    assert(++guard < 200, "hand did not terminate");
  }
  played.forEach(i => vpip[i]++);
  const after = g.seats.reduce((s, p) => s + p.stack, 0);
  assert.strictEqual(after, before, `chips not conserved in hand ${h}: ${before} -> ${after}`);
  assert(g.seats.every(p => p.stack >= 0));
  assert.strictEqual(g.board.length === 5 || g.result.shown.length === 0, true);
  if (g.result.shown.length) showdowns++;
  if (g.result.pots.length > 1) sidePots++;
}
const vp = vpip.map(v => (100 * v / HANDS).toFixed(0) + "%");
console.log(`${HANDS} hands, ${actions} actions, ${showdowns} showdowns, ${sidePots} with side pots; VPIP by seat ${vp.join(" ")}`);
assert(showdowns > HANDS * 0.1 && sidePots > 0, "expected showdowns and some side pots");
console.log("engine: all checks passed");
