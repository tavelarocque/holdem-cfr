//! Fast 5-, 6- and 7-card poker hand evaluation.
//!
//! ```
//! use eval::{Hand, parse_cards};
//! let board = Hand::from_cards(&parse_cards("Ah Kh Qh 7c 2d").unwrap());
//! let nuts = board.with(&parse_cards("Jh Th").unwrap()).rank();
//! let set = board.with(&parse_cards("7s 7d").unwrap()).rank();
//! assert!(nuts > set);
//! assert_eq!(nuts.category(), eval::Category::StraightFlush);
//! ```
//!
//! A [`Hand`] is a small `Copy` value that cards are added to incrementally, so a
//! shared board is folded in once and each player's hole cards are added on top.
//! [`Hand::rank`] does at most four popcounts and one flush-table lookup, or a
//! perfect-hash walk over 13 packed rank counts and one non-flush lookup. Both
//! tables together are about 125 KB and stay in cache.

pub mod card;
pub mod reference;
mod tables;

use std::fmt;

pub use card::{Card, ParseCardError, parse_cards};
pub use reference::Category;

/// Strength of the best five-card hand: 1 (7-5-4-3-2 offsuit) to 7462 (royal flush).
/// Higher is better; equal ranks split the pot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandRank(pub u16);

/// First dense rank of each category, weakest first.
const CATEGORY_START: [u16; 9] = [1, 1278, 4138, 4996, 5854, 5864, 7141, 7297, 7453];

impl HandRank {
    pub const WORST: HandRank = HandRank(1);
    pub const BEST: HandRank = HandRank(7462);

    pub fn category(self) -> Category {
        let i = CATEGORY_START.partition_point(|&s| s <= self.0) - 1;
        Category::ALL[i]
    }
}

impl fmt::Display for HandRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.category().name(), self.0)
    }
}

/// A set of up to 7 cards, built incrementally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hand {
    /// Bit `16 * suit + rank` is set for each card held.
    suits: u64,
    /// Four bits per rank holding how many cards of that rank are held.
    counts: u64,
    /// One byte per suit holding `3 + cards of that suit`, so a suit reaching five
    /// cards sets bit 3 of its byte and one AND detects any flush.
    suit_counts: u32,
    len: u32,
}

impl Default for Hand {
    fn default() -> Hand {
        Hand::EMPTY
    }
}

impl Hand {
    pub const EMPTY: Hand = Hand {
        suits: 0,
        counts: 0,
        suit_counts: 0x0303_0303,
        len: 0,
    };

    pub fn from_cards(cards: &[Card]) -> Hand {
        Hand::EMPTY.with(cards)
    }

    /// This hand plus one card. The card must not already be in the hand.
    #[inline(always)]
    #[must_use]
    pub fn plus(self, card: Card) -> Hand {
        let bit = 1u64 << (16 * card.suit() as u32 + card.rank() as u32);
        debug_assert!(self.suits & bit == 0, "duplicate card {card}");
        debug_assert!(self.len < 7, "more than 7 cards");
        Hand {
            suits: self.suits | bit,
            counts: self.counts + (1u64 << (4 * card.rank() as u32)),
            suit_counts: self.suit_counts + (1u32 << (8 * card.suit() as u32)),
            len: self.len + 1,
        }
    }

