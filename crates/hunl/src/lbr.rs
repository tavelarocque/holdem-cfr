//! Local best response (Lisý and Bowling, 2017): a lower bound on how
//! exploitable the blueprint is.
//!
//! Exact exploitability needs a best response computed over the whole game,
//! which is out of reach for no-limit hold'em. LBR is a *strategy* instead, so
//! whatever it wins against the blueprint is a valid lower bound on the
//! blueprint's exploitability, however crude LBR's own decisions are.
//!
//! At each of its decisions, LBR:
//!
//! 1. **Tracks the opponent's range** exactly. It starts from every hand not
//!    blocked by its own cards and the board, and after each blueprint action
//!    multiplies each hand's weight by the probability the blueprint plays that
//!    action with that hand. This is Bayes' rule with the blueprint's real
//!    strategy, which LBR is allowed to know.
//! 2. **Computes its equity against each opponent hand**, assuming the hand is
//!    checked down: exactly on the turn and river, over sampled runouts on the
//!    flop.
//! 3. **Scores every action** with a one-step lookahead:
//!    - fold: lose what is in;
//!    - check or call: win or lose the matched pot at showdown, weighted by equity;
//!    - bet or raise to x: each opponent hand folds with the blueprint's own
//!      fold probability at the resulting node, and otherwise calls and the hand
//!      is checked down.
//!
//!    It then plays the best-scoring action.
//!
//! Preflop LBR always checks or calls (the usual LBR variant), because a
//! one-step lookahead is a poor guide with four streets to come.
//!
//! LBR here chooses from the blueprint's own abstract actions, so it measures
//! leaks the *card* abstraction and imperfect convergence create, not leaks
//! from bet sizes the blueprint never trained on.

use eval::{Card, Hand};

use engine::{BettingNode, Mccfr, SampledGame, SplitMix};

use crate::arena::{Agent, View};
use crate::blueprint::{Blueprint, CardAbstraction};
use crate::rules::Action;

/// All 1,326 two-card hands, as (higher index, lower index).
pub fn all_combos() -> Vec<[Card; 2]> {
    let mut v = Vec::with_capacity(1326);
    for a in 0..52u8 {
        for b in 0..a {
            v.push([Card::from_index(a), Card::from_index(b)]);
        }
    }
    v
}

fn blocked(h: &[Card; 2], dead: &[Card]) -> bool {
    dead.contains(&h[0]) || dead.contains(&h[1])
}

/// LBR's equity against each combo (wins + ties / 2), with the remaining board
/// cards dealt exactly (turn, river) or sampled (`flop_samples` runouts on the
/// flop). Blocked combos get 0.
pub fn equities(
    hole: [Card; 2],
    board: &[Card],
    combos: &[[Card; 2]],
    flop_samples: usize,
    rng: &mut SplitMix,
) -> Vec<f64> {
    let dead: Vec<Card> = hole.iter().chain(board).copied().collect();
    let live: Vec<Card> = Card::deck().filter(|c| !dead.contains(c)).collect();
    let runouts: Vec<Vec<Card>> = match board.len() {
        5 => vec![Vec::new()],
        4 => live.iter().map(|&c| vec![c]).collect(),
        3 => (0..flop_samples)
            .map(|_| {
                let i = rng.below(live.len());
                let mut j = rng.below(live.len() - 1);
                if j >= i {
                    j += 1;
                }
                vec![live[i], live[j]]
            })
            .collect(),
        n => panic!("equities need a flop, turn or river board, got {n} cards"),
    };
    let mut score = vec![0f64; combos.len()];
    let mut count = vec![0u32; combos.len()];
    for extra in &runouts {
        let full = Hand::from_cards(board).with(extra);
        let me = full.with(&hole).rank();
        for (i, h) in combos.iter().enumerate() {
            if blocked(h, &dead) || blocked(h, extra) {
                continue;
            }
            let them = full.with(h).rank();
            score[i] += match me.cmp(&them) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Less => 0.0,
            };
            count[i] += 1;
        }
    }
    score
        .iter()
        .zip(&count)
        .map(|(s, &n)| if n > 0 { s / n as f64 } else { 0.0 })
        .collect()
}

/// The local best responder.
pub struct LbrAgent<'a, A: CardAbstraction> {
    pub bp: &'a Blueprint<A>,
    pub m: &'a Mccfr<'a, Blueprint<A>>,
    /// Sampled runouts for flop equities.
    pub flop_samples: usize,
    combos: Vec<[Card; 2]>,
    cache: Option<(Vec<Card>, Vec<f64>)>,
}

impl<'a, A: CardAbstraction> LbrAgent<'a, A> {
    pub fn new(bp: &'a Blueprint<A>, m: &'a Mccfr<'a, Blueprint<A>>, flop_samples: usize) -> Self {
        LbrAgent {
            bp,
            m,
            flop_samples,
            combos: all_combos(),
            cache: None,
        }
    }

