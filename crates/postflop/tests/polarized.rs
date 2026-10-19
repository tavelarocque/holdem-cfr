//! Milestone 4 gate, part 1: the textbook polarized river.
//!
//! OOP holds either the nuts (KK, a set) or air (76, seven-high). IP holds only
//! bluff-catchers (QQ), which beat air and lose to the nuts. OOP may check or bet
//! the pot; IP may only check behind. With pot P and bet B, the unique equilibrium
//! frequencies are textbook:
//!
//! - IP calls with probability P / (P + B), which makes air indifferent to bluffing.
//! - OOP's bets are bluffs with probability B / (P + 2B), which makes QQ
//!   indifferent to calling. With 3 KK combos and P = B that is 1.5 bluff combos.
//! - OOP bets every KK.

use postflop::{
    Action, Algorithm, BetConfig, NodeKind, Range, Solver, Spot, SpotConfig, hand_class,
};

fn spot(pot: i32, bet_fraction: f64) -> Spot {
    let stack = 1000;
    let bets = BetConfig::symmetric(&[bet_fraction], &[], 1, false).with_player_bets(1, &[]);
    Spot::new(&SpotConfig {
        board: eval::parse_cards("Kh 9d 5c 3s 2h").unwrap(),
        ranges: [Range::parse("KK,76").unwrap(), Range::parse("QQ").unwrap()],
        pot,
        stack,
        bets,
        isomorphism: true,
    })
}

struct Outcome {
    kk_bet: f64,
    air_bet_combos: f64,
    qq_call: f64,
    exploitability_pct: f64,
    ev_oop: f64,
    ev_ip: f64,
}

fn solve(pot: i32, bet_fraction: f64, iters: u32) -> Outcome {
    let spot = spot(pot, bet_fraction);
    let mut solver = Solver::new(&spot, Algorithm::DCFR);
    for _ in 0..iters {
        solver.iterate();
    }
    let NodeKind::Action {
        actions, children, ..
    } = &spot.tree.nodes[0].kind
    else {
        panic!("root is not an action node")
    };
    assert_eq!(actions[0], Action::Check);
    assert!(matches!(actions[1], Action::Bet(_)));

    let root = solver.average_strategy(0);
    let n0 = spot.num_hands(0);
    let mut kk = (0.0, 0.0);
    let mut air = 0.0;
    for h in 0..n0 {
        let bet = root[n0 + h] as f64;
        match hand_class(spot.combos[0][h]).as_str() {
            "KK" => kk = (kk.0 + bet, kk.1 + 1.0),
            "76s" | "76o" => air += bet,
            c => panic!("unexpected class {c}"),
        }
    }
    let facing = solver.average_strategy(children[1]);
    let n1 = spot.num_hands(1);
    let qq_call = (0..n1).map(|h| facing[n1 + h] as f64).sum::<f64>() / n1 as f64;

    Outcome {
        kk_bet: kk.0 / kk.1,
        air_bet_combos: air,
        qq_call,
        exploitability_pct: solver.exploitability_pct(),
        ev_oop: solver.expected_value(0),
        ev_ip: solver.expected_value(1),
    }
}

#[test]
fn pot_sized_bet_matches_closed_form() {
    let o = solve(100, 1.0, 3000);
    assert_eq!(spot(100, 1.0).num_hands(0), 3 + 16);
    assert!(o.exploitability_pct < 0.05, "{}", o.exploitability_pct);
    assert!((o.kk_bet - 1.0).abs() < 1e-3, "KK bets {}", o.kk_bet);
    // B / (P + 2B) of bets are bluffs: bluffs = value * B / (P + B) = 3 * 1/2.
    assert!(
        (o.air_bet_combos - 1.5).abs() < 0.02,
        "air bets {}",
        o.air_bet_combos
    );
    assert!((o.qq_call - 0.5).abs() < 0.01, "QQ calls {}", o.qq_call);
    // OOP's share of the pot: KK wins 100 + 50% * 100, air wins nothing,
    // weighted 3/19. Utility subtracts OOP's half of the starting pot.
    let share = 3.0 / 19.0 * 150.0;
    assert!((o.ev_oop - (share - 50.0)).abs() < 0.05, "EV {}", o.ev_oop);
    assert!((o.ev_oop + o.ev_ip).abs() < 1e-4, "not zero-sum");
}

#[test]
fn half_pot_bet_matches_closed_form() {
    // P = 100, B = 50: IP calls 100/150 = 2/3; bluffs = 3 * 50/150 = 1 combo.
    let o = solve(100, 0.5, 3000);
    assert!(o.exploitability_pct < 0.05, "{}", o.exploitability_pct);
    assert!((o.kk_bet - 1.0).abs() < 1e-3);
    assert!(
        (o.air_bet_combos - 1.0).abs() < 0.02,
        "air bets {}",
        o.air_bet_combos
    );
    assert!(
        (o.qq_call - 2.0 / 3.0).abs() < 0.01,
        "QQ calls {}",
        o.qq_call
    );
}

#[test]
fn overbet_matches_closed_form() {
    // P = 100, B = 200: IP calls 100/300 = 1/3; bluffs = 3 * 200/300 = 2 combos.
    let o = solve(100, 2.0, 3000);
    assert!(o.exploitability_pct < 0.05, "{}", o.exploitability_pct);
    assert!(
        (o.air_bet_combos - 2.0).abs() < 0.02,
        "air bets {}",
        o.air_bet_combos
    );
    assert!(
        (o.qq_call - 1.0 / 3.0).abs() < 0.01,
        "QQ calls {}",
        o.qq_call
    );
}
