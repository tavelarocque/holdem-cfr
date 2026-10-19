//! Cards and card parsing.

use std::fmt;
use std::str::FromStr;

pub const RANK_CHARS: [char; 13] = [
    '2', '3', '4', '5', '6', '7', '8', '9', 'T', 'J', 'Q', 'K', 'A',
];
pub const SUIT_CHARS: [char; 4] = ['c', 'd', 'h', 's'];

/// A playing card, encoded as `rank * 4 + suit` with rank 0 = deuce .. 12 = ace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Card(u8);

impl Card {
    pub const fn new(rank: u8, suit: u8) -> Card {
        assert!(rank < 13 && suit < 4);
        Card(rank * 4 + suit)
    }

    /// The card with index `i` in `0..52`.
    pub const fn from_index(i: u8) -> Card {
        assert!(i < 52);
        Card(i)
    }

    pub const fn index(self) -> u8 {
        self.0
    }

    pub const fn rank(self) -> u8 {
        self.0 / 4
    }

    pub const fn suit(self) -> u8 {
        self.0 % 4
    }

    /// All 52 cards in index order.
    pub fn deck() -> impl Iterator<Item = Card> {
        (0..52).map(Card)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            RANK_CHARS[self.rank() as usize],
            SUIT_CHARS[self.suit() as usize]
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCardError(pub String);

impl fmt::Display for ParseCardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid card {:?}", self.0)
    }
}

impl std::error::Error for ParseCardError {}

impl FromStr for Card {
    type Err = ParseCardError;

    /// Parses cards like `As`, `td`, `2C`.
    fn from_str(s: &str) -> Result<Card, ParseCardError> {
        let err = || ParseCardError(s.to_string());
        let mut chars = s.chars();
        let (r, su) = (chars.next().ok_or_else(err)?, chars.next().ok_or_else(err)?);
        if chars.next().is_some() {
            return Err(err());
        }
        let rank = RANK_CHARS
            .iter()
            .position(|&c| c == r.to_ascii_uppercase())
            .ok_or_else(err)?;
        let suit = SUIT_CHARS
            .iter()
            .position(|&c| c == su.to_ascii_lowercase())
            .ok_or_else(err)?;
        Ok(Card::new(rank as u8, suit as u8))
    }
}

/// Parses a run of cards such as `"AsKd"` or `"As Kd 7c"`.
pub fn parse_cards(s: &str) -> Result<Vec<Card>, ParseCardError> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.len().is_multiple_of(2) {
        return Err(ParseCardError(s.to_string()));
    }
    compact
        .as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap().parse())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_every_card() {
        for c in Card::deck() {
            assert_eq!(c.to_string().parse::<Card>().unwrap(), c);
        }
    }

    #[test]
    fn parse_runs_and_reject_garbage() {
        let cards = parse_cards("As kd 7C").unwrap();
        assert_eq!(
            cards,
            vec![Card::new(12, 3), Card::new(11, 1), Card::new(5, 0)]
        );
        assert!("Ax".parse::<Card>().is_err());
        assert!("1s".parse::<Card>().is_err());
        assert!(parse_cards("AsK").is_err());
    }
}
