//! Milestone 1 gate: CFR and best response are correct on Kuhn poker.

use engine::{Solver, Tree, Variant, best_response_value, expected_value, exploitability};
use games::Kuhn;
use proptest::prelude::*;

const GAME_VALUE: f64 = -1.0 / 18.0;

/// The analytic Nash equilibrium family, parameterized by alpha in [0, 1/3].
/// Probabilities are [Pass, Bet] at each information set.
fn analytic_equilibrium(tree: &Tree, alpha: f64) -> Vec<f64> {
    let bet = |p: f64| [1.0 - p, p];
    let table: [(&str, [f64; 2]); 12] = [
        // Player 0, first action.
        ("J", bet(alpha)),
        ("Q", bet(0.0)),
        ("K", bet(3.0 * alpha)),
        // Player 0 facing a bet after checking: "Bet" means call.
        ("Jpb", bet(0.0)),
        ("Qpb", bet(alpha + 1.0 / 3.0)),
        ("Kpb", bet(1.0)),
        // Player 1 after a check.
        ("Jp", bet(1.0 / 3.0)),
        ("Qp", bet(0.0)),
        ("Kp", bet(1.0)),
        // Player 1 facing a bet.
        ("Jb", bet(0.0)),
        ("Qb", bet(1.0 / 3.0)),
        ("Kb", bet(1.0)),
    ];
    let mut s = vec![0.0; tree.num_actions];
    for (label, probs) in table {
        let info = tree
            .find(label)
            .unwrap_or_else(|| panic!("no infoset {label}"));
        s[info.range()].copy_from_slice(&probs);
    }
    s
}

#[test]
fn tree_has_known_size() {
    let tree = Tree::build(&Kuhn);
    assert_eq!(tree.infosets.len(), 12);
    assert_eq!(tree.infosets_of(0), 6);
    assert_eq!(tree.infosets_of(1), 6);
    // 1 deal node + 6 deals x 9 betting nodes (4 decisions, 5 terminals).
    assert_eq!(tree.nodes.len(), 55);
}

#[test]
fn analytic_equilibria_are_unexploitable() {
    let tree = Tree::build(&Kuhn);
    for alpha in [0.0, 1.0 / 6.0, 1.0 / 3.0] {
        let s = analytic_equilibrium(&tree, alpha);
        assert!((expected_value(&tree, &s) - GAME_VALUE).abs() < 1e-12);
        assert!((best_response_value(&tree, &s, 0) - GAME_VALUE).abs() < 1e-12);
        assert!((best_response_value(&tree, &s, 1) + GAME_VALUE).abs() < 1e-12);
        assert!(exploitability(&tree, &s).abs() < 1e-12, "alpha={alpha}");
    }
}

#[test]
fn uniform_play_is_exploitable() {
    let tree = Tree::build(&Kuhn);
    assert!(exploitability(&tree, &tree.uniform_strategy()) > 0.1);
}

#[test]
fn every_variant_reaches_minus_one_eighteenth() {
    let tree = Tree::build(&Kuhn);
    for variant in [
        Variant::Vanilla,
        Variant::CfrPlus,
        Variant::LINEAR,
        Variant::DCFR,
    ] {
        let mut solver = Solver::new(&tree, variant);
        solver.run(20_000);
        let avg = solver.average_strategy();
        let value = expected_value(&tree, &avg);
        let expl = exploitability(&tree, &avg);
        assert!(
            (value - GAME_VALUE).abs() < 1e-3,
            "{variant:?}: value {value}"
        );
        assert!(expl < 1e-3, "{variant:?}: exploitability {expl}");
    }
}

#[test]
fn cfr_lands_inside_the_equilibrium_family() {
    // Whatever alpha CFR picks, the other probabilities must be tied to it.
    let tree = Tree::build(&Kuhn);
    let mut solver = Solver::new(&tree, Variant::Vanilla);
    solver.run(100_000);
    let s = solver.average_strategy();
    let bet = |label: &str| s[tree.find(label).unwrap().offset + 1];
    let alpha = bet("J");
    assert!((0.0..=1.0 / 3.0 + 1e-2).contains(&alpha));
    assert!((bet("K") - 3.0 * alpha).abs() < 2e-2);
    assert!((bet("Qpb") - (alpha + 1.0 / 3.0)).abs() < 2e-2);
    assert!((bet("Jb")).abs() < 1e-2);
    assert!((bet("Kb") - 1.0).abs() < 1e-2);
}

proptest! {
    /// For any strategy profile, a best response does at least as well as the profile.
    #[test]
    fn best_response_dominates_any_profile(raw in prop::collection::vec(0.01f64..1.0, 24)) {
        let tree = Tree::build(&Kuhn);
        let mut s = vec![0.0; tree.num_actions];
        for info in &tree.infosets {
            let r = info.range();
            let total: f64 = raw[r.clone()].iter().sum();
            for k in r { s[k] = raw[k] / total; }
        }
        let ev = expected_value(&tree, &s);
        prop_assert!(best_response_value(&tree, &s, 0) >= ev - 1e-12);
        prop_assert!(best_response_value(&tree, &s, 1) >= -ev - 1e-12);
        prop_assert!(exploitability(&tree, &s) >= -1e-12);
    }
}
