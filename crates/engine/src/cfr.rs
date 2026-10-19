//! Full-traversal CFR and its variants on a compiled [`Tree`].
//!
//! All variants share one traversal and differ only in how instantaneous regrets
//! are folded into the cumulative regrets and how the average strategy is weighted:
//!
//! | variant    | regret update                          | average weight     |
//! |------------|----------------------------------------|--------------------|
//! | Vanilla    | R += r                                 | 1                  |
//! | CFR+       | R = max(R + r, 0)                      | t                  |
//! | Discounted | R += r, then R>0 *= t^a/(t^a+1),       | sum *= (t/(t+1))^g |
//! |            | R<=0 *= t^b/(t^b+1)                    |                    |
//!
//! Updates alternate between players: player 0's regrets are updated against the
//! current strategy, then player 1's against player 0's freshly updated strategy.

use crate::regret::{normalize, regret_matching};
use crate::tree::{Node, Tree};

/// Largest action count the stack buffers in the traversal support.
pub const MAX_ACTIONS: usize = 16;

/// Which CFR variant to run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    /// Zinkevich et al. 2007, with alternating updates.
    Vanilla,
    /// Tammelin 2014: regrets floored at zero, linearly weighted average.
    CfrPlus,
    /// Brown & Sandholm 2019. The paper's recommended setting is (1.5, 0, 2).
    Discounted { alpha: f64, beta: f64, gamma: f64 },
}

impl Variant {
    /// Discounted CFR with the paper's recommended parameters.
    pub const DCFR: Variant = Variant::Discounted {
        alpha: 1.5,
        beta: 0.0,
        gamma: 2.0,
    };
    /// Linear CFR: iteration t's regrets and strategy both weighted by t.
    pub const LINEAR: Variant = Variant::Discounted {
        alpha: 1.0,
        beta: 1.0,
        gamma: 1.0,
    };
}

/// Runs CFR iterations on a tree and tracks the average strategy.
pub struct Solver<'t> {
    tree: &'t Tree,
    variant: Variant,
    regrets: Vec<f64>,
    strategy_sum: Vec<f64>,
    current: Vec<f64>,
    instant: Vec<f64>,
    iteration: u64,
}

impl<'t> Solver<'t> {
    pub fn new(tree: &'t Tree, variant: Variant) -> Self {
        assert!(
            tree.infosets.iter().all(|i| i.num_actions() <= MAX_ACTIONS),
            "an infoset has more than {MAX_ACTIONS} actions"
        );
        let n = tree.num_actions;
        Solver {
            tree,
            variant,
            regrets: vec![0.0; n],
            strategy_sum: vec![0.0; n],
            current: tree.uniform_strategy(),
            instant: vec![0.0; n],
            iteration: 0,
        }
    }

    /// One full iteration: a regret update for each player, in turn.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        let t = self.iteration as f64;
        let weight = match self.variant {
            Variant::CfrPlus => t,
            Variant::Vanilla | Variant::Discounted { .. } => 1.0,
        };

        for player in 0..2 {
            self.refresh_current_strategy();
            let mut walk = Walk {
                tree: self.tree,
                current: &self.current,
                strategy_sum: &mut self.strategy_sum,
                instant: &mut self.instant,
                player,
                weight,
            };
            walk.value(self.tree.root(), 1.0, 1.0);
            self.fold_instant_regrets(player);
        }

        if let Variant::Discounted { alpha, beta, gamma } = self.variant {
            let pos = t.powf(alpha) / (t.powf(alpha) + 1.0);
            let neg = t.powf(beta) / (t.powf(beta) + 1.0);
            let avg = (t / (t + 1.0)).powf(gamma);
            for r in &mut self.regrets {
                *r *= if *r > 0.0 { pos } else { neg };
            }
            for s in &mut self.strategy_sum {
                *s *= avg;
            }
        }
    }

    /// Runs `n` iterations.
    pub fn run(&mut self, n: u64) {
        for _ in 0..n {
            self.iterate();
        }
    }

    pub fn iteration(&self) -> u64 {
        self.iteration
    }

    pub fn regrets(&self) -> &[f64] {
        &self.regrets
    }

    /// The strategy regret matching would play next.
    pub fn current_strategy(&mut self) -> &[f64] {
        self.refresh_current_strategy();
        &self.current
    }

    /// The average strategy, which is what converges to a Nash equilibrium.
    pub fn average_strategy(&self) -> Vec<f64> {
        let mut avg = vec![0.0; self.tree.num_actions];
        for info in &self.tree.infosets {
            let r = info.range();
            normalize(&self.strategy_sum[r.clone()], &mut avg[r]);
        }
        avg
    }

    fn refresh_current_strategy(&mut self) {
        for info in &self.tree.infosets {
            let r = info.range();
            regret_matching(&self.regrets[r.clone()], &mut self.current[r]);
        }
    }

    /// Adds this traversal's instantaneous regrets into the cumulative regrets.
    /// CFR+ floors at zero *after* summing over every history in the infoset.
    fn fold_instant_regrets(&mut self, player: usize) {
        for info in self.tree.infosets.iter().filter(|i| i.player == player) {
            for k in info.range() {
                let updated = self.regrets[k] + self.instant[k];
                self.regrets[k] = match self.variant {
                    Variant::CfrPlus => updated.max(0.0),
                    _ => updated,
                };
                self.instant[k] = 0.0;
            }
        }
    }
}

/// State for one traversal on behalf of `player`.
struct Walk<'a> {
    tree: &'a Tree,
    current: &'a [f64],
    strategy_sum: &'a mut [f64],
    instant: &'a mut [f64],
    player: usize,
    weight: f64,
}

impl Walk<'_> {
    /// Returns the expected value of `node` for the traversing player under the
    /// current strategy profile. `reach_me` is the traverser's own probability of
    /// reaching the node; `reach_other` is chance's times the opponent's.
    fn value(&mut self, node: usize, reach_me: f64, reach_other: f64) -> f64 {
        if reach_me == 0.0 && reach_other == 0.0 {
            return 0.0;
        }
        match &self.tree.nodes[node] {
            Node::Terminal { payoff } => {
                if self.player == 0 {
                    *payoff
                } else {
                    -*payoff
                }
            }
            Node::Chance { children, probs } => children
                .iter()
                .zip(probs)
                .map(|(&c, &p)| p * self.value(c, reach_me, reach_other * p))
                .sum(),
            Node::Decision {
                player,
                infoset,
                children,
            } => {
                let range = self.tree.infosets[*infoset].range();
                let offset = range.start;
                if *player != self.player {
                    let mut v = 0.0;
                    for (a, &c) in children.iter().enumerate() {
                        let p = self.current[offset + a];
                        v += p * self.value(c, reach_me, reach_other * p);
                    }
                    return v;
                }

                let mut action_values = [0.0; MAX_ACTIONS];
                let mut v = 0.0;
                for (a, &c) in children.iter().enumerate() {
                    let p = self.current[offset + a];
                    action_values[a] = self.value(c, reach_me * p, reach_other);
                    v += p * action_values[a];
                }
                for (a, &av) in action_values.iter().enumerate().take(children.len()) {
                    // Counterfactual regret: weighted by everyone else's reach.
                    self.instant[offset + a] += reach_other * (av - v);
                    // Average strategy: weighted by the traverser's own reach.
                    self.strategy_sum[offset + a] +=
                        self.weight * reach_me * self.current[offset + a];
                }
                v
            }
        }
    }
}
