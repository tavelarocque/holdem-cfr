//! Vectorized CFR on a postflop spot, parallel over chance cards.
//!
//! An information set is (public node, private hand). Rather than visiting each
//! hand separately, every traversal carries a whole *vector* over hands: the
//! opponent's reach probability for each of their hands going down, and the
//! traverser's counterfactual value for each of their hands coming back up. Each
//! public node is visited once per traversal, and terminal nodes are evaluated
//! for all hands at once in linear time (see [`Spot::showdown_values`]).
//!
//! Storage is action-major: the regret of action `a` for hand `h` at a node with
//! offset `o` lives at `o + a * n + h`, so every inner loop runs over contiguous
//! hands.
//!
//! **Parallelism.** Nodes are stored in pre-order and offsets are handed out in
//! node order, so every subtree owns one contiguous range of each player's arrays,
//! and sibling subtrees own disjoint, increasing ranges. At a chance node the
//! arrays are therefore cut with `split_at_mut` into one exclusive slice per
//! child, and the children run in parallel with rayon in entirely safe Rust.
//! Child values are combined in a fixed order afterwards, so results are
//! bit-for-bit identical whatever the thread count.

use rayon::prelude::*;

use crate::spot::Spot;
use crate::tree::{Action, NodeKind};

/// Regret-update rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Algorithm {
    /// Regrets floored at zero, average weighted by iteration.
    CfrPlus,
    /// Discounted CFR (Brown & Sandholm 2019).
    Discounted { alpha: f64, beta: f64, gamma: f64 },
}

impl Algorithm {
    pub const DCFR: Algorithm = Algorithm::Discounted {
        alpha: 1.5,
        beta: 0.0,
        gamma: 2.0,
    };
}

/// Half-open range of offsets a subtree owns in one player's arrays.
type Span = (usize, usize);

pub struct Solver<'s> {
    spot: &'s Spot,
    algorithm: Algorithm,
    /// Offset of each action node into its player's arrays (`usize::MAX` otherwise).
    offset: Vec<usize>,
    /// Per node, per player: the offsets owned by the node's subtree.
    span: Vec<[Span; 2]>,
    regrets: [Vec<f32>; 2],
    strategy_sum: [Vec<f32>; 2],
    iteration: u32,
}

/// Read-only context shared by every thread during a traversal.
struct Ctx<'a> {
    spot: &'a Spot,
    offset: &'a [usize],
    span: &'a [[Span; 2]],
    algorithm: Algorithm,
    weight: f32,
}

/// Exclusive access to one subtree's slice of both players' arrays.
/// `base[p]` is the global offset of `regrets[p][0]`.
struct View<'a> {
    regrets: [&'a mut [f32]; 2],
    sums: [&'a mut [f32]; 2],
    base: [usize; 2],
}

/// How strategies are chosen during an evaluation pass.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Traverser plays a best response to the opponent's average strategy.
    BestResponse,
    /// Both play their average strategies.
    Average,
}

fn regret_matching(regrets: &[f32], n: usize, out: &mut [f32]) {
    let actions = regrets.len() / n;
    let mut sums = vec![0f32; n];
    for a in 0..actions {
        for (s, &r) in sums.iter_mut().zip(&regrets[a * n..(a + 1) * n]) {
            *s += r.max(0.0);
        }
    }
    let uniform = 1.0 / actions as f32;
    for a in 0..actions {
        let (src, dst) = (&regrets[a * n..(a + 1) * n], &mut out[a * n..(a + 1) * n]);
        for h in 0..n {
            dst[h] = if sums[h] > 0.0 {
                src[h].max(0.0) / sums[h]
            } else {
                uniform
            };
        }
    }
}

