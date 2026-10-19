//! External-sampling Monte Carlo CFR (Lanctot et al. 2009) on an abstract game,
//! multithreaded with lock-free atomic updates.
//!
//! A [`SampledGame`] is a *public betting tree* plus a way to deal cards. Each
//! iteration samples one full deal (every card, up front), then walks the tree
//! once per player. At the traverser's nodes every action is explored; at the
//! opponent's nodes one action is sampled from the current strategy. Regrets are
//! stored per (node, bucket), where the bucket is whatever the game says the
//! acting player observes on that street: their exact cards in a small game, or
//! a card-abstraction bucket in hold'em.
//!
//! **Why this is unbiased.** With chance and the opponent sampled from their
//! true distributions, the sampled value of each traverser action is an unbiased
//! estimate of its counterfactual value scaled by the traverser-independent
//! sampling probability, so no importance weights are needed. Lanctot et al.
//! prove the regret bound still holds with high probability.
//!
//! **Threads.** Many iterations run at once on rayon threads and update shared
//! arrays with atomic compare-and-swap adds. Two threads occasionally interleave
//! on the same entry; the update is never lost (CAS retries), only applied in a
//! different order, which is the standard "hogwild" trade made by Pluribus and
//! most large MCCFR implementations. Multi-threaded runs are therefore not
//! bit-reproducible; single-threaded runs with a fixed seed are.
//!
//! **Precision.** Regrets and strategy sums are f64. With f32, a strategy sum
//! that has grown to 2^24 (about 17 million) can no longer absorb an increment
//! below 1: adding a probability of 0.3 rounds to 0 or 1, and the average
//! strategy silently degrades into a count of argmax picks. A hold'em blueprint
//! reaches that magnitude at busy preflop nodes within a billion iterations.
//! f64 pushes the same threshold beyond 10^15.

use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;

/// A node of the public (card-independent) betting tree.
#[derive(Clone, Debug)]
pub enum BettingNode {
    Action {
        player: usize,
        street: usize,
        children: Vec<usize>,
    },
    Terminal,
}

/// SplitMix64: tiny, fast, good enough for sampling.
#[derive(Clone)]
pub struct SplitMix(pub u64);

impl SplitMix {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in 0..n.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A game MCCFR can train on.
pub trait SampledGame: Sync {
    /// Everything chance decides in one hand (all cards).
    type Deal;

    /// The public betting tree; node 0 is the root.
    fn nodes(&self) -> &[BettingNode];
    /// Number of buckets a player can be in on `street`.
    fn num_buckets(&self, street: usize) -> usize;
    fn sample(&self, rng: &mut SplitMix) -> Self::Deal;
    /// What `player` observes on `street` for this deal.
    fn bucket(&self, deal: &Self::Deal, player: usize, street: usize) -> usize;
    /// Payoff to player 0 at a terminal node.
    fn utility(&self, deal: &Self::Deal, node: usize) -> f64;
}

/// How iterations are weighted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weighting {
    /// Every iteration counts equally (vanilla MCCFR).
    Uniform,
    /// Iteration t's regrets and strategy contributions are weighted by t
    /// (Linear CFR, as in Pluribus).
    Linear,
}

pub struct Mccfr<'g, G: SampledGame> {
    game: &'g G,
    weighting: Weighting,
    offset: Vec<usize>,
    regrets: Vec<AtomicU64>,
    strategy_sum: Vec<AtomicU64>,
    iterations: AtomicU64,
}

fn load(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::Relaxed))
}

fn add(a: &AtomicU64, x: f64) {
    let mut cur = a.load(Ordering::Relaxed);
    loop {
        let new = (f64::from_bits(cur) + x).to_bits();
        match a.compare_exchange_weak(cur, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(v) => cur = v,
        }
    }
}

