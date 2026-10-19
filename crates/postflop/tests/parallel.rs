//! Milestone 5 gate: parallelism and suit isomorphism change speed, not answers.

use postflop::solver::required_memory;
use postflop::{Algorithm, BetConfig, NodeKind, Range, Solver, Spot, SpotConfig};

const OOP: &str = "22+,A2s+,K9s+,Q9s+,J9s+,T8s+,97s+,87s,76s,65s,ATo+,KTo+,QTo+,JTo";
const IP: &str = "22+,A2s+,K7s+,Q8s+,J8s+,T8s+,97s+,86s+,75s+,64s+,54s,A8o+,KTo+,QTo+,JTo,T9o";

fn turn_spot(board: &str, oop: &str, ip: &str, isomorphism: bool) -> Spot {
    Spot::new(&SpotConfig {
        board: eval::parse_cards(board).unwrap(),
        ranges: [Range::parse(oop).unwrap(), Range::parse(ip).unwrap()],
        pot: 100,
        stack: 250,
        bets: BetConfig::symmetric(&[0.5], &[1.0], 2, true).with_street(2, &[0.75], &[]),
        isomorphism,
    })
}

/// Number of distinct cards dealt at the first chance node below the root.
fn first_chance_classes(spot: &Spot) -> (usize, usize) {
    for node in &spot.tree.nodes {
        if let NodeKind::Chance { cards, members, .. } = &node.kind {
            return (cards.len(), members.iter().map(Vec::len).sum());
        }
    }
    panic!("no chance node");
}

fn run_in_pool(threads: usize, spot: &Spot, iters: u32) -> (Vec<Vec<u32>>, u64) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| {
        let mut solver = Solver::new(spot, Algorithm::DCFR);
        for _ in 0..iters {
            solver.iterate();
        }
        let bits = solver
            .regrets()
            .iter()
            .map(|r| r.iter().map(|x| x.to_bits()).collect())
            .collect();
        (bits, solver.exploitability().to_bits())
    })
}

#[test]
fn parallel_is_bit_identical_to_serial() {
    let spot = turn_spot("Qs Jh 7c 4d", OOP, IP, true);
    let (serial, e1) = run_in_pool(1, &spot, 6);
    let (parallel, e4) = run_in_pool(4, &spot, 6);
    assert!(serial == parallel, "regrets differ between 1 and 4 threads");
    assert_eq!(e1, e4, "exploitability differs between 1 and 4 threads");
}

#[test]
fn rainbow_turn_has_no_symmetry() {
    let spot = turn_spot("Qs Jh 7c 4d", OOP, IP, true);
    assert_eq!(spot.symmetries.len(), 1);
    assert_eq!(first_chance_classes(&spot), (48, 48));
}

#[test]
fn two_tone_turn_merges_the_missing_suits() {
    // Hearts and diamonds are absent: swapping them fixes the board and the
    // class-based ranges, so each heart river merges with its diamond twin.
    let spot = turn_spot("Qs Js 7c 4c", OOP, IP, true);
    assert_eq!(spot.symmetries.len(), 2);
    assert_eq!(first_chance_classes(&spot), (35, 48));
}

#[test]
fn asymmetric_ranges_disable_merging() {
    // A specific heart combo breaks the heart/diamond symmetry.
    let spot = turn_spot("Qs Js 7c 4c", &format!("{OOP},AhKh"), IP, true);
    let spot2 = turn_spot("Qs Js 7c 4c", OOP, "AhKh,QQ,JJ,T9s", true);
    assert_eq!(
        spot.symmetries.len(),
        2,
        "AhKh is already in AKs: still symmetric"
    );
    assert_eq!(spot2.symmetries.len(), 1, "a lone AhKh is not symmetric");
    assert_eq!(first_chance_classes(&spot2), (48, 48));
}

#[test]
fn isomorphic_solve_matches_full_solve() {
    let iso = turn_spot("Qs Js 7c 4c", OOP, IP, true);
    let full = turn_spot("Qs Js 7c 4c", OOP, IP, false);
    assert!(required_memory(&iso) < required_memory(&full) * 3 / 4);
    let mut a = Solver::new(&iso, Algorithm::DCFR);
    let mut b = Solver::new(&full, Algorithm::DCFR);
    for i in 1..=40 {
        a.iterate();
        b.iterate();
        if i == 1 || i == 40 {
            for p in 0..2 {
                let (x, y) = (a.best_response_value(p), b.best_response_value(p));
                assert!((x - y).abs() < 1e-3, "iter {i} BR{p}: iso {x} vs full {y}");
                let (x, y) = (a.expected_value(p), b.expected_value(p));
                assert!((x - y).abs() < 1e-3, "iter {i} EV{p}: iso {x} vs full {y}");
            }
        }
    }
    assert_eq!(a.memory_bytes(), required_memory(&iso));
}

#[test]
fn monotone_flop_merges_three_suits() {
    let spot = Spot::new(&SpotConfig {
        board: eval::parse_cards("Ks Qs 7s").unwrap(),
        ranges: [
            Range::parse("AA,KK,AKs,QJs").unwrap(),
            Range::parse("QQ,JJ,ATs,T9s").unwrap(),
        ],
        pot: 100,
        stack: 100,
        bets: BetConfig::symmetric(&[0.5], &[], 1, false),
        isomorphism: true,
    });
    assert_eq!(spot.symmetries.len(), 6);
    // 10 spades left, plus one class per rank for hearts, diamonds and clubs.
    assert_eq!(first_chance_classes(&spot), (23, 49));
}
