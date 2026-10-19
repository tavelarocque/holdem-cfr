//! Milestone 2 gate: Leduc hold'em, CFR vs CFR+ vs DCFR.

use engine::{Solver, Tree, Variant, expected_value, exploitability};
use games::Leduc;
use proptest::prelude::*;

/// Published game value of Leduc hold'em for the first player.
const GAME_VALUE: f64 = -0.0856;

fn exploitability_after(tree: &Tree, variant: Variant, iters: u64) -> f64 {
    let mut solver = Solver::new(tree, variant);
    solver.run(iters);
    exploitability(tree, &solver.average_strategy())
}

#[test]
fn tree_has_known_size() {
    let tree = Tree::build(&Leduc);
    assert_eq!(tree.infosets.len(), 936);
    assert_eq!(tree.infosets_of(0), 468);
    assert_eq!(tree.infosets_of(1), 468);
}

#[test]
fn cfr_plus_converges_to_the_game_value() {
    let tree = Tree::build(&Leduc);
    let mut solver = Solver::new(&tree, Variant::CfrPlus);
    solver.run(2_000);
    let avg = solver.average_strategy();
    let expl = exploitability(&tree, &avg);
    let value = expected_value(&tree, &avg);
    assert!(expl < 2e-4, "exploitability {expl}");
    assert!((value - GAME_VALUE).abs() < 1e-3, "value {value}");
}

#[test]
fn vanilla_cfr_exploitability_falls() {
    let tree = Tree::build(&Leduc);
    let early = exploitability_after(&tree, Variant::Vanilla, 100);
    let late = exploitability_after(&tree, Variant::Vanilla, 1_000);
    assert!(late < early / 4.0, "early {early}, late {late}");
    assert!(late < 3e-2, "late {late}");
}

#[test]
fn cfr_plus_and_dcfr_are_much_faster_than_cfr() {
    let tree = Tree::build(&Leduc);
    let cfr = exploitability_after(&tree, Variant::Vanilla, 1_000);
    let plus = exploitability_after(&tree, Variant::CfrPlus, 1_000);
    let dcfr = exploitability_after(&tree, Variant::DCFR, 1_000);
    assert!(plus < cfr / 10.0, "CFR {cfr}, CFR+ {plus}");
    assert!(dcfr < cfr / 10.0, "CFR {cfr}, DCFR {dcfr}");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Invariants that must hold after any number of iterations of any variant.
    #[test]
    fn solver_invariants(iters in 1u64..40, which in 0usize..4) {
        let variant = [Variant::Vanilla, Variant::CfrPlus, Variant::LINEAR, Variant::DCFR][which];
        let tree = Tree::build(&Leduc);
        let mut solver = Solver::new(&tree, variant);
        solver.run(iters);

        prop_assert!(solver.regrets().iter().all(|r| r.is_finite()));
        if variant == Variant::CfrPlus {
            prop_assert!(solver.regrets().iter().all(|&r| r >= 0.0));
        }
        let avg = solver.average_strategy();
        let cur = solver.current_strategy().to_vec();
        for info in &tree.infosets {
            for s in [&avg, &cur] {
                let sum: f64 = s[info.range()].iter().sum();
                prop_assert!((sum - 1.0).abs() < 1e-9, "{} sums to {}", info.label, sum);
                prop_assert!(s[info.range()].iter().all(|&p| (0.0..=1.0).contains(&p)));
            }
        }
        prop_assert!(exploitability(&tree, &avg) >= -1e-12);
    }
}