impl<'g, G: SampledGame> Mccfr<'g, G> {
    pub fn new(game: &'g G, weighting: Weighting) -> Self {
        assert!(
            game.nodes().iter().all(|n| match n {
                BettingNode::Action { children, .. } => children.len() <= 16,
                BettingNode::Terminal => true,
            }),
            "at most 16 actions per node"
        );
        let mut offset = vec![usize::MAX; game.nodes().len()];
        let mut len = 0;
        for (i, n) in game.nodes().iter().enumerate() {
            if let BettingNode::Action {
                street, children, ..
            } = n
            {
                offset[i] = len;
                len += children.len() * game.num_buckets(*street);
            }
        }
        let zeros = |n: usize| (0..n).map(|_| AtomicU64::new(0)).collect::<Vec<_>>();
        Mccfr {
            game,
            weighting,
            offset,
            regrets: zeros(len),
            strategy_sum: zeros(len),
            iterations: AtomicU64::new(0),
        }
    }

    /// Number of (node, bucket, action) entries.
    pub fn len(&self) -> usize {
        self.regrets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regrets.is_empty()
    }

    pub fn iterations(&self) -> u64 {
        self.iterations.load(Ordering::Relaxed)
    }

    /// Runs `n` iterations in parallel. Iteration `k` uses seed `seed + k`.
    pub fn run(&self, n: u64, seed: u64) {
        let start = self.iterations.fetch_add(n, Ordering::Relaxed);
        (start..start + n).into_par_iter().for_each(|t| {
            let mut rng = SplitMix(seed.wrapping_add(t.wrapping_mul(0x2545_F491_4F6C_DD1D)));
            let w = match self.weighting {
                Weighting::Uniform => 1.0,
                Weighting::Linear => (t + 1) as f64,
            };
            for trav in 0..2 {
                let deal = self.game.sample(&mut rng);
                self.traverse(0, &deal, trav, w, &mut rng);
            }
        });
    }

    /// Multiplies every regret by `regret_factor` and every strategy sum by
    /// `strategy_factor`. Calling this with T / (T + 1) after the T-th batch of
    /// uniformly weighted iterations is Linear CFR with bounded magnitudes, which
    /// keeps magnitudes bounded over billions of iterations (Pluribus does this).
    pub fn discount(&self, regret_factor: f64, strategy_factor: f64) {
        self.regrets
            .par_iter()
            .for_each(|a| a.store((load(a) * regret_factor).to_bits(), Ordering::Relaxed));
        self.strategy_sum
            .par_iter()
            .for_each(|a| a.store((load(a) * strategy_factor).to_bits(), Ordering::Relaxed));
    }

    /// Zeroes the strategy sums and keeps the regrets. The average strategy is
    /// then rebuilt from the current, well-trained regrets onward, discarding
    /// early iterations (or sums damaged by an older f32 trainer).
    pub fn reset_average(&self) {
        self.strategy_sum
            .par_iter()
            .for_each(|a| a.store(0f64.to_bits(), Ordering::Relaxed));
    }

    /// Copies out regrets and strategy sums (for checkpoints).
    pub fn export(&self) -> (Vec<f64>, Vec<f64>) {
        (
            self.regrets.iter().map(load).collect(),
            self.strategy_sum.iter().map(load).collect(),
        )
    }

    /// Restores a checkpoint written by [`Mccfr::export`].
    pub fn import(&self, regrets: &[f64], strategy_sum: &[f64], iterations: u64) {
        assert_eq!(
            regrets.len(),
            self.regrets.len(),
            "checkpoint does not match the tree"
        );
        assert_eq!(strategy_sum.len(), self.strategy_sum.len());
        for (a, &x) in self.regrets.iter().zip(regrets) {
            a.store(x.to_bits(), Ordering::Relaxed);
        }
        for (a, &x) in self.strategy_sum.iter().zip(strategy_sum) {
            a.store(x.to_bits(), Ordering::Relaxed);
        }
        self.iterations.store(iterations, Ordering::Relaxed);
    }