    /// The opponent's range at `view.node`: weight per combo (unnormalized),
    /// from replaying the hand's actions through the blueprint's strategy.
    pub fn range(&self, v: &View) -> Vec<f64> {
        let mut dead: Vec<Card> = v.hole.to_vec();
        dead.extend_from_slice(v.board);
        let mut w: Vec<f64> = self
            .combos
            .iter()
            .map(|h| if blocked(h, &dead) { 0.0 } else { 1.0 })
            .collect();
        let me = v.seat;
        let mut node = 0;
        for street in 0..4 {
            let board = &v.board[..[0, 3, 4, 5][street].min(v.board.len())];
            for a in &v.state.history[street] {
                let BettingNode::Action {
                    player, children, ..
                } = &self.bp.nodes()[node]
                else {
                    unreachable!("history continues past a terminal")
                };
                let i = self
                    .bp
                    .node_actions(node)
                    .iter()
                    .position(|x| x == a)
                    .expect("history action is on the tree");
                if *player != me {
                    for (wh, h) in w.iter_mut().zip(&self.combos) {
                        if *wh > 0.0 {
                            let b = self.bp.cards.bucket(*h, board);
                            *wh *= self.m.average_strategy(node, b)[i];
                        }
                    }
                }
                node = children[i];
            }
        }
        debug_assert_eq!(node, v.node, "replayed history must reach the current node");
        w
    }

    fn equities_cached(&mut self, v: &View, rng: &mut SplitMix) -> Vec<f64> {
        let key: Vec<Card> = v.hole.iter().chain(v.board).copied().collect();
        if let Some((k, e)) = &self.cache
            && *k == key
        {
            return e.clone();
        }
        let e = equities(v.hole, v.board, &self.combos, self.flop_samples, rng);
        self.cache = Some((key, e.clone()));
        e
    }

    /// Expected chips (net, for LBR) of each available action under the LBR
    /// assumptions, given the range and per-combo equities.
    pub fn action_values(&self, v: &View, range: &[f64], eq: &[f64]) -> Vec<f64> {
        let me = v.seat;
        let c_me = v.state.contributed[me] as f64;
        let c_opp = v.state.contributed[1 - me] as f64;
        let total: f64 = range.iter().sum();
        let showdown = |stake: f64, weights: &dyn Fn(usize) -> f64| -> (f64, f64) {
            // (weighted value of checking down for `stake` each, weight)
            let mut val = 0.0;
            let mut wt = 0.0;
            for (i, &w) in range.iter().enumerate() {
                if w > 0.0 {
                    let x = w * weights(i);
                    val += x * (2.0 * eq[i] - 1.0) * stake;
                    wt += x;
                }
            }
            (val, wt)
        };
        let BettingNode::Action { children, .. } = &self.bp.nodes()[v.node] else {
            unreachable!()
        };
        v.actions
            .iter()
            .enumerate()
            .map(|(k, a)| match a {
                Action::Fold => -c_me,
                Action::Check | Action::Call => showdown(c_opp, &|_| 1.0).0 / total,
                Action::RaiseTo(x) => {
                    let child = children[k];
                    let fold_idx = self
                        .bp
                        .node_actions(child)
                        .iter()
                        .position(|a| matches!(a, Action::Fold));
                    let street = v.state.street;
                    let fold_p: Vec<f64> = match fold_idx {
                        Some(f) => self
                            .combos
                            .iter()
                            .zip(range)
                            .map(|(h, &w)| {
                                if w > 0.0 {
                                    let b = self.bp.cards.bucket(*h, v.board);
                                    debug_assert_eq!(street, self.bp.info(child).street);
                                    self.m.average_strategy(child, b)[f]
                                } else {
                                    0.0
                                }
                            })
                            .collect(),
                        None => vec![0.0; range.len()],
                    };
                    let folds: f64 = range.iter().zip(&fold_p).map(|(w, f)| w * f).sum();
                    let (called, _) = showdown(*x as f64, &|i| 1.0 - fold_p[i]);
                    (folds * c_opp + called) / total
                }
            })
            .collect()
    }
}

impl<A: CardAbstraction> Agent for LbrAgent<'_, A> {
    fn name(&self) -> String {
        "local best response".into()
    }

    fn act(&mut self, v: &View, rng: &mut SplitMix) -> usize {
        let passive = v
            .actions
            .iter()
            .position(|a| matches!(a, Action::Check | Action::Call))
            .expect("check or call is always available");
        if v.state.street == 0 {
            return passive;
        }
        let range = self.range(v);
        let eq = self.equities_cached(v, rng);
        let values = self.action_values(v, &range, &eq);
        let mut best = passive;
        for (k, &x) in values.iter().enumerate() {
            if x > values[best] + 1e-9 {
                best = k;
            }
        }
        best
    }
}
