//! Head-to-head evaluation: agents play duplicate matches on the abstract tree.
//!
//! **Duplicate play.** Every deal is played twice with the seats swapped, so both
//! agents hold both hands from both positions. Card luck cancels almost entirely,
//! which cuts the variance of the result by a large factor at no cost in bias.
//!
//! **Units.** Results are reported in milli-big-blinds per hand (mbb/hand), the
//! standard unit in the computer-poker literature, with a 95% confidence
//! interval over duplicate pairs.
//!
//! All agents choose from the same abstract action menu, so the blueprint never
//! has to interpret an off-tree bet. Matches against opponents who bet arbitrary
//! sizes need action translation, which comes with real-time play.

use eval::{Card, Hand};
use rayon::prelude::*;

use engine::{BettingNode, Mccfr, SampledGame, SplitMix};

use crate::blueprint::{Blueprint, CardAbstraction};
use crate::rules::{Action, State};

/// What an agent sees when it must act.
pub struct View<'a> {
    pub seat: usize,
    pub hole: [Card; 2],
    pub board: &'a [Card],
    pub state: &'a State,
    /// The abstract actions available, in tree order.
    pub actions: &'a [Action],
    /// The abstract tree node (for agents that use the blueprint).
    pub node: usize,
}

pub trait Agent: Send {
    fn name(&self) -> String;
    /// Index into `view.actions`.
    fn act(&mut self, view: &View, rng: &mut SplitMix) -> usize;
}

fn sample(probs: &[f64], rng: &mut SplitMix) -> usize {
    let mut x = rng.next_f64();
    for (i, &p) in probs.iter().enumerate() {
        if x < p {
            return i;
        }
        x -= p;
    }
    probs.len() - 1
}

/// Plays the blueprint's average strategy.
pub struct BlueprintAgent<'a, A: CardAbstraction> {
    pub bp: &'a Blueprint<A>,
    pub m: &'a Mccfr<'a, Blueprint<A>>,
    pub label: String,
}

impl<A: CardAbstraction> Agent for BlueprintAgent<'_, A> {
    fn name(&self) -> String {
        self.label.clone()
    }
    fn act(&mut self, v: &View, rng: &mut SplitMix) -> usize {
        let bucket = self.bp.cards.bucket(v.hole, v.board);
        sample(&self.m.average_strategy(v.node, bucket), rng)
    }
}

/// Never folds, never raises.
pub struct CallingStation;

impl Agent for CallingStation {
    fn name(&self) -> String {
        "always call".into()
    }
    fn act(&mut self, v: &View, _: &mut SplitMix) -> usize {
        v.actions
            .iter()
            .position(|a| matches!(a, Action::Check | Action::Call))
            .expect("check or call is always available")
    }
}

/// Raises with the smallest size whenever it can, otherwise calls.
pub struct Raiser;

impl Agent for Raiser {
    fn name(&self) -> String {
        "always raise".into()
    }
    fn act(&mut self, v: &View, _: &mut SplitMix) -> usize {
        v.actions
            .iter()
            .position(|a| matches!(a, Action::RaiseTo(_)))
            .or_else(|| {
                v.actions
                    .iter()
                    .position(|a| matches!(a, Action::Check | Action::Call))
            })
            .unwrap()
    }
}

/// Uniformly random among the abstract actions.
pub struct RandomAgent;

impl Agent for RandomAgent {
    fn name(&self) -> String {
        "random".into()
    }
    fn act(&mut self, v: &View, rng: &mut SplitMix) -> usize {
        rng.below(v.actions.len())
    }
}

/// A sensible hand-strength player: Monte Carlo equity against a random hand,
/// bets or raises strong hands, calls when the price is right, folds otherwise.
pub struct EquityAgent {
    pub samples: usize,
}

impl EquityAgent {
    fn equity(&self, hole: [Card; 2], board: &[Card], rng: &mut SplitMix) -> f64 {
        let dead: Vec<Card> = hole.iter().chain(board).copied().collect();
        let live: Vec<Card> = Card::deck().filter(|c| !dead.contains(c)).collect();
        let mut score = 0.0;
        for _ in 0..self.samples {
            let mut pool = live.clone();
            let mut draw = || {
                let k = rng.below(pool.len());
                pool.swap_remove(k)
            };
            let opp = [draw(), draw()];
            let mut full = board.to_vec();
            while full.len() < 5 {
                full.push(draw());
            }
            let b = Hand::from_cards(&full);
            let (me, them) = (b.with(&hole).rank(), b.with(&opp).rank());
            score += match me.cmp(&them) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Less => 0.0,
            };
        }
        score / self.samples as f64
    }
}

