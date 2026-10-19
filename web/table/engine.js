// Six-max no-limit hold'em engine and bot policy. No DOM: runs in node for tests.
// Depends on core.js (evaluate, category, handClass, buildPots, awardPots) and a
// global PREFLOP table {class: [E[HS], E[HS^2]]} exported from holdem-cfr.

const SMALL_BLIND = 50, BIG_BLIND = 100, START_STACK = 10000;
const POSITIONS = ["BTN", "SB", "BB", "UTG", "HJ", "CO"];

// Preflop strength percentile (0 = best) by class, combo-weighted, from E[HS].
function buildPercentiles(table) {
  const combos = n => (n.length === 2 ? 6 : n[2] === "s" ? 4 : 12);
  const names = Object.keys(table).sort((a, b) => table[b][0] - table[a][0]);
  const pct = {};
  let cum = 0;
  for (const n of names) {
    pct[n] = (cum + combos(n) / 2) / 1326;
    cum += combos(n);
  }
  return pct;
}

function makeRng(seed) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0;
    return s / 4294967296;
  };
}

class Game {
  constructor(players, rng) {
    // players: [{name, human, style}]
    this.rng = rng;
    this.seats = players.map(p => ({ ...p, stack: START_STACK, hole: [], bet: 0, committed: 0,
      folded: false, allIn: false, acted: false, raiseClosed: false, lastAction: "", range: 1, aggr: 0 }));
    this.button = Math.floor(rng() * players.length);
    this.handNo = 0;
    this.phase = "idle";
    this.board = [];
    this.history = [];
  }

  n() { return this.seats.length; }
  next(i) { return (i + 1) % this.n(); }
  pot() { return this.seats.reduce((s, p) => s + p.committed, 0); }
  inHand() { return this.seats.map((p, i) => i).filter(i => !this.seats[i].folded); }
  canAct(i) { const p = this.seats[i]; return !p.folded && !p.allIn; }
  position(i) { return POSITIONS[(i - this.button + this.n()) % this.n()]; }
  streetName() { return ["preflop", "flop", "turn", "river"][this.street] || "showdown"; }

  log(text) { this.history.push(text); }

  startHand() {
    this.handNo++;
    this.button = this.next(this.button);
    this.deck = [...Array(52).keys()];
    for (let i = 51; i > 0; i--) {
      const j = Math.floor(this.rng() * (i + 1));
      [this.deck[i], this.deck[j]] = [this.deck[j], this.deck[i]];
    }
    this.board = [];
    this.street = 0;
    this.result = null;
    this.history = [];
    for (const p of this.seats) {
      Object.assign(p, { hole: [this.deck.pop(), this.deck.pop()], bet: 0, committed: 0, folded: false,
        allIn: false, acted: false, raiseClosed: false, lastAction: "", range: 1, aggr: 0, streetRaises: 0 });
    }
    this.raisesThisStreet = 0;
    const sb = this.next(this.button), bb = this.next(sb);
    this.post(sb, SMALL_BLIND); this.post(bb, BIG_BLIND);
    this.seats[sb].lastAction = "SB"; this.seats[bb].lastAction = "BB";
    this.currentBet = BIG_BLIND;
    this.minRaise = BIG_BLIND;
    this.phase = "betting";
    this.log(`Hand #${this.handNo}: ${this.seats[this.button].name} has the button`);
    this.toAct = this.firstToAct(this.next(bb));
    this.settleIfDone();
  }

  post(i, amount) {
    const p = this.seats[i];
    const a = Math.min(amount, p.stack);
    p.stack -= a; p.bet += a; p.committed += a;
    if (p.stack === 0) p.allIn = true;
  }

  needsAction(i) {
    const p = this.seats[i];
    return this.canAct(i) && (!p.acted || p.bet < this.currentBet);
  }

  firstToAct(from) {
    for (let k = 0, i = from; k < this.n(); k++, i = this.next(i)) if (this.needsAction(i)) return i;
    return null;
  }

  // What the player to act may do.
  options(i = this.toAct) {
    const p = this.seats[i];
    const toCall = Math.min(this.currentBet - p.bet, p.stack);
    const maxTo = p.bet + p.stack;
    const canRaise = !p.raiseClosed && maxTo > this.currentBet &&
      this.inHand().some(j => j !== i && this.canAct(j));
    const minTo = Math.min(this.currentBet + this.minRaise, maxTo);
    return { toCall, canCheck: toCall === 0, canRaise, minTo, maxTo, pot: this.pot() };
  }

