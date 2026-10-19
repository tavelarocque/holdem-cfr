//! Random legal play never breaks the rules' invariants.

use hunl::{Action, Params, State};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn random_hands_respect_invariants(choices in prop::collection::vec((0u8..4, 0.0f64..1.0), 1..60), stack in 150i32..30_000) {
        let params = Params { small_blind: 50, big_blind: 100, stack };
        let mut s = State::new(params);
        let mut steps = 0;
        for (kind, frac) in choices {
            if s.is_terminal() {
                break;
            }
            let mut options = vec![];
            for a in [Action::Fold, Action::Check, Action::Call] {
                if s.legal(a) {
                    options.push(a);
                }
            }
            if let Some((lo, hi)) = s.raise_range() {
                options.push(Action::RaiseTo(lo + ((hi - lo) as f64 * frac) as i32));
            }
            prop_assert!(!options.is_empty(), "no legal action at {}", s);
            let a = options[kind as usize % options.len()];
            let before = s.contributed;
            s = s.apply(a).unwrap();
            steps += 1;
            // Contributions never shrink and never exceed the stack.
            for (now, was) in s.contributed.iter().zip(before) {
                prop_assert!(*now >= was);
                prop_assert!(*now <= stack);
            }
            prop_assert!(s.street <= 4);
        }
        prop_assert!(steps <= 60);
        if s.is_terminal() {
            let cards = eval::parse_cards("As Ad Ks Kd 2c 7h 9d Jc 3s").unwrap();
            let pay = s.payoff([[cards[0], cards[1]], [cards[2], cards[3]]], &cards[4..]);
            prop_assert_eq!(pay[0] + pay[1], 0, "not zero-sum");
            prop_assert!(pay[0].abs() <= stack);
        }
    }
}