    /// This hand plus several cards.
    #[must_use]
    pub fn with(self, cards: &[Card]) -> Hand {
        cards.iter().fold(self, |h, &c| h.plus(c))
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether `card` is in the hand.
    pub fn contains(&self, card: Card) -> bool {
        self.suits & (1u64 << (16 * card.suit() as u32 + card.rank() as u32)) != 0
    }

    /// Strength of the best five cards. The hand must hold 5, 6 or 7 cards.
    #[inline]
    pub fn rank(&self) -> HandRank {
        debug_assert!(
            (5..=7).contains(&self.len),
            "need 5-7 cards, have {}",
            self.len
        );
        let t = tables::tables();
        let flush = self.suit_counts & 0x0808_0808;
        if flush != 0 {
            let suit = flush.trailing_zeros() / 8;
            let mask = (self.suits >> (16 * suit)) & 0x1FFF;
            return HandRank(t.flush[mask as usize]);
        }
        HandRank(t.nonflush[tables::nonflush_index(self.counts, self.len)])
    }
}

/// Strength of the best five of 5 to 7 cards.
pub fn evaluate(cards: &[Card]) -> HandRank {
    Hand::from_cards(cards).rank()
}

/// Dense rank of a sparse [`reference`] value. Used to compare against the reference.
pub fn dense_rank(reference_value: u32) -> HandRank {
    HandRank(tables::tables().dense(reference_value))
}

/// Forces the lookup tables to be built now rather than on first use.
pub fn warm_up() {
    tables::tables();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> HandRank {
        evaluate(&parse_cards(s).unwrap())
    }

    #[test]
    fn extremes() {
        assert_eq!(r("7c 5d 4h 3s 2c"), HandRank::WORST);
        assert_eq!(r("As Ks Qs Js Ts"), HandRank::BEST);
        assert_eq!(r("As Ks Qs Js Ts 2c 2d"), HandRank::BEST);
    }

    #[test]
    fn wheel_is_the_lowest_straight() {
        let wheel = r("Ac 2d 3h 4s 5c");
        let six_high = r("2d 3h 4s 5c 6d");
        assert_eq!(wheel.category(), Category::Straight);
        assert!(wheel < six_high);
        // Ace-high card is not a straight with K-Q-J-T missing one.
        assert_eq!(r("Ac Kd Qh Js 9c").category(), Category::HighCard);
        // No wrap-around straight.
        assert_eq!(r("Qc Kd Ah 2s 3c").category(), Category::HighCard);
    }

    #[test]
    fn steel_wheel_is_the_lowest_straight_flush() {
        let steel = r("Ah 2h 3h 4h 5h");
        assert_eq!(steel.category(), Category::StraightFlush);
        assert_eq!(steel.0, 7453);
        assert!(steel > r("Ac Ad Ah As Kc"));
        // A 7-card hand holding both a wheel flush and a 6-high straight flush.
        assert_eq!(r("Ah 2h 3h 4h 5h 6h 9c").0, 7454);
    }

    #[test]
    fn kickers_decide() {
        assert!(r("Ac Ad Kh Qs Jc") > r("Ac Ad Kh Qs Tc"));
        assert!(r("Kc Kd Qh Qs 3c") > r("Kc Kd Qh Qs 2c"));
        assert!(r("9c 9d 9h As 2c") > r("9c 9d 9h Ks Qc"));
        assert!(r("Ah Jh 9h 6h 3h") > r("Ah Jh 9h 6h 2h"));
    }

    #[test]
    fn split_pot_when_the_board_plays() {
        let board = "As Ks Qd Jc Th";
        assert_eq!(r(&format!("{board} 2c 3d")), r(&format!("{board} 4h 5c")));
        // Two players with the same two pair and the board kicker.
        assert_eq!(r("Ac Kd 8h 8s 4c 4d 2h"), r("Ad Kh 8c 8d 4h 4s 3c"));
    }

    #[test]
    fn seven_card_shapes() {
        // Three pairs: best two pairs plus the best remaining kicker.
        assert_eq!(r("Ac Ad Kh Ks Qc Qd 2h"), r("Ac Ad Kh Ks Qc 3d 2h"));
        // Two sets make a full house, using the higher set.
        let h = r("9c 9d 9h 5s 5c 5d Ah");
        assert_eq!(h.category(), Category::FullHouse);
        assert_eq!(h, r("9c 9d 9h 5s 5c 2d 3h"));
        // Quads with trips: kicker is the highest other card.
        assert_eq!(r("8c 8d 8h 8s Kc Kd Kh"), r("8c 8d 8h 8s Kc 2d 3h"));
        // Flush beats straight in the same seven cards.
        assert_eq!(r("4h 5h 6c 7h 8d 9h Kh").category(), Category::Flush);
    }

    #[test]
    fn incremental_matches_batch() {
        let board = Hand::from_cards(&parse_cards("2c 7d 9h Js Qc").unwrap());
        let hole = parse_cards("Tc 8s").unwrap();
        assert_eq!(board.with(&hole).rank(), r("2c 7d 9h Js Qc Tc 8s"));
        assert_eq!(board.with(&hole).rank().category(), Category::Straight);
        assert!(board.contains("Js".parse().unwrap()));
        assert!(!board.contains("Jc".parse().unwrap()));
    }
}