  act(i, action) {
    if (i !== this.toAct || this.phase !== "betting") throw new Error("not your turn");
    const p = this.seats[i];
    const o = this.options(i);
    const name = p.name;
    if (action.type === "fold") {
      p.folded = true; p.lastAction = "Fold";
      this.log(`${name} folds`);
    } else if (action.type === "check") {
      if (!o.canCheck) throw new Error("cannot check");
      p.acted = true; p.lastAction = "Check";
      this.log(`${name} checks`);
    } else if (action.type === "call") {
      this.post(i, o.toCall);
      p.acted = true; p.lastAction = p.allIn ? "All-in" : "Call";
      p.aggr += 0.4;
      if (this.street === 0) p.range = Math.min(p.range, this.raisesThisStreet >= 2 ? 0.1 : this.raisesThisStreet === 1 ? 0.3 : 0.6);
      this.log(`${name} calls ${o.toCall}`);
    } else if (action.type === "raise") {
      let to = Math.round(action.to);
      if (!o.canRaise) throw new Error("cannot raise");
      to = Math.max(o.minTo, Math.min(to, o.maxTo));
      const increment = to - this.currentBet;
      this.post(i, to - p.bet);
      if (increment >= this.minRaise) {
        this.minRaise = increment;
        for (const q of this.seats) { q.acted = false; q.raiseClosed = false; }
      } else {
        // A short all-in: players who already acted may call but not re-raise.
        for (const q of this.seats) if (q.acted) { q.acted = false; q.raiseClosed = true; }
      }
      this.currentBet = Math.max(this.currentBet, to);
      p.acted = true;
      this.raisesThisStreet++;
      p.aggr += 1;
      if (this.street === 0) {
        const cap = [0.3, 0.3, 0.09, 0.035, 0.02][Math.min(this.raisesThisStreet, 4)];
        p.range = Math.min(p.range, cap);
      }
      p.lastAction = p.allIn ? "All-in" : this.raisesThisStreet === 1 && this.street > 0 ? "Bet" : "Raise";
      this.log(`${name} ${p.allIn ? "goes all-in for" : this.street > 0 && this.raisesThisStreet === 1 ? "bets" : "raises to"} ${p.bet}`);
    }
    this.toAct = this.firstToAct(this.next(i));
    this.settleIfDone();
  }

  // Close the street, deal, or finish the hand when nobody else needs to act.
  settleIfDone() {
    if (this.inHand().length === 1) return this.finish();
    if (this.toAct !== null) return;
    const canStillAct = this.inHand().filter(i => this.canAct(i));
    if (this.street === 3 || canStillAct.length <= 1) {
      while (this.board.length < 5) this.board.push(this.deck.pop());
      return this.finish();
    }
    this.street++;
    const deal = this.street === 1 ? 3 : 1;
    for (let k = 0; k < deal; k++) this.board.push(this.deck.pop());
    for (const p of this.seats) { p.bet = 0; p.acted = false; p.raiseClosed = false; if (!p.folded) p.lastAction = p.allIn ? "All-in" : ""; }
    this.currentBet = 0;
    this.minRaise = BIG_BLIND;
    this.raisesThisStreet = 0;
    this.log(`${this.streetName()[0].toUpperCase() + this.streetName().slice(1)}: ${this.board.map(cardName).join(" ")}`);
    this.toAct = this.firstToAct(this.next(this.button));
    if (this.toAct === null || canStillAct.length <= 1) {
      // Everyone left is all-in: run the board out.
      this.toAct = null;
      return this.settleIfDone();
    }
  }

  finish() {
    this.phase = "done";
    this.toAct = null;
    const live = this.inHand();
    const order = [];
    for (let k = 0, i = this.next(this.button); k < this.n(); k++, i = this.next(i)) order.push(i);
    const pots = buildPots(this.seats);
    let values, shown = [];
    if (live.length === 1) {
      values = this.seats.map((p, i) => (i === live[0] ? 1 : 0));
    } else {
      values = this.seats.map(p => (p.folded ? -1 : evaluate([...p.hole, ...this.board])));
      shown = live;
    }
    const won = awardPots(pots, values, order);
    for (const [i, amt] of Object.entries(won)) this.seats[i].stack += amt;
    const describe = i => (shown.length ? ` with ${CATEGORY_NAMES[category(values[i])]}` : "");
    const parts = Object.entries(won).map(([i, amt]) => `${this.seats[i].name} wins ${amt}${describe(i)}`);
    parts.forEach(t => this.log(t));
    this.result = { won, shown, pots, values };
  }
}

// ---------------------------------------------------------------- bots