    /// Regret-matching strategy at (node, bucket).
    fn current(&self, node: usize, bucket: usize, actions: usize, out: &mut [f64]) {
        let base = self.offset[node] + bucket * actions;
        let mut total = 0.0;
        for (a, o) in out.iter_mut().enumerate().take(actions) {
            *o = load(&self.regrets[base + a]).max(0.0);
            total += *o;
        }
        for o in out.iter_mut().take(actions) {
            *o = if total > 0.0 {
                *o / total
            } else {
                1.0 / actions as f64
            };
        }
    }

    /// Sampled counterfactual value for the traverser.
    fn traverse(
        &self,
        node: usize,
        deal: &G::Deal,
        trav: usize,
        w: f64,
        rng: &mut SplitMix,
    ) -> f64 {
        match &self.game.nodes()[node] {
            BettingNode::Terminal => {
                let u = self.game.utility(deal, node);
                if trav == 0 { u } else { -u }
            }
            BettingNode::Action {
                player,
                street,
                children,
            } => {
                let k = children.len();
                let bucket = self.game.bucket(deal, *player, *street);
                let mut sigma = [0f64; 16];
                self.current(node, bucket, k, &mut sigma);
                let base = self.offset[node] + bucket * k;
                if *player == trav {
                    let mut values = [0f64; 16];
                    let mut v = 0.0;
                    for (a, &child) in children.iter().enumerate() {
                        values[a] = self.traverse(child, deal, trav, w, rng);
                        v += sigma[a] * values[a];
                    }
                    let slots = &self.regrets[base..base + k];
                    for (slot, &value) in slots.iter().zip(&values) {
                        add(slot, w * (value - v));
                    }
                    v
                } else {
                    // Average strategy is accumulated at the opponent's nodes, where
                    // they are reached with their own sampling probability.
                    let slots = &self.strategy_sum[base..base + k];
                    for (slot, &p) in slots.iter().zip(&sigma) {
                        add(slot, w * p);
                    }
                    let x = rng.next_f64();
                    let mut acc = 0.0;
                    let mut pick = k - 1;
                    for (a, &s) in sigma.iter().enumerate().take(k) {
                        acc += s;
                        if x < acc {
                            pick = a;
                            break;
                        }
                    }
                    self.traverse(children[pick], deal, trav, w, rng)
                }
            }
        }
    }

    /// Average strategy at (node, bucket).
    pub fn average_strategy(&self, node: usize, bucket: usize) -> Vec<f64> {
        let BettingNode::Action { children, .. } = &self.game.nodes()[node] else {
            panic!("node {node} is not an action node");
        };
        let k = children.len();
        let base = self.offset[node] + bucket * k;
        let sums: Vec<f64> = (0..k).map(|a| load(&self.strategy_sum[base + a])).collect();
        let total: f64 = sums.iter().sum();
        if total > 0.0 {
            sums.iter().map(|s| s / total).collect()
        } else {
            vec![1.0 / k as f64; k]
        }
    }

    /// Current (regret-matching) strategy at (node, bucket).
    pub fn current_strategy(&self, node: usize, bucket: usize) -> Vec<f64> {
        let BettingNode::Action { children, .. } = &self.game.nodes()[node] else {
            panic!("node {node} is not an action node");
        };
        let mut s = [0f64; 16];
        self.current(node, bucket, children.len(), &mut s);
        s[..children.len()].to_vec()
    }
}

#[cfg(test)]
mod precision {
    use super::*;

    /// The failure that motivated f64: once a sum passes 2^24, f32 cannot
    /// absorb a probability-sized increment at all.
    #[test]
    fn large_sums_still_absorb_small_increments() {
        let big: f64 = 16_777_216.0; // 2^24
        let mut f = big as f32;
        for _ in 0..1000 {
            f += 0.3f32;
        }
        assert_eq!(f, big as f32, "f32 drops every increment");
        let a = AtomicU64::new(big.to_bits());
        for _ in 0..1000 {
            add(&a, 0.3);
        }
        assert!((load(&a) - big - 300.0).abs() < 1e-6);
    }
}
