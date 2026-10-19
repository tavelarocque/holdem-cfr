//! Milestone 3 gate: the evaluator is correct.
//!
//! The reference evaluator is checked against known combinatorial counts, and
//! the fast evaluator is checked against the reference: on every 5-card hand, on
//! a large random sample of 6- and 7-card hands, and (in `--ignored` mode) on all
//! 133,784,560 seven-card hands. The full 7-card category histogram, which pins
//! down the fast evaluator independently of the reference, runs in CI.

// Nested index loops are the clearest way to write "every k-subset of 52".
#![allow(clippy::needless_range_loop)]

use std::collections::HashSet;

use eval::reference::{category_of, eval_best, eval5};
use eval::{Card, Category, Hand, HandRank, dense_rank};

const CARDS: [Card; 52] = {
    let mut d = [Card::from_index(0); 52];
    let mut i = 0;
    while i < 52 {
        d[i] = Card::from_index(i as u8);
        i += 1;
    }
    d
};

/// Known counts of 5-card hands per category (of 2,598,960).
const FIVE_CARD_COUNTS: [u64; 9] = [
    1_302_540, 1_098_240, 123_552, 54_912, 10_200, 5_108, 3_744, 624, 40,
];
/// Known number of distinct equivalence classes per category (7,462 total).
const FIVE_CARD_CLASSES: [usize; 9] = [1277, 2860, 858, 858, 10, 1277, 156, 156, 10];
/// Known counts of 7-card hands per category, by best five (of 133,784,560).
const SEVEN_CARD_COUNTS: [u64; 9] = [
    23_294_460, 58_627_800, 31_433_400, 6_461_620, 6_180_020, 4_047_644, 3_473_184, 224_848, 41_584,
];

/// Deterministic xorshift so failures reproduce.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn hand(&mut self, n: usize) -> Vec<Card> {
        let mut deck = CARDS;
        for i in 0..n {
            let j = i + (self.next() % (52 - i) as u64) as usize;
            deck.swap(i, j);
        }
        deck[..n].to_vec()
    }
}

#[test]
fn reference_matches_known_five_card_counts() {
    let mut counts = [0u64; 9];
    let mut classes: [HashSet<u32>; 9] = Default::default();
    for a in 0..52 {
        for b in a + 1..52 {
            for c in b + 1..52 {
                for d in c + 1..52 {
                    for e in d + 1..52 {
                        let v = eval5([CARDS[a], CARDS[b], CARDS[c], CARDS[d], CARDS[e]]);
                        let cat = category_of(v) as usize;
                        counts[cat] += 1;
                        classes[cat].insert(v);
                    }
                }
            }
        }
    }
    assert_eq!(counts, FIVE_CARD_COUNTS);
    let class_counts: Vec<usize> = classes.iter().map(|s| s.len()).collect();
    assert_eq!(class_counts, FIVE_CARD_CLASSES);
}

#[test]
fn fast_matches_reference_on_every_five_card_hand() {
    let mut seen = HashSet::new();
    for a in 0..52 {
        for b in a + 1..52 {
            for c in b + 1..52 {
                for d in c + 1..52 {
                    for e in d + 1..52 {
                        let five = [CARDS[a], CARDS[b], CARDS[c], CARDS[d], CARDS[e]];
                        let fast = Hand::from_cards(&five).rank();
                        let slow = eval5(five);
                        assert_eq!(fast, dense_rank(slow), "{five:?}");
                        assert_eq!(fast.category(), category_of(slow));
                        seen.insert(fast);
                    }
                }
            }
        }
    }
    assert_eq!(seen.len(), 7462);
    assert_eq!(seen.iter().min(), Some(&HandRank::WORST));
    assert_eq!(seen.iter().max(), Some(&HandRank::BEST));
}

#[test]
fn fast_matches_brute_force_on_random_six_and_seven_card_hands() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for n in [6, 7] {
        for _ in 0..300_000 {
            let cards = rng.hand(n);
            let fast = Hand::from_cards(&cards).rank();
            assert_eq!(fast, dense_rank(eval_best(&cards)), "{cards:?}");
        }
    }
}

/// Calls `f` on every 7-card hand whose lowest card is `first`, building hands
/// incrementally the way a real enumeration would.
fn for_each_seven_starting_at(first: usize, f: &mut impl FnMut(Hand)) {
    let h1 = Hand::EMPTY.plus(CARDS[first]);
    for b in first + 1..52 {
        let h2 = h1.plus(CARDS[b]);
        for c in b + 1..52 {
            let h3 = h2.plus(CARDS[c]);
            for d in c + 1..52 {
                let h4 = h3.plus(CARDS[d]);
                for e in d + 1..52 {
                    let h5 = h4.plus(CARDS[e]);
                    for g in e + 1..52 {
                        let h6 = h5.plus(CARDS[g]);
                        for &card in &CARDS[g + 1..] {
                            f(h6.plus(card));
                        }
                    }
                }
            }
        }
    }
}

/// Runs `per_first` for each lowest card on all available threads and merges.
fn parallel_over_first_card<T: Send>(
    per_first: impl Fn(usize) -> T + Sync,
    merge: impl Fn(T, T) -> T,
    init: T,
) -> T {
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<T> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let first = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if first > 45 {
                            break;
                        }
                        out.push(per_first(first));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    results.into_iter().fold(init, merge)
}

#[test]
fn all_seven_card_hands_have_known_category_counts() {
    let (counts, seen) = parallel_over_first_card(
        |first| {
            let mut counts = [0u64; 9];
            let mut seen = vec![false; 7463];
            for_each_seven_starting_at(first, &mut |h| {
                let r = h.rank();
                counts[r.category() as usize] += 1;
                seen[r.0 as usize] = true;
            });
            (counts, seen)
        },
        |(mut c1, mut s1), (c2, s2)| {
            for i in 0..9 {
                c1[i] += c2[i];
            }
            for (a, b) in s1.iter_mut().zip(s2) {
                *a |= b;
            }
            (c1, s1)
        },
        ([0u64; 9], vec![false; 7463]),
    );
    assert_eq!(counts.iter().sum::<u64>(), 133_784_560);
    assert_eq!(counts, SEVEN_CARD_COUNTS);
    // Only 4,824 of the 7,462 classes can be the best five of seven cards.
    assert_eq!(seen.iter().filter(|&&s| s).count(), 4824);
    assert_eq!(Category::ALL.len(), counts.len());
}

/// Every 7-card hand against the brute-force reference. About 2.8 billion
/// reference evaluations, so it is opt-in:
/// `cargo test --release -p eval -- --ignored`
#[test]
#[ignore]
fn fast_matches_brute_force_on_all_seven_card_hands() {
    let mismatches = parallel_over_first_card(
        |first| {
            let mut bad = 0u64;
            let mut cards = Vec::with_capacity(7);
            for_each_seven_starting_at(first, &mut |h| {
                cards.clear();
                cards.extend(CARDS.iter().copied().filter(|&c| h.contains(c)));
                if h.rank() != dense_rank(eval_best(&cards)) {
                    bad += 1;
                }
            });
            bad
        },
        |a, b| a + b,
        0,
    );
    assert_eq!(mismatches, 0);
}