fn normalize(sum: &[f32], n: usize, out: &mut [f32]) {
    let actions = sum.len() / n;
    let mut totals = vec![0f32; n];
    for a in 0..actions {
        for (t, &s) in totals.iter_mut().zip(&sum[a * n..(a + 1) * n]) {
            *t += s;
        }
    }
    let uniform = 1.0 / actions as f32;
    for a in 0..actions {
        for h in 0..n {
            out[a * n + h] = if totals[h] > 0.0 {
                sum[a * n + h] / totals[h]
            } else {
                uniform
            };
        }
    }
}

/// Bytes the solver would allocate for `spot`, without allocating it.
pub fn required_memory(spot: &Spot) -> usize {
    let entries: usize = spot
        .tree
        .nodes
        .iter()
        .map(|n| match &n.kind {
            NodeKind::Action {
                player, actions, ..
            } => actions.len() * spot.num_hands(*player),
            _ => 0,
        })
        .sum();
    entries * 2 * std::mem::size_of::<f32>()
}

/// Adds one chance class's values into `out`: the canonical card's values for
/// itself, and permuted copies for every other card in its class.
fn accumulate_class(
    spot: &Spot,
    trav: usize,
    members: &[(eval::Card, usize)],
    p: f64,
    v: &[f64],
    out: &mut [f64],
) {
    for &(_, g) in members {
        let inv = &spot.inverse[g][trav];
        for (h, o) in out.iter_mut().enumerate() {
            *o += p * v[inv[h] as usize];
        }
    }
}

fn train(
    ctx: &Ctx,
    view: &mut View,
    node: usize,
    trav: usize,
    trav_reach: &[f32],
    opp_reach: &[f32],
) -> Vec<f32> {
    let spot = ctx.spot;
    let n = spot.num_hands(trav);
    let opp = 1 - trav;
    let mut out = vec![0f32; n];
    let info = &spot.tree.nodes[node];
    match &info.kind {
        NodeKind::Fold { folder } => {
            spot.fold_values(trav, *folder, info.invested, opp_reach, &mut out);
        }
        NodeKind::Showdown => {
            spot.showdown_values(trav, info.board, info.invested, opp_reach, &mut out);
        }
        NodeKind::Chance {
            cards,
            children,
            members,
        } => {
            // Cut both players' arrays into one exclusive slice per child.
            let mut views = Vec::with_capacity(children.len());
            // Reborrow (not take) the parent's slices: the parent node still needs
            // them for its remaining actions once this chance node returns.
            let [r0, r1] = &mut view.regrets;
            let [s0, s1] = &mut view.sums;
            let mut rest: [&mut [f32]; 2] = [&mut **r0, &mut **r1];
            let mut rest_sums: [&mut [f32]; 2] = [&mut **s0, &mut **s1];
            let mut cursor = view.base;
            for &child in children {
                let mut r: [&mut [f32]; 2] = [&mut [], &mut []];
                let mut s: [&mut [f32]; 2] = [&mut [], &mut []];
                let mut base = [0; 2];
                for p in 0..2 {
                    let (lo, hi) = ctx.span[child][p];
                    if lo >= hi {
                        continue;
                    }
                    let skip = lo - cursor[p];
                    let (_, tail) = std::mem::take(&mut rest[p]).split_at_mut(skip);
                    let (mine, tail) = tail.split_at_mut(hi - lo);
                    rest[p] = tail;
                    let (_, tail) = std::mem::take(&mut rest_sums[p]).split_at_mut(skip);
                    let (mine_s, tail) = tail.split_at_mut(hi - lo);
                    rest_sums[p] = tail;
                    r[p] = mine;
                    s[p] = mine_s;
                    base[p] = lo;
                    cursor[p] = hi;
                }
                views.push(View {
                    regrets: r,
                    sums: s,
                    base,
                });
            }

            let values: Vec<Vec<f32>> = children
                .par_iter()
                .zip(cards.par_iter())
                .zip(views.into_par_iter())
                .map(|((&child, &card), mut child_view)| {
                    let c = card.index() as usize;
                    let mut tr = trav_reach.to_vec();
                    let mut or = opp_reach.to_vec();
                    for &h in &spot.with_card[trav][c] {
                        tr[h as usize] = 0.0;
                    }
                    for &o in &spot.with_card[opp][c] {
                        or[o as usize] = 0.0;
                    }
                    let mut v = train(ctx, &mut child_view, child, trav, &tr, &or);
                    for &h in &spot.with_card[trav][c] {
                        v[h as usize] = 0.0;
                    }
                    v
                })
                .collect();

            let p = spot.deal_probability(info.board);
            for (v, class) in values.iter().zip(members) {
                for &(_, g) in class {
                    let inv = &spot.inverse[g][trav];
                    for (h, o) in out.iter_mut().enumerate() {
                        *o += p * v[inv[h] as usize];
                    }
                }
            }
        }
        NodeKind::Action {
            player, children, ..
        } => {
            let player = *player;
            let off = ctx.offset[node] - view.base[player];
            let m = spot.num_hands(player);
            let len = children.len() * m;
            let mut sigma = vec![0f32; len];
            regret_matching(&view.regrets[player][off..off + len], m, &mut sigma);

            if player == trav {
                let mut values = Vec::with_capacity(children.len());
                for (a, &child) in children.iter().enumerate() {
                    let s = &sigma[a * n..(a + 1) * n];
                    let reach: Vec<f32> = trav_reach.iter().zip(s).map(|(r, s)| r * s).collect();
                    let v = train(ctx, view, child, trav, &reach, opp_reach);
                    for h in 0..n {
                        out[h] += s[h] * v[h];
                    }
                    values.push(v);
                }
                let regrets = &mut view.regrets[player][off..off + len];
                let sums = &mut view.sums[player][off..off + len];
                for (a, v) in values.iter().enumerate() {
                    for h in 0..n {
                        let k = a * n + h;
                        let r = regrets[k] + v[h] - out[h];
                        regrets[k] = match ctx.algorithm {
                            Algorithm::CfrPlus => r.max(0.0),
                            _ => r,
                        };
                        sums[k] += ctx.weight * trav_reach[h] * sigma[k];
                    }
                }
            } else {
                for (a, &child) in children.iter().enumerate() {
                    let s = &sigma[a * m..(a + 1) * m];
                    let reach: Vec<f32> = opp_reach.iter().zip(s).map(|(r, s)| r * s).collect();
                    let v = train(ctx, view, child, trav, trav_reach, &reach);
                    for (o, x) in out.iter_mut().zip(&v) {
                        *o += x;
                    }
                }
            }
        }
    }
    out
}