const OPEN = { UTG: 0.16, HJ: 0.2, CO: 0.27, BTN: 0.44, SB: 0.38, BB: 0.15 };

function ramp(pct, threshold, width = 0.05) {
  return Math.max(0, Math.min(1, (threshold + width / 2 - pct) / width));
}

// Postflop "how plausible is this holding for a player who kept betting".
function holdingStrength(hole, board) {
  if (board.length < 3) return 1;
  const made = category(evaluate([...hole, ...board]));
  const onBoard = category(evaluate(board));
  if (made - onBoard >= 2 || made >= 4) return 1;
  if (made > onBoard) {
    const top = Math.max(...board.map(c => c >> 2));
    const paired = hole.some(c => (c >> 2) >= top) ? 0.85 : 0.55;
    return paired;
  }
  const suits = [0, 0, 0, 0];
  for (const c of [...hole, ...board]) suits[c & 3]++;
  const flushDraw = hole.some(c => suits[c & 3] >= 4);
  return flushDraw ? 0.5 : 0.18;
}

// Monte Carlo equity of `hole` against opponents sampled from their ranges.
function equity(game, hero, iters, rng, PCT) {
  const me = game.seats[hero];
  const opps = game.inHand().filter(i => i !== hero).map(i => game.seats[i]);
  const dead = new Set([...me.hole, ...game.board]);
  const live = [...Array(52).keys()].filter(c => !dead.has(c));
  let score = 0;
  for (let it = 0; it < iters; it++) {
    const avail = live.slice();
    const take = () => { const k = Math.floor(rng() * avail.length); const c = avail[k]; avail[k] = avail[avail.length - 1]; avail.pop(); return c; };
    const hands = [];
    for (const o of opps) {
      let pick = null;
      for (let tries = 0; tries < 30; tries++) {
        const k1 = Math.floor(rng() * avail.length);
        let k2 = Math.floor(rng() * (avail.length - 1)); if (k2 >= k1) k2++;
        const h = [avail[k1], avail[k2]];
        const pctOk = PCT[handClass(h[0], h[1])] <= o.range + 0.03 * rng();
        const post = game.board.length >= 3 && o.aggr > 0 ? holdingStrength(h, game.board) ** Math.min(o.aggr, 3) : 1;
        if ((pctOk && rng() < post) || tries === 29) { pick = [k1, k2]; break; }
      }
      const [k1, k2] = pick[0] > pick[1] ? pick : [pick[1], pick[0]];
      const c1 = avail[k1]; avail[k1] = avail[avail.length - 1]; avail.pop();
      const c2 = avail[k2]; avail[k2] = avail[avail.length - 1]; avail.pop();
      hands.push([c1, c2]);
    }
    const board = game.board.slice();
    while (board.length < 5) board.push(take());
    const mine = evaluate([...me.hole, ...board]);
    let best = true, ties = 1;
    for (const h of hands) {
      const v = evaluate([...h, ...board]);
      if (v > mine) { best = false; break; }
      if (v === mine) ties++;
    }
    if (best) score += 1 / ties;
  }
  return score / iters;
}

// Regret matching over estimated action values: play each action in proportion
// to how much better it looks than the average action.
function regretMatch(evs) {
  const keys = Object.keys(evs);
  const mean = keys.reduce((s, k) => s + evs[k], 0) / keys.length;
  const w = {};
  let total = 0;
  for (const k of keys) { w[k] = Math.max(0, evs[k] - mean) ** 1.3; total += w[k]; }
  if (total === 0) {
    const best = keys.reduce((a, b) => (evs[a] >= evs[b] ? a : b));
    keys.forEach(k => (w[k] = k === best ? 1 : 0));
    return w;
  }
  keys.forEach(k => (w[k] /= total));
  return w;
}

function sample(probs, rng) {
  let x = rng(), last = null;
  for (const [k, p] of Object.entries(probs)) { last = k; if ((x -= p) < 0) return k; }
  return last;
}

