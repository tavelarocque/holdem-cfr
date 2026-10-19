//! Exact best response, expected value and exploitability on a compiled [`Tree`].
//!
//! A best responder for player `i` knows the opponent's strategy but not the
//! opponent's private cards. So at each of `i`'s information sets it must pick one
//! action for every history in the set at once, choosing the action that maximizes
//! the sum over histories of (chance x opponent reach) x (value after the action).
//! Values below an information set depend only on deeper information sets
//! (perfect recall), so a memoized recursion settles every choice exactly.

use crate::tree::{Node, Tree};

/// Expected payoff to player 0 when both players follow `strategy`.
pub fn expected_value(tree: &Tree, strategy: &[f64]) -> f64 {
    fn ev(tree: &Tree, s: &[f64], n: usize) -> f64 {
        match &tree.nodes[n] {
            Node::Terminal { payoff } => *payoff,
            Node::Chance { children, probs } => children
                .iter()
                .zip(probs)
                .map(|(&c, &p)| p * ev(tree, s, c))
                .sum(),
            Node::Decision {
                infoset, children, ..
            } => {
                let off = tree.infosets[*infoset].offset;
                children
                    .iter()
                    .enumerate()
                    .map(|(a, &c)| s[off + a] * ev(tree, s, c))
                    .sum()
            }
        }
    }
    ev(tree, strategy, tree.root())
}

/// Value to `player` of best-responding to the opponent's part of `strategy`.
pub fn best_response_value(tree: &Tree, strategy: &[f64], player: usize) -> f64 {
    let mut br = BestResponse::new(tree, strategy, player);
    br.value(tree.root())
}

/// The pure best-response action at every information set of `player`
/// (entries for the opponent's information sets are `None`).
pub fn best_response_actions(tree: &Tree, strategy: &[f64], player: usize) -> Vec<Option<usize>> {
    let mut br = BestResponse::new(tree, strategy, player);
    br.value(tree.root());
    for i in 0..tree.infosets.len() {
        if tree.infosets[i].player == player {
            br.best_action(i);
        }
    }
    br.best
}

/// Average over both players of how much a best response gains against `strategy`
/// relative to the game value. Zero exactly at a Nash equilibrium.
///
/// Because the game is zero-sum, BR_0 + BR_1 >= 0 with equality only at
/// equilibrium; the game value cancels out of the sum.
pub fn exploitability(tree: &Tree, strategy: &[f64]) -> f64 {
    (best_response_value(tree, strategy, 0) + best_response_value(tree, strategy, 1)) / 2.0
}

struct BestResponse<'a> {
    tree: &'a Tree,
    strategy: &'a [f64],
    player: usize,
    /// Chance reach times opponent reach, per node.
    reach: Vec<f64>,
    value: Vec<Option<f64>>,
    best: Vec<Option<usize>>,
}

impl<'a> BestResponse<'a> {
    fn new(tree: &'a Tree, strategy: &'a [f64], player: usize) -> Self {
        let mut reach = vec![0.0; tree.nodes.len()];
        reach[tree.root()] = 1.0;
        // Pre-order storage means a forward sweep visits parents before children.
        for n in 0..tree.nodes.len() {
            let r = reach[n];
            match &tree.nodes[n] {
                Node::Terminal { .. } => {}
                Node::Chance { children, probs } => {
                    for (&c, &p) in children.iter().zip(probs) {
                        reach[c] = r * p;
                    }
                }
                Node::Decision {
                    player: actor,
                    infoset,
                    children,
                } => {
                    let off = tree.infosets[*infoset].offset;
                    for (a, &c) in children.iter().enumerate() {
                        reach[c] = if *actor == player {
                            r
                        } else {
                            r * strategy[off + a]
                        };
                    }
                }
            }
        }
        BestResponse {
            tree,
            strategy,
            player,
            reach,
            value: vec![None; tree.nodes.len()],
            best: vec![None; tree.infosets.len()],
        }
    }

    fn utility(&self, payoff: f64) -> f64 {
        if self.player == 0 { payoff } else { -payoff }
    }

    fn value(&mut self, n: usize) -> f64 {
        if let Some(v) = self.value[n] {
            return v;
        }
        let tree = self.tree;
        let v = match &tree.nodes[n] {
            Node::Terminal { payoff } => self.utility(*payoff),
            Node::Chance { children, probs } => children
                .iter()
                .zip(probs)
                .map(|(&c, &p)| p * self.value(c))
                .sum(),
            Node::Decision {
                player,
                infoset,
                children,
            } => {
                if *player == self.player {
                    let a = self.best_action(*infoset);
                    self.value(children[a])
                } else {
                    let off = tree.infosets[*infoset].offset;
                    children
                        .iter()
                        .enumerate()
                        .map(|(a, &c)| self.strategy[off + a] * self.value(c))
                        .sum()
                }
            }
        };
        self.value[n] = Some(v);
        v
    }

    fn best_action(&mut self, infoset: usize) -> usize {
        if let Some(a) = self.best[infoset] {
            return a;
        }
        let tree = self.tree;
        let info = &tree.infosets[infoset];
        let mut totals = vec![0.0; info.num_actions()];
        for &h in &info.nodes {
            let Node::Decision { children, .. } = &tree.nodes[h] else {
                unreachable!("infoset member is not a decision node");
            };
            let w = self.reach[h];
            for (a, &c) in children.iter().enumerate() {
                totals[a] += w * self.value(c);
            }
        }
        let mut best = 0;
        for a in 1..totals.len() {
            if totals[a] > totals[best] {
                best = a;
            }
        }
        self.best[infoset] = Some(best);
        best
    }
}
