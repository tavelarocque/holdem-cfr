// Pure poker logic shared by the table page and its node tests.
// Cards are ints 0..51: rank = c >> 2 (0 = deuce .. 12 = ace), suit = c & 3.

const RANKS = "23456789TJQKA";
const SUITS = "cdhs";
const CATEGORY_NAMES = [
  "high card", "a pair", "two pair", "three of a kind", "a straight",
  "a flush", "a full house", "four of a kind", "a straight flush",
];

function popcount(x) { let n = 0; while (x) { x &= x - 1; n++; } return n; }

// Rank of the top card of the best straight in a 13-bit rank mask, or -1.
function straightHigh(mask) {
  const m = (mask << 1) | ((mask >> 12) & 1); // bit 0 = ace playing low
  for (let hi = 13; hi >= 4; hi--) {
    if (((m >> (hi - 4)) & 31) === 31) return hi - 1;
  }
  return -1;
}

// Highest `n` ranks in `mask` not in `exclude`, as an array (descending).
function kickers(mask, exclude, n) {
  const out = [];
  for (let r = 12; r >= 0 && out.length < n; r--) {
    if ((mask >> r) & 1 && !exclude.includes(r)) out.push(r);
  }
  return out;
}

function pack(ks, shift) {
  let v = 0;
  ks.forEach((k, i) => { v |= k << (shift - 4 * i); });
  return v;
}

// Strength of the best five of 5-7 cards: higher wins; category = value >> 20.
function evaluate(cards) {
  const rc = new Array(13).fill(0);
  const sm = [0, 0, 0, 0];
  let rm = 0;
  for (const c of cards) {
    const r = c >> 2, s = c & 3;
    rc[r]++; sm[s] |= 1 << r; rm |= 1 << r;
  }
  for (let s = 0; s < 4; s++) {
    if (popcount(sm[s]) >= 5) {
      const sf = straightHigh(sm[s]);
      if (sf >= 0) return (8 << 20) | (sf << 16);
      return (5 << 20) | pack(kickers(sm[s], [], 5), 16);
    }
  }
  let quad = -1; const trips = [], pairs = [];
  for (let r = 12; r >= 0; r--) {
    if (rc[r] === 4) quad = r;
    else if (rc[r] === 3) trips.push(r);
    else if (rc[r] === 2) pairs.push(r);
  }
  if (quad >= 0) return (7 << 20) | (quad << 16) | (kickers(rm, [quad], 1)[0] << 12);
  if (trips.length >= 2 || (trips.length && pairs.length)) {
    const p = trips.length >= 2 ? Math.max(trips[1], pairs.length ? pairs[0] : -1) : pairs[0];
    return (6 << 20) | (trips[0] << 16) | (p << 12);
  }
  const st = straightHigh(rm);
  if (st >= 0) return (4 << 20) | (st << 16);
  if (trips.length) return (3 << 20) | (trips[0] << 16) | pack(kickers(rm, [trips[0]], 2), 12);
  if (pairs.length >= 2) {
    return (2 << 20) | (pairs[0] << 16) | (pairs[1] << 12) | (kickers(rm, [pairs[0], pairs[1]], 1)[0] << 8);
  }
  if (pairs.length) return (1 << 20) | (pairs[0] << 16) | pack(kickers(rm, [pairs[0]], 3), 12);
  return pack(kickers(rm, [], 5), 16);
}

function category(value) { return value >> 20; }

// "AKs", "T9o", "77" for two hole cards.
function handClass(a, b) {
  let hi = a, lo = b;
  if ((b >> 2) > (a >> 2)) { hi = b; lo = a; }
  const h = RANKS[hi >> 2], l = RANKS[lo >> 2];
  if ((hi >> 2) === (lo >> 2)) return h + l;
  return h + l + ((hi & 3) === (lo & 3) ? "s" : "o");
}

function cardName(c) { return RANKS[c >> 2] + SUITS[c & 3]; }

// Split committed chips into a main pot and side pots.
// players: [{committed, folded}] -> [{amount, eligible: [seat indices]}]
function buildPots(players) {
  const levels = [...new Set(players.map(p => p.committed).filter(x => x > 0))].sort((a, b) => a - b);
  const pots = [];
  let prev = 0;
  for (const level of levels) {
    const contributors = players.filter(p => p.committed >= level).length;
    const amount = (level - prev) * contributors;
    const eligible = players.map((p, i) => i).filter(i => !players[i].folded && players[i].committed >= level);
    if (amount > 0) {
      const last = pots[pots.length - 1];
      if (last && last.eligible.join() === eligible.join()) last.amount += amount;
      else pots.push({ amount, eligible });
    }
    prev = level;
  }
  return pots;
}

// Pay each pot to its best eligible hands. `order` lists seats starting left of
// the button, so odd chips go to the first winner in that order.
function awardPots(pots, values, order) {
  const won = {};
  for (const pot of pots) {
    if (!pot.eligible.length) continue;
    const best = Math.max(...pot.eligible.map(i => values[i]));
    const winners = order.filter(i => pot.eligible.includes(i) && values[i] === best);
    const share = Math.floor(pot.amount / winners.length);
    let odd = pot.amount - share * winners.length;
    for (const w of winners) {
      won[w] = (won[w] || 0) + share + (odd > 0 ? 1 : 0);
      if (odd > 0) odd--;
    }
  }
  return won;
}

if (typeof module !== "undefined") {
  module.exports = { evaluate, category, straightHigh, handClass, cardName, buildPots, awardPots, CATEGORY_NAMES, RANKS, SUITS };
}