// The bot's decision and its reasoning. `style` = {loose, aggr}.
function decide(game, seat, style, rng, PCT) {
  const p = game.seats[seat];
  const o = game.options(seat);
  const pos = game.position(seat);
  const cls = handClass(p.hole[0], p.hole[1]);
  const bb = BIG_BLIND;

  if (game.street === 0) {
    const pct = PCT[cls];
    const r = game.raisesThisStreet;
    let probs, sizes = {};
    const limpers = game.seats.filter((q, i) => i !== seat && !q.folded && q.bet === bb && q.lastAction === "Call").length;
    if (r === 0) {
      const thr = (o.canCheck ? 0.12 : OPEN[pos]) * style.loose;
      const raise = o.canRaise ? ramp(pct, thr) : 0;
      probs = o.canCheck ? { check: 1 - raise, raise } : { fold: 1 - raise, raise };
      sizes.raise = (2.5 + limpers) * bb + (pos === "SB" ? 0.5 * bb : 0);
    } else if (r === 1) {
      const facing = game.currentBet;
      const value = 0.05 * style.aggr;
      const suitedAce = cls[0] === "A" && cls[2] === "s";
      const bluff = suitedAce && pct > 0.12 && pct < 0.45 ? 0.3 * style.aggr : 0;
      const base = pos === "BB" ? 0.4 : pos === "SB" ? 0.17 : 0.22;
      const callThr = base * style.loose * Math.sqrt(2.5 * bb / facing);
      const raise = o.canRaise ? Math.min(1, ramp(pct, value, 0.03) + bluff) : 0;
      const call = Math.max(0, ramp(pct, callThr) - raise);
      probs = { fold: Math.max(0, 1 - raise - call), call, raise };
      const ip = ["BTN", "CO"].includes(pos);
      sizes.raise = facing * (ip ? 3 : 3.6);
    } else {
      const jam = o.toCall >= p.stack * 0.45;
      const raise = o.canRaise ? ramp(pct, 0.02 * style.aggr, 0.015) : 0;
      const call = Math.max(0, ramp(pct, (jam ? 0.05 : 0.08) * style.loose, 0.03) - raise);
      probs = { fold: Math.max(0, 1 - raise - call), call, raise };
      sizes.raise = jam ? o.maxTo : game.currentBet * 2.3;
    }
    const pick = sample(probs, rng);
    const action = pick === "raise" ? { type: "raise", to: sizes.raise } : { type: pick };
    return { action, why: { street: "preflop", cls, pct, probs, pick } };
  }

  // Postflop: Monte Carlo equity, crude action values, regret matching.
  const opps = game.inHand().filter(i => i !== seat);
  const eq = equity(game, seat, opps.length >= 3 ? 300 : 450, rng, PCT);
  const pot = o.pot;
  const ip = opps.every(i => (i - game.button + game.n()) % game.n() < (seat - game.button + game.n()) % game.n() || (seat === game.button));
  const realize = (ip ? 1 : 0.88) * 0.95 ** (opps.length - 1);
  const evs = {};
  const sizes = {};
  if (o.canCheck) evs.check = eq * pot * realize;
  else {
    evs.fold = 0;
    evs.call = eq * realize * (pot + o.toCall) - o.toCall;
  }
  if (o.canRaise) {
    const targets = o.canCheck
      ? { "bet 33%": 0.33, "bet 66%": 0.66, "bet pot": 1.0 }
      : { "raise 3x": null };
    for (const [label, f] of Object.entries(targets)) {
      const to = f === null ? Math.max(o.minTo, game.currentBet * 3) : Math.max(o.minTo, Math.round(f * pot));
      sizes[label] = Math.min(to, o.maxTo);
    }
    if (p.stack <= 2.5 * pot || eq > 0.8) sizes["all-in"] = o.maxTo;
    for (const [label, to] of Object.entries(sizes)) {
      const add = to - p.bet;
      const pressure = add / (pot + add);
      const fold = opps.reduce((acc, i) => {
        const q = game.seats[i];
        const shown = q.bet > 0 ? 0.55 : 1;
        return acc * Math.min(0.85, Math.max(0.03, pressure * 1.15 * shown * (1.1 - Math.min(q.aggr, 2) * 0.2)));
      }, 1);
      const eqCalled = Math.max(0, eq - 0.12 * Math.min(add / pot, 2));
      const calledPot = pot + add + Math.max(0, to - game.currentBet);
      evs[label] = fold * pot + (1 - fold) * (eqCalled * calledPot * realize - add) + (style.aggr - 1) * 0.06 * pot;
    }
  }
  const probs = regretMatch(evs);
  const pick = sample(probs, rng);
  let action;
  if (pick === "check" || pick === "fold" || pick === "call") action = { type: pick };
  else action = { type: "raise", to: sizes[pick] };
  return { action, why: { street: game.streetName(), eq, potOdds: o.toCall ? o.toCall / (pot + o.toCall) : 0, evs, probs, pick } };
}

if (typeof module !== "undefined") {
  module.exports = { Game, decide, equity, regretMatch, buildPercentiles, makeRng, BIG_BLIND, START_STACK, POSITIONS };
}
