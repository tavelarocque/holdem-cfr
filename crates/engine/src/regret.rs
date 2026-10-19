//! Regret matching: turning accumulated regrets into a strategy.

/// Writes the regret-matching strategy for `regrets` into `out`.
///
/// Each action is played in proportion to its positive regret. If no action has
/// positive regret, play uniformly.
pub fn regret_matching(regrets: &[f64], out: &mut [f64]) {
    debug_assert_eq!(regrets.len(), out.len());
    let positive_sum: f64 = regrets.iter().map(|&r| r.max(0.0)).sum();
    if positive_sum > 0.0 {
        for (o, &r) in out.iter_mut().zip(regrets) {
            *o = r.max(0.0) / positive_sum;
        }
    } else {
        let uniform = 1.0 / out.len() as f64;
        out.fill(uniform);
    }
}

/// Normalizes accumulated strategy weights into a probability distribution.
/// Falls back to uniform when nothing has been accumulated yet.
pub fn normalize(weights: &[f64], out: &mut [f64]) {
    let total: f64 = weights.iter().sum();
    if total > 0.0 {
        for (o, &w) in out.iter_mut().zip(weights) {
            *o = w / total;
        }
    } else {
        let uniform = 1.0 / out.len() as f64;
        out.fill(uniform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rock_paper_scissors_example() {
        // Played rock, opponent played paper: regrets are rock 0, paper 1, scissors 2.
        let mut s = [0.0; 3];
        regret_matching(&[0.0, 1.0, 2.0], &mut s);
        assert_eq!(s, [0.0, 1.0 / 3.0, 2.0 / 3.0]);
    }

    #[test]
    fn no_positive_regret_is_uniform() {
        let mut s = [0.0; 4];
        regret_matching(&[-1.0, 0.0, -3.0, 0.0], &mut s);
        assert_eq!(s, [0.25; 4]);
    }

    proptest! {
        #[test]
        fn output_is_a_distribution(regrets in prop::collection::vec(-1e6f64..1e6, 1..8)) {
            let mut s = vec![0.0; regrets.len()];
            regret_matching(&regrets, &mut s);
            let sum: f64 = s.iter().sum();
            prop_assert!((sum - 1.0).abs() < 1e-9);
            prop_assert!(s.iter().all(|&p| (0.0..=1.0).contains(&p)));
            // Actions with non-positive regret get zero probability unless all do.
            if regrets.iter().any(|&r| r > 0.0) {
                for (p, r) in s.iter().zip(&regrets) {
                    if *r <= 0.0 { prop_assert_eq!(*p, 0.0); }
                }
            }
        }
    }
}
