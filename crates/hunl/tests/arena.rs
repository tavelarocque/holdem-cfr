//! M6.4 gate: the match harness is unbiased, and a briefly trained blueprint
//! beats simple baselines by a statistically clear margin.

use engine::{Mccfr, Weighting};
use hunl::arena::{
    Agent, BlueprintAgent, CallingStation, EquityAgent, Raiser, RandomAgent, duplicate_match,
};
use hunl::{ActionAbstraction, Blueprint, CategoryBuckets, Params};

type AgentFactory<'a> = Box<dyn Fn() -> Box<dyn Agent + 'a> + Sync + 'a>;

fn blueprint() -> Blueprint<CategoryBuckets> {
    Blueprint::new(
        Params::DEFAULT,
        ActionAbstraction::default_100bb(),
        CategoryBuckets::default(),
    )
}

#[test]
fn identical_agents_break_even() {
    let bp = blueprint();
    let r = duplicate_match(
        &bp,
        || Box::new(RandomAgent) as Box<dyn Agent>,
        || Box::new(RandomAgent),
        20_000,
        1,
    );
    eprintln!("{r}");
    assert!(r.mbb_per_hand.abs() < r.ci95 * 1.5 + 5.0, "{r}");
    // A deterministic agent against itself is exactly zero: duplicate play
    // gives each copy both seats of every deal.
    let r = duplicate_match(
        &bp,
        || Box::new(CallingStation) as Box<dyn Agent>,
        || Box::new(CallingStation),
        5_000,
        2,
    );
    assert_eq!(r.mbb_per_hand, 0.0, "{r}");
}

#[test]
fn trained_blueprint_beats_baselines() {
    let bp = blueprint();
    let m = Mccfr::new(&bp, Weighting::Uniform);
    for batch in 1..=6u64 {
        m.run(250_000, batch);
        let f = batch as f64 / (batch + 1) as f64;
        m.discount(f, f);
    }
    let me = || {
        Box::new(BlueprintAgent {
            bp: &bp,
            m: &m,
            label: "blueprint".into(),
        }) as Box<dyn Agent>
    };
    let opponents: Vec<(&str, AgentFactory<'_>)> = vec![
        (
            "call",
            Box::new(|| Box::new(CallingStation) as Box<dyn Agent>),
        ),
        ("raise", Box::new(|| Box::new(Raiser) as Box<dyn Agent>)),
        (
            "random",
            Box::new(|| Box::new(RandomAgent) as Box<dyn Agent>),
        ),
    ];
    for (name, opp) in &opponents {
        let r = duplicate_match(&bp, me, opp, 20_000, 7);
        eprintln!("{r}");
        assert!(
            r.mbb_per_hand - r.ci95 > 0.0,
            "blueprint does not clearly beat {name}: {r}"
        );
    }
    let r = duplicate_match(&bp, me, || Box::new(EquityAgent { samples: 40 }), 4_000, 9);
    eprintln!("{r}");
}
