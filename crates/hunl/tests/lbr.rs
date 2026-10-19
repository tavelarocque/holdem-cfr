//! M6.5 gate: local best response computes equities and ranges correctly and
//! finds real exploitability in weak blueprints.

use engine::{BettingNode, Mccfr, SampledGame, SplitMix, Weighting};
use eval::{Card, Hand, parse_cards};
use hunl::arena::{Agent, BlueprintAgent, View, duplicate_match};
use hunl::lbr::{LbrAgent, all_combos, equities};
use hunl::{Action, ActionAbstraction, Blueprint, CardAbstraction, CategoryBuckets, Params, State};

fn blueprint() -> Blueprint<CategoryBuckets> {
    Blueprint::new(
        Params::DEFAULT,
        ActionAbstraction::default_100bb(),
        CategoryBuckets::default(),
    )
}

fn brute(hole: [Card; 2], opp: [Card; 2], board: &[Card]) -> f64 {
    let dead: Vec<Card> = hole.iter().chain(&opp).chain(board).copied().collect();
    let live: Vec<Card> = Card::deck().filter(|c| !dead.contains(c)).collect();
    let mut runouts: Vec<Vec<Card>> = Vec::new();
    match board.len() {
        5 => runouts.push(vec![]),
        4 => runouts.extend(live.iter().map(|&c| vec![c])),
        _ => {
            for i in 0..live.len() {
                for j in i + 1..live.len() {
                    runouts.push(vec![live[i], live[j]]);
                }
            }
        }
    }
    let n = runouts.len() as f64;
    runouts
        .iter()
        .map(|r| {
            let b = Hand::from_cards(board).with(r);
            match b.with(&hole).rank().cmp(&b.with(&opp).rank()) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Less => 0.0,
            }
        })
        .sum::<f64>()
        / n
}

#[test]
fn equities_match_brute_force() {
    let combos = all_combos();
    let hole: [Card; 2] = parse_cards("Ah Kh").unwrap().try_into().unwrap();
    let mut rng = SplitMix(3);
    for board in ["Qh Jh 2c 7d 9s", "Qh Jh 2c 7d"] {
        let board = parse_cards(board).unwrap();
        let eq = equities(hole, &board, &combos, 0, &mut rng);
        for (i, h) in combos.iter().enumerate().step_by(37) {
            if hole.contains(&h[0])
                || hole.contains(&h[1])
                || board.contains(&h[0])
                || board.contains(&h[1])
            {
                assert_eq!(eq[i], 0.0);
                continue;
            }
            assert!((eq[i] - brute(hole, *h, &board)).abs() < 1e-12, "{h:?}");
        }
    }
    // The flop is sampled: close to exact, not equal.
    let board = parse_cards("Qh Jh 2c").unwrap();
    let eq = equities(hole, &board, &combos, 4000, &mut rng);
    for (i, h) in combos.iter().enumerate().step_by(97) {
        if eq[i] == 0.0 {
            continue;
        }
        let exact = brute(hole, *h, &board);
        assert!((eq[i] - exact).abs() < 0.04, "{h:?}: {} vs {exact}", eq[i]);
    }
}

#[test]
fn range_is_bayes_with_the_blueprint_strategy() {
    let bp = blueprint();
    let m = Mccfr::new(&bp, Weighting::Uniform);
    m.run(200_000, 4);
    let lbr = LbrAgent::new(&bp, &m, 64);
    // The button limps; LBR is the big blind.
    let state = State::new(Params::DEFAULT).apply(Action::Call).unwrap();
    let hole: [Card; 2] = parse_cards("7c 2d").unwrap().try_into().unwrap();
    let call = bp
        .node_actions(0)
        .iter()
        .position(|a| *a == Action::Call)
        .unwrap();
    let child = match &bp.nodes()[0] {
        BettingNode::Action { children, .. } => children[call],
        _ => unreachable!(),
    };
    let actions = bp.node_actions(child).to_vec();
    let view = View {
        seat: 1,
        hole,
        board: &[],
        state: &state,
        actions: &actions,
        node: child,
    };
    let w = lbr.range(&view);
    for (h, &x) in all_combos().iter().zip(&w) {
        if hole.contains(&h[0]) || hole.contains(&h[1]) {
            assert_eq!(x, 0.0);
        } else {
            let want = m.average_strategy(0, bp.cards.bucket(*h, &[]))[call];
            assert!((x - want).abs() < 1e-12);
        }
    }
}

type Factory<'a> = Box<dyn Fn() -> Box<dyn Agent + 'a> + Sync + 'a>;

#[test]
fn lbr_exploits_weak_blueprints_and_less_so_trained_ones() {
    let bp = blueprint();
    let untrained = Mccfr::new(&bp, Weighting::Uniform);
    let trained = Mccfr::new(&bp, Weighting::Uniform);
    for batch in 1..=6u64 {
        trained.run(250_000, batch);
        let f = batch as f64 / (batch + 1) as f64;
        trained.discount(f, f);
    }
    let bp = &bp;
    let mut results = Vec::new();
    for (label, m) in [("untrained", &untrained), ("trained", &trained)] {
        let lbr: Factory = Box::new(move || Box::new(LbrAgent::new(bp, m, 48)));
        let me: Factory = Box::new(move || {
            Box::new(BlueprintAgent {
                bp,
                m,
                label: label.into(),
            })
        });
        let r = duplicate_match(bp, &lbr, &me, 3_000, 11);
        eprintln!("{r}");
        assert!(
            r.mbb_per_hand - r.ci95 > 0.0,
            "LBR should beat the {label} blueprint: {r}"
        );
        results.push(r.mbb_per_hand);
    }
    assert!(
        results[1] < results[0],
        "training should reduce exploitability: {results:?}"
    );
}
