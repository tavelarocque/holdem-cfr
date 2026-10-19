//! M6.3 gate: the external-sampling MCCFR that will train the no-limit blueprint
//! converges on Leduc, where exploitability can be computed exactly.

use engine::{BettingNode, Mccfr, SampledGame, Tree, Weighting, expected_value, exploitability};
use games::Leduc;
use games::leduc_sampled::LeducSampled;

fn card_index(name: &str) -> u8 {
    let mut c = name.chars();
    let rank = match c.next().unwrap() {
        'J' => 0,
        'Q' => 1,
        'K' => 2,
        r => panic!("bad rank {r}"),
    };
    let suit = match c.next().unwrap() {
        's' => 0,
        'h' => 1,
        s => panic!("bad suit {s}"),
    };
    rank * 2 + suit
}

/// The MCCFR average strategy as a strategy vector for the exact engine.
fn to_engine(tree: &Tree, m: &Mccfr<LeducSampled>, g: &LeducSampled) -> Vec<f64> {
    let mut s = vec![0.0; tree.num_actions];
    for info in &tree.infosets {
        let parts: Vec<&str> = info.label.split('|').collect();
        let card = card_index(parts[0]);
        let board = (parts[1] != "--").then(|| card_index(parts[1]));
        let node = g
            .node(parts[2])
            .unwrap_or_else(|| panic!("no node {}", parts[2]));
        let BettingNode::Action { children, .. } = &g.nodes()[node] else {
            panic!("not an action node")
        };
        assert_eq!(children.len(), info.num_actions());
        let strat = m.average_strategy(node, LeducSampled::bucket_of(card, board));
        s[info.range()].copy_from_slice(&strat);
    }
    s
}

#[test]
fn mccfr_converges_on_leduc() {
    let tree = Tree::build(&Leduc);
    let game = LeducSampled::new();
    let m = Mccfr::new(&game, Weighting::Linear);
    let mut curve = Vec::new();
    for target in [10_000u64, 100_000, 400_000] {
        m.run(target - m.iterations(), 42);
        let s = to_engine(&tree, &m, &game);
        curve.push((target, exploitability(&tree, &s), expected_value(&tree, &s)));
    }
    eprintln!("MCCFR on Leduc (iterations, exploitability, value): {curve:?}");
    let (_, first, _) = curve[0];
    let (_, last, value) = curve[2];
    assert!(last < first / 3.0, "not converging: {curve:?}");
    assert!(
        last < 0.05,
        "exploitability {last} after 400k iterations: {curve:?}"
    );
    // Published Leduc game value for the first player is -0.0856.
    assert!((value + 0.0856).abs() < 0.03, "value {value}");
}

#[test]
fn single_threaded_runs_are_reproducible() {
    let game = LeducSampled::new();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let run = || {
        pool.install(|| {
            let m = Mccfr::new(&game, Weighting::Uniform);
            m.run(2_000, 9);
            let root = 0;
            (0..game.num_buckets(0))
                .map(|b| m.average_strategy(root, b))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(run(), run());
}
