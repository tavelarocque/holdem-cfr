// Heads-up mode: you against the trained blueprint, on its own abstract tree.
(() => {
  const bp = new Blueprint(BLUEPRINT);
  const BB = bp.params.bb, STACK = bp.params.stack;
  const rng = makeRng((crypto.getRandomValues(new Uint32Array(1))[0]) || 11);
  const $ = id => document.getElementById(id);
  const els = {
    table: $("hTable"), board: $("hBoard"), pot: $("hPot"), status: $("hStatus"), mine: $("hMine"),
    coach: $("hCoach"), acts: $("hActs"), ask: $("hAsk"), next: $("hNext"), feed: $("hFeed"),
    score: $("hScore"), hands: $("hHands"), net: $("hNet"), rate: $("hRate"), ci: $("hCI"), hist: $("hHist"),
    speed: $("speed"), reveal: $("reveal"), auto: $("auto"), meta: $("meta"),
  };
  const STREETS = ["preflop", "flop", "turn", "river"];
  const BOARD_LEN = [0, 3, 4, 5];
  const SUIT = ["♣", "♦", "♥", "♠"];
  const bb = x => {
    const v = x / BB;
    return (Number.isInteger(v) ? v.toString() : v.toFixed(1)) + " bb";
  };
  const signed = x => (x > 0 ? "+" : x < 0 ? "−" : "") + bb(Math.abs(x));
  function cardEl(c, size = "") {
    const d = document.createElement("div");
    if (c === null) { d.className = `card back ${size}`; return d; }
    if (c === undefined) { d.className = `card slot ${size}`; return d; }
    const red = (c & 3) === 1 || (c & 3) === 2;
    d.className = `card ${size}${red ? " red" : ""}`;
    d.innerHTML = `<span class="r">${RANKS[c >> 2] === "T" ? "10" : RANKS[c >> 2]}</span><span class="s">${SUIT[c & 3]}</span>`;
    d.setAttribute("aria-label", cardName(c));
    return d;
  }

  $("hIntro").innerHTML =
    `You are playing heads-up no-limit hold'em against the holdem-cfr blueprint: <b>${(bp.iterations / 1e6).toFixed(0)} million</b> ` +
    `iterations of external-sampling MCCFR. Blinds 0.5/1 bb, both stacks reset to 100 bb every hand, and you swap the button each hand. ` +
    `It sees cards only through its abstraction (169 preflop classes, then 50 E[HS²] buckets on the flop and turn and 50 HS buckets on the river), ` +
    `recomputed here from the cards exactly as the Rust code does. Its reasoning for a hand appears when the hand ends, or live if you tick Show bot cards.`;
  $("aboutHU").textContent =
    "The heads-up mode runs the raw blueprint: an average strategy over an abstract game, with no real-time search. Your bet sizes come from the blueprint's own menu " +
    "(one open size, 33% or 75% pot on the flop, 66% on the turn, 66% or 125% on the river, pot-sized raises, all-in), so it never has to interpret an off-tree bet. " +
    "Its known weaknesses are the ones the abstraction creates: it cannot tell apart hands inside one bucket, and it forgets earlier streets' buckets.";

  // ---------- seats: 0 = you (bottom), 1 = blueprint (top) ----------
  const names = ["You", "Blueprint"];
  const seatEls = [0, 1].map(i => {
    const s = document.createElement("div");
    s.className = "seat";
    s.style.setProperty("--x", `var(--s${i}x)`);
    s.style.setProperty("--y", `var(--s${i}y)`);
    s.innerHTML = `<div class="hole"></div><div class="plate"><div class="nm">${names[i]}${i ? '<span class="tag">CFR</span>' : ""}</div><div class="stk"></div><span class="badge btn" hidden>BTN</span></div><div class="act"></div>`;
    els.table.appendChild(s);
    const chip = document.createElement("div");
    chip.className = "betchip";
    chip.hidden = true;
    chip.style.left = "50%";
    chip.style.top = i ? "28%" : "70%";
    els.table.appendChild(chip);
    return { root: s, hole: s.querySelector(".hole"), stk: s.querySelector(".stk"), badge: s.querySelector(".badge"), act: s.querySelector(".act"), chip };
  });

  // ---------- session score (kept per viewer) ----------
  let score = { hands: 0, net: 0, sq: 0, hist: [] };
  try { const s = JSON.parse(localStorage.getItem("hcfr-hu-score")); if (s && typeof s.hands === "number") score = s; } catch (e) {}
  const saveScore = () => { try { localStorage.setItem("hcfr-hu-score", JSON.stringify(score)); } catch (e) {} };
  function renderScore() {
    els.hands.textContent = score.hands;
    els.net.textContent = signed(score.net);
    els.net.className = score.net > 0 ? "pos" : score.net < 0 ? "neg" : "";
    if (score.hands) {
      const mean = score.net / score.hands / BB; // bb per hand
      els.rate.textContent = (mean * 100 >= 0 ? "+" : "") + (mean * 100).toFixed(1);
      els.rate.className = mean > 0 ? "pos" : mean < 0 ? "neg" : "";
      if (score.hands >= 30) {
        const varr = Math.max(0, score.sq / score.hands / (BB * BB) - mean * mean);
        const ci = 1.96 * Math.sqrt(varr / score.hands) * 100;
        els.ci.textContent = `95% interval: ${(mean * 100).toFixed(1)} ± ${ci.toFixed(1)} bb/100 over ${score.hands} hands. ` +
          (Math.abs(mean * 100) > ci ? "That is a statistically clear result." : "Still consistent with an even match: keep playing.");
      }
    } else {
      els.rate.textContent = "—";
    }
    els.hist.replaceChildren(...score.hist.slice(-40).reverse().map(t => {
      const d = document.createElement("div");
      d.className = "logline" + (t.includes(" +") ? " win" : "");
      d.textContent = t;
      return d;
    }));
  }
  renderScore();

  // ---------- hand state ----------
  let handNo = score.hands;
  let you = 0;            // your seat in the tree (0 = button / small blind)
  let holes, board, path, node, contributed, visible = 0, done = false, lastAct = ["", ""];
  const bucketCache = [[], []];
  const pending = [];     // bot reasoning held back until the hand ends

  const seatOf = p => (p === you ? 0 : 1); // screen seat of tree player p
  function bucketFor(p, street) {
    if (bucketCache[p][street] === undefined) {
      const b = board.slice(0, BOARD_LEN[street]);
      bucketCache[p][street] = { bucket: bp.bucket(holes[p], b), value: street ? metric(holes[p], b) : null };
    }
    return bucketCache[p][street];
  }

  function render() {
    const potNow = contributed ? contributed[0] + contributed[1] : 0;
    for (const p of [0, 1]) {
      const e = seatEls[seatOf(p)];
      const live = contributed && !done && !bp.isTerminal(node);
      e.root.classList.toggle("turn", Boolean(live && bp.player(node) === p));
      e.badge.hidden = p !== 0;
      e.stk.textContent = contributed ? bb(STACK - contributed[p]) : bb(STACK);
      e.act.textContent = lastAct[p];
      const streetIn = contributed ? contributed[p] - streetBase(p) : 0;
      e.chip.hidden = !(streetIn > 0 && !done);
      e.chip.textContent = bb(streetIn);
      e.hole.replaceChildren();
      if (!holes || p === you) continue;
      const faceUp = els.reveal.checked || (done && showdown);
      for (const c of holes[p]) e.hole.appendChild(cardEl(faceUp ? c : null, "sm"));
    }
    els.board.replaceChildren();
    for (let k = 0; k < 5; k++) els.board.appendChild(cardEl(board && k < visible ? board[k] : undefined));
    els.pot.textContent = `Pot ${bb(potNow)}`;
    els.mine.replaceChildren();
    if (holes) for (const c of holes[you]) els.mine.appendChild(cardEl(c, "lg"));
    if (MODE.current === "hu") els.meta.textContent = `heads-up · blueprint ${(bp.iterations / 1e6).toFixed(0)}M iterations · hand #${handNo}`;
  }

  // Chips each player had in at the start of the current street.
  let streetStart = [0, 0];
  const streetBase = p => streetStart[p];

  // Raise-to amounts in the tree count the whole hand; show them per street.
  function describe(nd, a) {
    const l = bp.label(nd, a, contributed);
    const p = bp.player(nd);
    const pot = contributed[0] + contributed[1];
    if (l.kind === "call") return `Call ${bb(l.amount)}`;
    if (l.kind === "allin") return `All-in ${bb(STACK)}`;
    if (l.kind === "bet") {
      const amt = l.to - streetStart[p];
      return `Bet ${bb(amt)} · ${Math.round((100 * amt) / pot)}%`;
    }
    if (l.kind === "raise") return `Raise to ${bb(l.to - streetStart[p])}`;
    return l.text;
  }

  function entry(title, where, stats, probs, labels, chosen, cls = "") {
    const d = document.createElement("div");
    d.className = "entry wide" + cls;
    const rows = probs.map((v, a) => `<div class="prob${a === chosen ? " chosen" : ""}"><span class="lbl">${labels[a]}</span><span class="track"><span class="fill" style="width:${(v * 100).toFixed(1)}%;display:block"></span></span><span class="pc">${Math.round(v * 100)}%</span></div>`).join("");
    d.innerHTML = `<div class="hd"><b>${title}</b><span>${where}</span></div><div class="stats">${stats}</div>${rows}`;
    return d;
  }
  function pushFeed(d) {
    els.feed.insertBefore(d, els.feed.children[1] || null);
    while (els.feed.children.length > 80) els.feed.lastChild.remove();
  }
  function strengthText(p, street) {
    const { bucket, value } = bucketFor(p, street);
    const cls = classOf(holes[p][0], holes[p][1]);
    if (street === 0) return `${cls} · preflop class ${bucket + 1} of 169`;
    const what = street === 3 ? "HS" : "E[HS²]";
    return `${cls} · ${STREETS[street]} bucket ${bucket + 1} of 50 · ${what} ${value.toFixed(3)}`;
  }

  // ---------- your turn ----------
  let resolveTurn = null;
  function showActions(on) {
    els.acts.replaceChildren();
    els.ask.disabled = !on;
    if (!on) return;
    bp.actions(node).forEach((x, a) => {
      const b = document.createElement("button");
      const l = bp.label(node, a, contributed);
      b.className = "btnx" + (l.kind === "raise" || l.kind === "bet" ? " primary" : l.kind === "allin" ? " allin" : "");
      b.textContent = describe(node, a);
      b.dataset.kind = l.kind;
      b.addEventListener("click", () => { if (resolveTurn) { const r = resolveTurn; resolveTurn = null; showActions(false); r(a); } });
      els.acts.appendChild(b);
    });
  }
  document.addEventListener("keydown", e => {
    if (!resolveTurn || MODE.current !== "hu") return;
    const k = e.key.toLowerCase();
    const pick = kinds => [...els.acts.children].find(b => kinds.includes(b.dataset.kind));
    const b = k === "f" ? pick(["fold"]) : k === "c" ? pick(["check", "call"]) : null;
    if (b) b.click();
  });
  els.ask.addEventListener("click", () => {
    if (!resolveTurn) return;
    const street = bp.street(node);
    const probs = bp.probs(node, bucketFor(you, street).bucket);
    const labels = bp.actions(node).map((_, a) => describe(node, a));
    pushFeed(entry("Blueprint's play with your hand", `${you === 0 ? "BTN" : "BB"} · ${STREETS[street]}`, strengthText(you, street), probs, labels, -1, " you"));
    showTab("model");
  });

  function coachText() {
    const street = bp.street(node);
    const me = holes[you];
    const toCall = contributed[1 - you] - contributed[you];
    const odds = toCall ? ` Calling ${bb(toCall)} into ${bb(contributed[0] + contributed[1])} needs ${(100 * toCall / (contributed[0] + contributed[1] + toCall)).toFixed(0)}% equity.` : "";
    if (street === 0) {
      const cls = classOf(me[0], me[1]);
      return `<b>${cls}</b>, ${(PREFLOP[cls][0] * 100).toFixed(1)}% equity against a random hand.${odds}`;
    }
    const made = CATEGORY_NAMES[category(evaluate([...me, ...board.slice(0, BOARD_LEN[street])]))];
    return `You have <b>${made}</b>.${odds}`;
  }

  // ---------- tabs ----------
  function showTab(which) {
    const model = which === "model";
    $("hTabModel").setAttribute("aria-selected", model);
    $("hTabScore").setAttribute("aria-selected", !model);
    els.feed.hidden = !model;
    els.score.hidden = model;
  }
  $("hTabModel").addEventListener("click", () => showTab("model"));
  $("hTabScore").addEventListener("click", () => showTab("score"));
  els.reveal.addEventListener("change", () => { flushPending(); render(); });

  function flushPending() {
    if (!done && !els.reveal.checked) return;
    while (pending.length) pushFeed(pending.shift());
  }

  // ---------- main loop ----------
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const speed = () => Number(els.speed.value);
  let waitNext = null;
  let showdown = false;
  els.next.addEventListener("click", () => { if (waitNext) { const r = waitNext; waitNext = null; r(); } });
  els.auto.addEventListener("change", () => {
    if (els.auto.checked && waitNext) { const r = waitNext; waitNext = null; els.next.hidden = true; r(); }
  });

  function shuffleDeal() {
    const deck = Array.from({ length: 52 }, (_, i) => i);
    for (let i = 0; i < 9; i++) {
      const j = i + Math.floor(rng() * (52 - i));
      [deck[i], deck[j]] = [deck[j], deck[i]];
    }
    holes = [[deck[0], deck[1]], [deck[2], deck[3]]];
    board = deck.slice(4, 9);
  }

  async function revealTo(n) {
    while (visible < n) {
      visible = visible < 3 ? 3 : visible + 1;
      render();
      await sleep(Math.max(250, speed() * 0.7));
    }
  }

  async function playHand() {
    await waitMode("hu");
    handNo += 1;
    you = handNo % 2 === 1 ? 0 : 1;
    shuffleDeal();
    bucketCache[0] = []; bucketCache[1] = [];
    path = []; node = 0; done = false; showdown = false; visible = 0;
    lastAct = ["", ""];
    contributed = [bp.params.sb, bp.params.bb];
    streetStart = [0, 0];
    let street = 0;
    els.status.textContent = you === 0 ? "You have the button" : "The blueprint has the button";
    els.coach.innerHTML = "";
    render();
    await sleep(350);
    while (!bp.isTerminal(node)) {
      const p = bp.player(node);
      const st = bp.street(node);
      if (st !== street) {
        street = st;
        streetStart = contributed.slice();
        lastAct = ["", ""];
        await revealTo(BOARD_LEN[st]);
      }
      render();
      let a;
      const labels = bp.actions(node).map((_, k) => describe(node, k));
      if (p === you) {
        els.status.textContent = "Your turn";
        els.coach.innerHTML = coachText();
        showActions(true);
        a = await new Promise(r => (resolveTurn = r));
        els.status.textContent = "";
      } else {
        await waitMode("hu");
        els.status.textContent = "Blueprint is thinking…";
        await sleep(speed());
        const probs = bp.probs(node, bucketFor(p, st).bucket);
        let x = rng();
        a = probs.length - 1;
        for (let k = 0; k < probs.length; k++) { if (x < probs[k]) { a = k; break; } x -= probs[k]; }
        pending.push(entry("Blueprint", `${p === 0 ? "BTN" : "BB"} · ${STREETS[st]} · hand #${handNo}`, strengthText(p, st), probs, labels, a));
        flushPending();
        els.status.textContent = "";
      }
      lastAct[p] = labels[a];
      path.push(a);
      const next = contributionsAlong(bp, path);
      node = next.node;
      contributed = next.contributed;
      render();
    }
    // Terminal.
    done = true;
    const folded = bp.folded(node);
    contributed = bp.contributed(node);
    if (folded < 0) { showdown = true; await revealTo(5); }
    const won0 = payoff(bp, node, holes, board);
    const mine = you === 0 ? won0 : -won0;
    let msg;
    if (folded >= 0) {
      msg = folded === you ? `You fold. Blueprint wins ${bb(contributed[you] * 2 > 0 ? contributed[you] : 0)}` : `Blueprint folds. You win ${bb(contributed[1 - you])}`;
    } else {
      const vYou = evaluate([...holes[you], ...board]), vBot = evaluate([...holes[1 - you], ...board]);
      const how = v => CATEGORY_NAMES[category(v)];
      msg = mine > 0 ? `You win ${bb(mine)} with ${how(vYou)}` : mine < 0 ? `Blueprint wins ${bb(-mine)} with ${how(vBot)}` : `Split pot, ${how(vYou)}`;
    }
    els.status.textContent = msg;
    score.hands += 1;
    score.net += mine;
    score.sq += mine * mine;
    const cls = p => classOf(holes[p][0], holes[p][1]);
    score.hist.push(`#${handNo} ${you === 0 ? "BTN" : "BB "} ${cls(you)} vs ${showdown ? cls(1 - you) : "??"} · ${mine >= 0 ? "+" : "−"}${bb(Math.abs(mine))}`);
    if (score.hist.length > 200) score.hist.shift();
    saveScore();
    renderScore();
    flushPending();
    render();
    if (els.auto.checked) {
      await sleep(Math.max(1800, speed() * 3.5));
    } else {
      els.next.hidden = false;
      await new Promise(res => (waitNext = res));
      els.next.hidden = true;
    }
  }

  window.addEventListener("hcfr-mode", () => { if (MODE.current === "hu") render(); });
  $("mHU").addEventListener("click", () => setMode("hu"));
  $("m6").addEventListener("click", () => setMode("6max"));
  let start = "hu";
  try { if (localStorage.getItem("hcfr-mode") === "6max") start = "6max"; } catch (e) {}
  setMode(start);
  render();
  (async () => { for (;;) await playHand(); })();
})();
