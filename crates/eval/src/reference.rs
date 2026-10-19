//! A deliberately simple, slow 5-card evaluator.
//!
//! It is the ground truth everything else is checked against, so it favours
//! obviousness over speed: sort, count, classify. Values are `u32`s that compare
//! correctly (higher is better) but are sparse; [`crate::HandRank`] is the dense
//! 1..=7462 version.

use crate::card::Card;

/// Hand categories, weakest first. The discriminant is the category code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    HighCard = 0,
    OnePair = 1,
    TwoPair = 2,
    ThreeOfAKind = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    FourOfAKind = 7,
    StraightFlush = 8,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::HighCard,
        Category::OnePair,
        Category::TwoPair,
        Category::ThreeOfAKind,
        Category::Straight,
        Category::Flush,
        Category::FullHouse,
        Category::FourOfAKind,
        Category::StraightFlush,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::HighCard => "high card",
            Category::OnePair => "one pair",
            Category::TwoPair => "two pair",
            Category::ThreeOfAKind => "three of a kind",
            Category::Straight => "straight",
            Category::Flush => "flush",
            Category::FullHouse => "full house",
            Category::FourOfAKind => "four of a kind",
            Category::StraightFlush => "straight flush",
        }
    }
}

/// Category of a sparse reference value.
pub fn category_of(value: u32) -> Category {
    Category::ALL[(value >> 20) as usize]
}

/// Slow value of five ranks (0 = deuce .. 12 = ace), with or without a flush.
///
/// Layout: `category << 20 | k0 << 16 | k1 << 12 | k2 << 8 | k3 << 4 | k4`, where the
/// kickers are ranks ordered by (multiplicity desc, rank desc). For straights the
/// only kicker is the top card, and the wheel A-2-3-4-5 counts as five-high.
pub fn eval5_ranks(ranks: [u8; 5], flush: bool) -> u32 {
    let mut counts = [0u8; 13];
    for &r in &ranks {
        counts[r as usize] += 1;
    }
    // (count, rank) pairs sorted by count desc then rank desc.
    let mut groups: Vec<(u8, u8)> = (0..13u8)
        .rev()
        .filter(|&r| counts[r as usize] > 0)
        .map(|r| (counts[r as usize], r))
        .collect();
    groups.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    let straight_top = if groups.len() == 5 {
        let hi = groups[0].1;
        let lo = groups[4].1;
        if hi - lo == 4 {
            Some(hi)
        } else if hi == 12 && groups[1].1 == 3 {
            Some(3) // wheel: A 5 4 3 2
        } else {
            None
        }
    } else {
        None
    };

    let shape: Vec<u8> = groups.iter().map(|g| g.0).collect();
    let category = match (straight_top, flush, shape.as_slice()) {
        (Some(_), true, _) => Category::StraightFlush,
        (_, _, [4, 1]) => Category::FourOfAKind,
        (_, _, [3, 2]) => Category::FullHouse,
        (_, true, _) => Category::Flush,
        (Some(_), false, _) => Category::Straight,
        (_, _, [3, 1, 1]) => Category::ThreeOfAKind,
        (_, _, [2, 2, 1]) => Category::TwoPair,
        (_, _, [2, 1, 1, 1]) => Category::OnePair,
        (_, _, [1, 1, 1, 1, 1]) => Category::HighCard,
        _ => unreachable!("impossible 5-card shape {shape:?}"),
    };

    let mut value = (category as u32) << 20;
    if let Some(top) =
        straight_top.filter(|_| matches!(category, Category::Straight | Category::StraightFlush))
    {
        value |= (top as u32) << 16;
    } else {
        for (i, g) in groups.iter().enumerate() {
            value |= (g.1 as u32) << (16 - 4 * i);
        }
    }
    value
}

/// Slow value of exactly five cards.
pub fn eval5(cards: [Card; 5]) -> u32 {
    let flush = cards.iter().all(|c| c.suit() == cards[0].suit());
    eval5_ranks(cards.map(|c| c.rank()), flush)
}

/// Slow value of the best five-card hand among 5 to 7 cards, by brute force.
pub fn eval_best(cards: &[Card]) -> u32 {
    let n = cards.len();
    assert!((5..=7).contains(&n), "need 5 to 7 cards, got {n}");
    let mut best = 0;
    for a in 0..n {
        for b in a + 1..n {
            for c in b + 1..n {
                for d in c + 1..n {
                    for e in d + 1..n {
                        let v = eval5([cards[a], cards[b], cards[c], cards[d], cards[e]]);
                        best = best.max(v);
                    }
                }
            }
        }
    }
    best
}