impl<'s> Solver<'s> {
    pub fn new(spot: &'s Spot, algorithm: Algorithm) -> Self {
        let nodes = &spot.tree.nodes;
        let mut offset = vec![usize::MAX; nodes.len()];
        let mut len = [0usize; 2];
        for (i, node) in nodes.iter().enumerate() {
            if let NodeKind::Action {
                player, actions, ..
            } = &node.kind
            {
                offset[i] = len[*player];
                len[*player] += actions.len() * spot.num_hands(*player);
            }
        }
        // Children have larger indices than parents, so a reverse sweep sees every
        // child's span before its parent's.
        let empty: Span = (usize::MAX, 0);
        let merge = |a: Span, b: Span| (a.0.min(b.0), a.1.max(b.1));
        let mut span = vec![[empty; 2]; nodes.len()];
        for i in (0..nodes.len()).rev() {
            let mut sp = [empty; 2];
            let children: &[usize] = match &nodes[i].kind {
                NodeKind::Action {
                    player,
                    actions,
                    children,
                } => {
                    let own = (
                        offset[i],
                        offset[i] + actions.len() * spot.num_hands(*player),
                    );
                    sp[*player] = merge(sp[*player], own);
                    children
                }
                NodeKind::Chance { children, .. } => children,
                _ => &[],
            };
            for &c in children {
                for p in 0..2 {
                    sp[p] = merge(sp[p], span[c][p]);
                }
            }
            span[i] = sp;
        }
        Solver {
            spot,
            algorithm,
            offset,
            span,
            regrets: [vec![0.0; len[0]], vec![0.0; len[1]]],
            strategy_sum: [vec![0.0; len[0]], vec![0.0; len[1]]],
            iteration: 0,
        }
    }

    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    /// Bytes used by regrets and strategy sums.
    pub fn memory_bytes(&self) -> usize {
        4 * (self.regrets[0].len() + self.regrets[1].len()) * 2
    }

