//! Milestone 4 gate, part 2: the vectorized solver agrees with the scalar engine.
//!
//! The same subgame is re-expressed through `engine::Game`. There, a chance node
//! deals one specific pair of hands (probability proportional to w0 * w1 over
//! compatible pairs), every public node is visited per pair, and showdowns call
//! the hand evaluator directly. That shares none of the vector code's
//! inclusion-exclusion card removal, O(n) showdown sweep or chance weighting, so
//! agreement is strong evidence that all three are right.

use engine::{Game, NodeType, Tree, best_response_value, expected_value, exploitability};
use eval::{Card, parse_cards};
use postflop::{Algorithm, BetConfig, NodeKind, Range, Solver, Spot, SpotConfig};

struct Scalar<'a> {
    spot: &'a Spot,
}

#[derive(Clone)]
enum St {
    Deal,
    At { hands: [usize; 2], node: usize },
}

impl Scalar<'_> {
    fn cards(&self, p: usize, h: usize) -> [Card; 2] {
        let (a, b) = self.spot.hands[p][h];
        [a, b]
    }
    fn disjoint(&self, hands: [usize; 2]) -> bool {
        let (a, b) = (self.cards(0, hands[0]), self.cards(1, hands[1]));
        a.iter().all(|c| !b.contains(c))
    }
}

impl Game for Scalar<'_> {
    type State = St;
    type Action = usize;
    type InfoKey = (usize, usize, usize);

    fn root(&self) -> St {
        St::Deal
    }

    fn node_type(&self, s: &St) -> NodeType {
        match s {
            St::Deal => NodeType::Chance,
            St::At { node, .. } => match &self.spot.tree.nodes[*node].kind {
                NodeKind::Action { player, .. } => NodeType::Decision(*player),
                NodeKind::Chance { .. } => NodeType::Chance,
                NodeKind::Fold { .. } | NodeKind::Showdown => NodeType::Terminal,
            },
        }
    }

    fn payoff(&self, s: &St) -> f64 {
        let St::At { hands, node } = s else {
            unreachable!()
        };
        let n = &self.spot.tree.nodes[*node];
        let half = self.spot.tree.pot as f64 / 2.0;
        match n.kind {
            NodeKind::Fold { folder: 0 } => -(half + n.invested[0] as f64),
            NodeKind::Fold { .. } => half + n.invested[1] as f64,
            NodeKind::Showdown => {
                let board = &self.spot.tree.boards[n.board];
                let strength = |p: usize| {
                    let mut seven = board.clone();
                    seven.extend(self.cards(p, hands[p]));
                    eval::evaluate(&seven)
                };
                let stake = half + n.invested[0] as f64;
                match strength(0).cmp(&strength(1)) {
                    std::cmp::Ordering::Greater => stake,
                    std::cmp::Ordering::Less => -stake,
                    std::cmp::Ordering::Equal => 0.0,
                }
            }
            _ => unreachable!(),
        }
    }

    fn chance_outcomes(&self, s: &St) -> Vec<(f64, St)> {
        match s {
            St::Deal => {
                let mut out = Vec::new();
                for i in 0..self.spot.num_hands(0) {
                    for j in 0..self.spot.num_hands(1) {
                        if self.disjoint([i, j]) {
                            let p = self.spot.weights[0][i] as f64 * self.spot.weights[1][j] as f64
                                / self.spot.norm;
                            out.push((
                                p,
                                St::At {
                                    hands: [i, j],
                                    node: 0,
                                },
                            ));
                        }
                    }
                }
                out
            }
            St::At { hands, node } => {
                let NodeKind::Chance {
                    cards, children, ..
                } = &self.spot.tree.nodes[*node].kind
                else {
                    unreachable!()
                };
                let held: Vec<Card> = [self.cards(0, hands[0]), self.cards(1, hands[1])].concat();
                let live: Vec<usize> = (0..cards.len())
                    .filter(|&k| !held.contains(&cards[k]))
                    .collect();
                let p = 1.0 / live.len() as f64;
                live.into_iter()
                    .map(|k| {
                        (
                            p,
                            St::At {
                                hands: *hands,
                                node: children[k],
                            },
                        )
                    })
                    .collect()
            }
        }
    }

    fn actions(&self, s: &St) -> Vec<usize> {
        let St::At { node, .. } = s else {
            unreachable!()
        };
        match &self.spot.tree.nodes[*node].kind {
            NodeKind::Action { actions, .. } => (0..actions.len()).collect(),
            _ => unreachable!(),
        }
    }

    fn apply(&self, s: &St, a: usize) -> St {
        let St::At { hands, node } = s else {
            unreachable!()
        };
        let NodeKind::Action { children, .. } = &self.spot.tree.nodes[*node].kind else {
            unreachable!()
        };
        St::At {
            hands: *hands,
            node: children[a],
        }
    }

    fn info_key(&self, s: &St) -> (usize, usize, usize) {
        let St::At { hands, node } = s else {
            unreachable!()
        };
        let NodeKind::Action { player, .. } = &self.spot.tree.nodes[*node].kind else {
            unreachable!()
        };
        (*player, hands[*player], *node)
    }
}

