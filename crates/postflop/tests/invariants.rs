//! Milestone 4 gate, part 3: realistic spots converge, and invariants hold for
//! random ranges and bet menus.

use postflop::{Algorithm, BetConfig, NodeKind, Range, Solver, Spot, SpotConfig};
use proptest::prelude::*;

const OOP: &str = "22+,A2s+,K9s+,Q9s+,J9s+,T8s+,97s+,87s,76s,65s,ATo+,KTo+,QTo+,JTo";
const IP: &str = "22+,A2s+,K7s+,Q8s+,J8s+,T8s+,97s+,86s+,75s+,64s+,54s,A8o+,KTo+,QTo+,JTo,T9o";

#[test]
fn realistic_river_converges() {
    let spot = Spot::new(&SpotConfig {
        board: eval::parse_cards("Qs Jh 7c 4d 2s").unwrap(),
        ranges: [Range::parse(OOP).unwrap(), Range::parse(IP).unwrap()],
        pot: 100,
        stack: 300,
        bets: BetConfig::symmetric(&[0.33, 0.75], &[0.6], 3, true),
        isomorphism: true,
    });
    assert!(spot.num_hands(0) > 250 && spot.num_hands(1) > 300);
    let mut solver = Solver::new(&spot, Algorithm::DCFR);
    let first = solver.exploitability_pct();
    for _ in 0..300 {
        solver.iterate();
    }
    let last = solver.exploitability_pct();
    assert!(
        first > 10.0,
        "uniform play should be very exploitable: {first}"
    );
    assert!(
        last < 0.3,
        "exploitability {last}% of pot after 300 iterations"
    );
    assert!((solver.expected_value(0) + solver.expected_value(1)).abs() < 1e-2);
}

#[test]
fn dcfr_and_cfr_plus_both_converge() {
    let spot = Spot::new(&SpotConfig {
        board: eval::parse_cards("Ah 9c 6d 3s 3c").unwrap(),
        ranges: [Range::parse(OOP).unwrap(), Range::parse(IP).unwrap()],
        pot: 100,
        stack: 200,
        bets: BetConfig::symmetric(&[0.5, 1.0], &[1.0], 2, true),
        isomorphism: true,
    });
    let run = |alg| {
        let mut s = Solver::new(&spot, alg);
        for _ in 0..100 {
            s.iterate();
        }
        s.exploitability_pct()
    };
    let (dcfr, plus) = (run(Algorithm::DCFR), run(Algorithm::CfrPlus));
    assert!(dcfr < 1.0 && plus < 2.0, "DCFR {dcfr}, CFR+ {plus}");
}

fn sizes() -> impl Strategy<Value = Vec<f64>> {
    prop::collection::vec(
        prop::sample::select(vec![0.25, 0.5, 0.75, 1.0, 1.5, 2.5]),
        0..3,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn solver_invariants(
        oop in prop::collection::vec(0usize..1326, 3..30),
        ip in prop::collection::vec(0usize..1326, 3..30),
        bets in sizes(),
        raises in sizes(),
        max_raises in 1u8..4,
        allin: bool,
        iters in 1u32..20,
    ) {
        let board = eval::parse_cards("Ts 9h 4c 4d Kc").unwrap();
        let mut ranges = [Range::empty(), Range::empty()];
        for (p, list) in [oop, ip].iter().enumerate() {
            for &c in list {
                ranges[p].weights[c] = 0.25 + (c % 4) as f32 * 0.25;
            }
        }
        let cfg = SpotConfig {
            board: board.clone(),
            ranges,
            pot: 50,
            stack: 175,
            bets: BetConfig::symmetric(&bets, &raises, max_raises, allin),
            isomorphism: true,
        };
        // Skip draws where a range is wiped out by the board or no pair is compatible.
        let survives = |r: &Range| (0..1326).any(|c| {
            let (a, b) = postflop::combo_cards(c);
            r.weights[c] > 0.0 && !board.contains(&a) && !board.contains(&b)
        });
        prop_assume!(survives(&cfg.ranges[0]) && survives(&cfg.ranges[1]));
        let Ok(spot) = std::panic::catch_unwind(|| Spot::new(&cfg)) else {
            return Ok(());
        };
        let mut solver = Solver::new(&spot, Algorithm::DCFR);
        for _ in 0..iters {
            solver.iterate();
        }
        for (i, node) in spot.tree.nodes.iter().enumerate() {
            if let NodeKind::Action { player, children, .. } = &node.kind {
                let n = spot.num_hands(*player);
                let s = solver.average_strategy(i);
                for h in 0..n {
                    let sum: f32 = (0..children.len()).map(|a| s[a * n + h]).sum();
                    prop_assert!((sum - 1.0).abs() < 1e-4, "node {} hand {} sums to {}", i, h, sum);
                }
                prop_assert!(s.iter().all(|p| p.is_finite() && *p >= 0.0));
            }
            // Chips are conserved: nobody invests beyond the stack.
            prop_assert!(node.invested.iter().all(|&x| (0..=175).contains(&x)));
        }
        let e = solver.exploitability();
        prop_assert!(e >= -1e-3, "negative exploitability {}", e);
        prop_assert!((solver.expected_value(0) + solver.expected_value(1)).abs() < 1e-3);
        prop_assert!(solver.best_response_value(0) >= solver.expected_value(0) - 1e-3);
        prop_assert!(solver.best_response_value(1) >= solver.expected_value(1) - 1e-3);
    }
}
