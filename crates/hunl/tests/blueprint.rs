//! M6.3 gate: the abstract no-limit tree is legal and consistent, and MCCFR on it
//! learns a sensible button strategy.

use engine::{BettingNode, Mccfr, SampledGame, SplitMix, Weighting};
use hunl::blueprint::{load_checkpoint, preflop_class_names, save_checkpoint};
use hunl::{Action, ActionAbstraction, Blueprint, CategoryBuckets, Params};

fn blueprint() -> Blueprint<CategoryBuckets> {
    Blueprint::new(
        Params::DEFAULT,
        ActionAbstraction::default_100bb(),
        CategoryBuckets::default(),
    )
}

#[test]
fn tree_is_consistent() {
    let bp = blueprint();
    assert_eq!(bp.action_nodes_per_street(), [64, 466, 1284, 3712]);
    let mut rng = SplitMix(3);
    let deal = bp.sample(&mut rng);
    for (i, node) in bp.nodes().iter().enumerate() {
        let info = bp.info(i);
        assert!(info.contributed.iter().all(|&c| (0..=10_000).contains(&c)));
        match node {
            BettingNode::Action { children, .. } => {
                assert!(children.len() >= 2, "node {i} has a single action");
                assert_eq!(children.len(), bp.node_actions(i).len());
                // Children are later in the arena (pre-order).
                assert!(children.iter().all(|&c| c > i));
            }
            BettingNode::Terminal => {
                // Folds and showdowns: utility is bounded by what was put in.
                let u = bp.utility(&deal, i);
                assert!(u.abs() <= info.contributed[0].max(info.contributed[1]) as f64);
                if info.folded.is_none() {
                    assert_eq!(info.contributed[0], info.contributed[1]);
                }
            }
        }
    }
}

#[test]
fn deals_are_valid() {
    let bp = blueprint();
    let mut rng = SplitMix(11);
    for _ in 0..10_000 {
        let d = bp.sample(&mut rng);
        let mut cards: Vec<u8> = d
            .hole
            .iter()
            .flatten()
            .chain(&d.board)
            .map(|c| c.index())
            .collect();
        cards.sort_unstable();
        cards.dedup();
        assert_eq!(cards.len(), 9);
        assert!(
            d.buckets
                .iter()
                .all(|b| (b[0] as usize) < 169 && b[1..].iter().all(|&x| x < 9))
        );
    }
}

#[test]
fn button_learns_to_play_strong_hands_and_fold_trash() {
    let bp = blueprint();
    let m = Mccfr::new(&bp, Weighting::Uniform);
    for batch in 1..=6u64 {
        m.run(250_000, batch);
        let f = batch as f64 / (batch + 1) as f64;
        m.discount(f, f);
    }
    let names = preflop_class_names();
    let acts = bp.node_actions(0);
    let play = |name: &str| {
        let class = names.iter().position(|n| n == name).unwrap();
        let s = m.average_strategy(0, class);
        acts.iter()
            .zip(&s)
            .filter(|(a, _)| !matches!(a, Action::Fold))
            .map(|(_, p)| p)
            .sum::<f64>()
    };
    for strong in ["AA", "KK", "AKs", "AKo", "QJs"] {
        assert!(play(strong) > 0.95, "{strong} plays {}", play(strong));
    }
    let trash: f64 = ["72o", "82o", "62o", "32o", "42o"]
        .iter()
        .map(|h| play(h))
        .sum::<f64>()
        / 5.0;
    assert!(trash < 0.5, "trash plays {trash}");
}

#[test]
fn checkpoints_round_trip() {
    let bp = blueprint();
    let m = Mccfr::new(&bp, Weighting::Uniform);
    m.run(20_000, 5);
    let dir = std::env::temp_dir().join(format!("hcfr-ckpt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bp.bin");
    save_checkpoint(&m, &path).unwrap();
    let m2 = Mccfr::new(&bp, Weighting::Uniform);
    load_checkpoint(&m2, &path).unwrap();
    assert_eq!(m.export(), m2.export());
    assert_eq!(m2.iterations(), 20_000);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn format_1_checkpoints_still_load_and_average_can_be_reset() {
    let bp = blueprint();
    let m = Mccfr::new(&bp, Weighting::Uniform);
    m.run(20_000, 6);
    let (regrets, sums) = m.export();
    // Hand-write the old f32 format.
    let mut bytes = b"HCFRBP01".to_vec();
    bytes.extend(20_000u64.to_le_bytes());
    bytes.extend((regrets.len() as u64).to_le_bytes());
    for x in regrets.iter().chain(&sums) {
        bytes.extend((*x as f32).to_le_bytes());
    }
    let dir = std::env::temp_dir().join(format!("hcfr-v1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.bin");
    std::fs::write(&path, bytes).unwrap();
    let m2 = Mccfr::new(&bp, Weighting::Uniform);
    load_checkpoint(&m2, &path).unwrap();
    let (r2, s2) = m2.export();
    assert!(r2.iter().zip(&regrets).all(|(a, b)| *a == *b as f32 as f64));
    assert!(s2.iter().zip(&sums).all(|(a, b)| *a == *b as f32 as f64));
    assert_eq!(m2.iterations(), 20_000);
    m2.reset_average();
    let (r3, s3) = m2.export();
    assert_eq!(r3, r2, "regrets survive a reset");
    assert!(s3.iter().all(|&x| x == 0.0));
    std::fs::remove_dir_all(dir).unwrap();
}
