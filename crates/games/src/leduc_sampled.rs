//! Leduc hold'em as a [`SampledGame`], so the exact MCCFR code that trains the
//! no-limit blueprint can be validated on a game whose exploitability we can
//! compute exactly.

use std::collections::HashMap;

use engine::{BettingNode, Game, NodeType, SampledGame, SplitMix};

use crate::leduc::{Leduc, LeducAction, LeducState};

pub struct LeducSampled {
    nodes: Vec<BettingNode>,
    /// Betting history of each node, as `(round 0 actions, round 1 actions)`.
    histories: Vec<[Vec<LeducAction>; 2]>,
    /// Node id by history key `"h0/h1"` (e.g. `"rc/cr"`).
    by_key: HashMap<String, usize>,
}

/// Cards for one hand: private cards and the board card.
pub type LeducDeal = [u8; 3];

fn key(h: &[Vec<LeducAction>; 2]) -> String {
    let s = |v: &Vec<LeducAction>| -> String {
        v.iter()
            .map(|a| match a {
                LeducAction::Fold => 'f',
                LeducAction::Call => 'c',
                LeducAction::Raise => 'r',
            })
            .collect()
    };
    format!("{}/{}", s(&h[0]), s(&h[1]))
}

impl Default for LeducSampled {
    fn default() -> Self {
        Self::new()
    }
}

impl LeducSampled {
    pub fn new() -> LeducSampled {
        let mut me = LeducSampled {
            nodes: Vec::new(),
            histories: Vec::new(),
            by_key: HashMap::new(),
        };
        // Betting does not depend on the cards, so walk it with dummy cards.
        let mut root = Leduc.root();
        root.cards = Some([0, 2]);
        me.build(&root);
        me
    }

    fn build(&mut self, state: &LeducState) -> usize {
        let mut s = state.clone();
        if Leduc.node_type(&s) == NodeType::Chance {
            s.board = Some(4); // dummy board card; betting continues
        }
        let id = self.nodes.len();
        self.nodes.push(BettingNode::Terminal);
        self.histories.push(s.history.clone());
        self.by_key.insert(key(&s.history), id);
        if let NodeType::Decision(player) = Leduc.node_type(&s) {
            let children = Leduc
                .actions(&s)
                .into_iter()
                .map(|a| self.build(&Leduc.apply(&s, a)))
                .collect();
            self.nodes[id] = BettingNode::Action {
                player,
                street: s.round,
                children,
            };
        }
        id
    }

    /// The node reached by a history key like `"rc/cr"`.
    pub fn node(&self, history_key: &str) -> Option<usize> {
        self.by_key.get(history_key).copied()
    }

    /// Bucket of a player holding `card` with `board` (None before the flop).
    pub fn bucket_of(card: u8, board: Option<u8>) -> usize {
        match board {
            None => card as usize,
            Some(b) => card as usize * 6 + b as usize,
        }
    }
}

impl SampledGame for LeducSampled {
    type Deal = LeducDeal;

    fn nodes(&self) -> &[BettingNode] {
        &self.nodes
    }

    fn num_buckets(&self, street: usize) -> usize {
        if street == 0 { 6 } else { 36 }
    }

    fn sample(&self, rng: &mut SplitMix) -> LeducDeal {
        let a = rng.below(6) as u8;
        let mut b = rng.below(5) as u8;
        if b >= a {
            b += 1;
        }
        let mut c = rng.below(4) as u8;
        for x in [a.min(b), a.max(b)] {
            if c >= x {
                c += 1;
            }
        }
        [a, b, c]
    }

    fn bucket(&self, deal: &LeducDeal, player: usize, street: usize) -> usize {
        Self::bucket_of(deal[player], (street == 1).then_some(deal[2]))
    }

    fn utility(&self, deal: &LeducDeal, node: usize) -> f64 {
        let mut s = Leduc.root();
        s.cards = Some([deal[0], deal[1]]);
        for round in 0..2 {
            if Leduc.node_type(&s) == NodeType::Chance {
                s.board = Some(deal[2]);
            }
            for &a in &self.histories[node][round] {
                s = Leduc.apply(&s, a);
            }
        }
        Leduc.payoff(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_deals_are_distinct_and_uniform() {
        let g = LeducSampled::new();
        let mut rng = SplitMix(7);
        let mut counts = [[0u32; 6]; 3];
        for _ in 0..60_000 {
            let d = g.sample(&mut rng);
            assert!(d[0] != d[1] && d[0] != d[2] && d[1] != d[2]);
            for p in 0..3 {
                counts[p][d[p] as usize] += 1;
            }
        }
        for row in counts {
            assert!(row.iter().all(|&c| (9_000..11_000).contains(&c)), "{row:?}");
        }
    }

    #[test]
    fn betting_tree_matches_the_game() {
        let g = LeducSampled::new();
        let actions: usize = g
            .nodes()
            .iter()
            .filter(|n| matches!(n, BettingNode::Action { .. }))
            .count();
        // 6 decision histories in round 0, plus 5 continuing lines x 6 in round 1.
        assert_eq!(actions, 6 + 5 * 6);
        assert!(g.node("rc/cr").is_some());
    }
}