/// The vector solver's average strategy, laid out as an engine strategy vector.
fn to_engine_strategy(tree: &Tree, solver: &Solver, spot: &Spot) -> Vec<f64> {
    let mut s = vec![0.0; tree.num_actions];
    for info in &tree.infosets {
        let nums: Vec<usize> = info
            .label
            .trim_matches(|c| c == '(' || c == ')')
            .split(", ")
            .map(|x| x.parse().unwrap())
            .collect();
        let (player, hand, node) = (nums[0], nums[1], nums[2]);
        let sigma = solver.average_strategy(node);
        let n = spot.num_hands(player);
        for a in 0..info.num_actions() {
            s[info.offset + a] = sigma[a * n + hand] as f64;
        }
    }
    s
}

fn check(board: &str, bets: BetConfig, iters: u32) {
    let spot = Spot::new(&SpotConfig {
        board: parse_cards(board).unwrap(),
        ranges: [
            Range::parse("AhAd:0.5,KsKc,QhJh,7c6c,5s4s,9h9s:0.25").unwrap(),
            // QhJh appears in both ranges (identical-combo removal); 7h6h ties 7c6c.
            Range::parse("AhKh,AsQs,JcJd,9h8h,KcQc:0.75,Th9c,QhJh:0.6,7h6h").unwrap(),
        ],
        pot: 60,
        stack: 140,
        bets,
        isomorphism: false,
    });
    let scalar = Scalar { spot: &spot };
    let tree = Tree::build(&scalar);

    let mut solver = Solver::new(&spot, Algorithm::DCFR);
    let mut checked_unconverged = false;
    for i in 1..=iters {
        solver.iterate();
        // Compare the two best-response computations both early (a poor, highly
        // exploitable strategy) and at the end (a near-equilibrium one).
        if i == 3 || i == iters {
            let s = to_engine_strategy(&tree, &solver, &spot);
            let ev = expected_value(&tree, &s);
            assert!(
                (ev - solver.expected_value(0)).abs() < 2e-3,
                "EV {ev} vs {}",
                solver.expected_value(0)
            );
            for p in 0..2 {
                let a = best_response_value(&tree, &s, p);
                let b = solver.best_response_value(p);
                assert!((a - b).abs() < 2e-3, "BR{p}: engine {a} vs vector {b}");
            }
            let (a, b) = (exploitability(&tree, &s), solver.exploitability());
            assert!(
                (a - b).abs() < 2e-3,
                "exploitability: engine {a} vs vector {b}"
            );
            if i == 3 {
                assert!(
                    a > 0.1,
                    "early strategy should be clearly exploitable, got {a}"
                );
                checked_unconverged = true;
            }
        }
    }
    assert!(checked_unconverged);
    assert!(
        solver.exploitability_pct() < 0.5,
        "{}",
        solver.exploitability_pct()
    );

    // Independently, scalar CFR+ on the engine reaches the same game value.
    let mut engine_solver = engine::Solver::new(&tree, engine::Variant::CfrPlus);
    engine_solver.run(1500);
    let avg = engine_solver.average_strategy();
    let engine_value = expected_value(&tree, &avg);
    let slack = exploitability(&tree, &avg) + solver.exploitability() + 1e-3;
    assert!(
        (engine_value - solver.expected_value(0)).abs() <= 2.0 * slack,
        "game value: engine {engine_value} vs vector {}",
        solver.expected_value(0)
    );
}

#[test]
fn river_with_raises_allin_and_card_removal() {
    check(
        "Tc 8d 5h 3d 2s",
        BetConfig::symmetric(&[0.5, 1.0], &[1.0], 3, true),
        400,
    );
}

#[test]
fn turn_with_river_runouts() {
    check(
        "Tc 8d 5h 3d",
        BetConfig::symmetric(&[0.75], &[], 1, true),
        300,
    );
}