    /// Cumulative regrets of both players (for tests and diagnostics).
    pub fn regrets(&self) -> [&[f32]; 2] {
        [&self.regrets[0], &self.regrets[1]]
    }

    /// One iteration: a regret update for each player in turn.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        let t = self.iteration as f64;
        let weight = match self.algorithm {
            Algorithm::CfrPlus => t as f32,
            Algorithm::Discounted { .. } => 1.0,
        };
        let ctx = Ctx {
            spot: self.spot,
            offset: &self.offset,
            span: &self.span,
            algorithm: self.algorithm,
            weight,
        };
        let [r0, r1] = &mut self.regrets;
        let [s0, s1] = &mut self.strategy_sum;
        let mut view = View {
            regrets: [r0.as_mut_slice(), r1.as_mut_slice()],
            sums: [s0.as_mut_slice(), s1.as_mut_slice()],
            base: [0, 0],
        };
        for trav in 0..2 {
            let reach = self.spot.weights[trav].clone();
            let opp = self.spot.weights[1 - trav].clone();
            train(&ctx, &mut view, self.spot.tree.root(), trav, &reach, &opp);
        }
        if let Algorithm::Discounted { alpha, beta, gamma } = self.algorithm {
            let pos = (t.powf(alpha) / (t.powf(alpha) + 1.0)) as f32;
            let neg = (t.powf(beta) / (t.powf(beta) + 1.0)) as f32;
            let avg = ((t / (t + 1.0)).powf(gamma)) as f32;
            for p in 0..2 {
                self.regrets[p]
                    .par_iter_mut()
                    .for_each(|r| *r *= if *r > 0.0 { pos } else { neg });
                self.strategy_sum[p].par_iter_mut().for_each(|s| *s *= avg);
            }
        }
    }

    /// Average strategy at an action node: `actions x hands`, action-major.
    pub fn average_strategy(&self, node: usize) -> Vec<f32> {
        let NodeKind::Action {
            player, children, ..
        } = &self.spot.tree.nodes[node].kind
        else {
            panic!("node {node} is not an action node");
        };
        let m = self.spot.num_hands(*player);
        let off = self.offset[node];
        let len = children.len() * m;
        let mut out = vec![0f32; len];
        normalize(&self.strategy_sum[*player][off..off + len], m, &mut out);
        out
    }

    fn evaluate(&self, node: usize, trav: usize, opp_reach: &[f32], mode: Mode) -> Vec<f64> {
        let spot = self.spot;
        let n = spot.num_hands(trav);
        let opp = 1 - trav;
        let info = &spot.tree.nodes[node];
        match &info.kind {
            NodeKind::Fold { folder } => {
                let mut out = vec![0f32; n];
                spot.fold_values(trav, *folder, info.invested, opp_reach, &mut out);
                out.into_iter().map(f64::from).collect()
            }
            NodeKind::Showdown => {
                let mut out = vec![0f32; n];
                spot.showdown_values(trav, info.board, info.invested, opp_reach, &mut out);
                out.into_iter().map(f64::from).collect()
            }
            NodeKind::Chance {
                cards,
                children,
                members,
            } => {
                let values: Vec<Vec<f64>> = children
                    .par_iter()
                    .zip(cards.par_iter())
                    .map(|(&child, &card)| {
                        let c = card.index() as usize;
                        let mut or = opp_reach.to_vec();
                        for &o in &spot.with_card[opp][c] {
                            or[o as usize] = 0.0;
                        }
                        let mut v = self.evaluate(child, trav, &or, mode);
                        for &h in &spot.with_card[trav][c] {
                            v[h as usize] = 0.0;
                        }
                        v
                    })
                    .collect();
                let p = spot.deal_probability(info.board) as f64;
                let mut out = vec![0f64; n];
                for (v, class) in values.iter().zip(members) {
                    accumulate_class(spot, trav, class, p, v, &mut out);
                }
                out
            }
            NodeKind::Action {
                player, children, ..
            } => {
                let sigma = self.average_strategy(node);
                if *player == trav {
                    let start = if mode == Mode::BestResponse {
                        f64::NEG_INFINITY
                    } else {
                        0.0
                    };
                    let mut out = vec![start; n];
                    for (a, &child) in children.iter().enumerate() {
                        let v = self.evaluate(child, trav, opp_reach, mode);
                        for h in 0..n {
                            if mode == Mode::BestResponse {
                                out[h] = out[h].max(v[h]);
                            } else {
                                out[h] += sigma[a * n + h] as f64 * v[h];
                            }
                        }
                    }
                    out
                } else {
                    let m = spot.num_hands(opp);
                    let mut out = vec![0f64; n];
                    for (a, &child) in children.iter().enumerate() {
                        let reach: Vec<f32> = opp_reach
                            .iter()
                            .zip(&sigma[a * m..(a + 1) * m])
                            .map(|(r, s)| r * s)
                            .collect();
                        let v = self.evaluate(child, trav, &reach, mode);
                        for (o, x) in out.iter_mut().zip(&v) {
                            *o += x;
                        }
                    }
                    out
                }
            }
        }
    }

    fn root_value(&self, player: usize, mode: Mode) -> f64 {
        let v = self.evaluate(
            self.spot.tree.root(),
            player,
            &self.spot.weights[1 - player],
            mode,
        );
        let total: f64 = self.spot.weights[player]
            .iter()
            .zip(&v)
            .map(|(&w, &x)| w as f64 * x)
            .sum();
        total / self.spot.norm
    }

    /// Expected payoff to `player` (net chips, counting the starting pot as half
    /// theirs) when both play their average strategies.
    pub fn expected_value(&self, player: usize) -> f64 {
        self.root_value(player, Mode::Average)
    }

    /// Value to `player` of best-responding to the opponent's average strategy.
    pub fn best_response_value(&self, player: usize) -> f64 {
        self.root_value(player, Mode::BestResponse)
    }

    /// Average gain of a best responder over both seats, in chips. Zero exactly
    /// at an equilibrium of this tree.
    pub fn exploitability(&self) -> f64 {
        (self.best_response_value(0) + self.best_response_value(1)) / 2.0
    }

    /// Exploitability as a percentage of the starting pot.
    pub fn exploitability_pct(&self) -> f64 {
        100.0 * self.exploitability() / self.spot.tree.pot as f64
    }

    /// Range-weighted frequency of each action at `node` (for its acting player),
    /// given the actor's reach at that node.
    pub fn action_frequencies(&self, node: usize, reach: &[f32]) -> Vec<(Action, f64)> {
        let NodeKind::Action { actions, .. } = &self.spot.tree.nodes[node].kind else {
            panic!("not an action node");
        };
        let sigma = self.average_strategy(node);
        let n = reach.len();
        let total: f64 = reach.iter().map(|&r| r as f64).sum();
        actions
            .iter()
            .enumerate()
            .map(|(a, &act)| {
                let f: f64 = (0..n)
                    .map(|h| reach[h] as f64 * sigma[a * n + h] as f64)
                    .sum();
                (act, if total > 0.0 { f / total } else { 0.0 })
            })
            .collect()
    }
}