impl Agent for EquityAgent {
    fn name(&self) -> String {
        "equity heuristic".into()
    }
    fn act(&mut self, v: &View, rng: &mut SplitMix) -> usize {
        let eq = self.equity(v.hole, v.board, rng);
        let to_call = v.state.to_call() as f64;
        let pot = v.state.pot() as f64;
        let find = |pred: &dyn Fn(&Action) -> bool| v.actions.iter().position(pred);
        let passive = find(&|a| matches!(a, Action::Check | Action::Call)).unwrap();
        let raise = find(&|a| matches!(a, Action::RaiseTo(_)));
        if eq > 0.72
            && let Some(r) = raise
        {
            return r;
        }
        if to_call == 0.0 {
            return if eq > 0.6 {
                raise.unwrap_or(passive)
            } else {
                passive
            };
        }
        if eq >= to_call / (pot + to_call) + 0.05 {
            passive
        } else {
            find(&|a| matches!(a, Action::Fold)).unwrap_or(passive)
        }
    }
}

/// Result of a match from agent A's point of view.
#[derive(Clone, Debug)]
pub struct MatchResult {
    pub a: String,
    pub b: String,
    /// Duplicate pairs played (each is two hands).
    pub pairs: usize,
    pub mbb_per_hand: f64,
    /// Half-width of the 95% confidence interval, in mbb/hand.
    pub ci95: f64,
}

impl std::fmt::Display for MatchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} vs {}: {:+.0} ± {:.0} mbb/hand over {} hands",
            self.a,
            self.b,
            self.mbb_per_hand,
            self.ci95,
            self.pairs * 2
        )
    }
}

/// Plays one hand; returns chips won by seat 0.
fn play_hand<A: CardAbstraction>(
    bp: &Blueprint<A>,
    hole: [[Card; 2]; 2],
    board: &[Card; 5],
    agents: [&mut dyn Agent; 2],
    rng: &mut SplitMix,
) -> i32 {
    let seats = agents;
    let mut state = State::new(bp.params);
    let mut node = 0;
    while !state.is_terminal() {
        let BettingNode::Action { children, .. } = &bp.nodes()[node] else {
            unreachable!("non-terminal state at a terminal node")
        };
        let actions = bp.node_actions(node);
        let p = state.to_act;
        let view = View {
            seat: p,
            hole: hole[p],
            board: &board[..state.board_len()],
            state: &state,
            actions,
            node,
        };
        let i = seats[p].act(&view, rng);
        state = state.apply(actions[i]).expect("abstract action is legal");
        node = children[i];
    }
    state.payoff(hole, board)[0]
}

/// Plays `pairs` duplicate pairs between agents made by `make_a` and `make_b`,
/// in parallel. Agent factories are called once per worker chunk.
pub fn duplicate_match<'x, A, FA, FB>(
    bp: &Blueprint<A>,
    make_a: FA,
    make_b: FB,
    pairs: usize,
    seed: u64,
) -> MatchResult
where
    A: CardAbstraction,
    FA: Fn() -> Box<dyn Agent + 'x> + Sync,
    FB: Fn() -> Box<dyn Agent + 'x> + Sync,
{
    const CHUNK: usize = 2_000;
    let chunks = pairs.div_ceil(CHUNK);
    let per_chunk: Vec<(f64, f64, usize)> = (0..chunks)
        .into_par_iter()
        .map(|c| {
            let mut rng = SplitMix(seed ^ (c as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut a = make_a();
            let mut b = make_b();
            let n = CHUNK.min(pairs - c * CHUNK);
            let (mut sum, mut sq) = (0.0, 0.0);
            for _ in 0..n {
                let deal = bp.sample(&mut rng);
                let x = play_hand(
                    bp,
                    deal.hole,
                    &deal.board,
                    [a.as_mut(), b.as_mut()],
                    &mut rng,
                );
                let y = -play_hand(
                    bp,
                    deal.hole,
                    &deal.board,
                    [b.as_mut(), a.as_mut()],
                    &mut rng,
                );
                let pair = (x + y) as f64;
                sum += pair;
                sq += pair * pair;
            }
            (sum, sq, n)
        })
        .collect();
    let (sum, sq, n) = per_chunk.iter().fold((0.0, 0.0, 0), |acc, x| {
        (acc.0 + x.0, acc.1 + x.1, acc.2 + x.2)
    });
    let mean = sum / n as f64;
    let var = (sq / n as f64 - mean * mean).max(0.0);
    let se = (var / n as f64).sqrt();
    // chips per pair -> mbb per hand: / 2 hands, / big blind, * 1000.
    let scale = 1000.0 / (2.0 * bp.params.big_blind as f64);
    let (a, b) = (make_a().name(), make_b().name());
    MatchResult {
        a,
        b,
        pairs: n,
        mbb_per_hand: mean * scale,
        ci95: 1.96 * se * scale,
    }
}
