//! Prints random 7-card hands with their dense rank, for cross-checking other evaluators.
use eval::{Card, Hand};
fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..n {
        let mut deck: Vec<Card> = Card::deck().collect();
        for i in 0..7 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let j = i + (s % (52 - i) as u64) as usize;
            deck.swap(i, j);
        }
        let cards: String = deck[..7].iter().map(|c| c.to_string()).collect();
        println!("{cards} {}", Hand::from_cards(&deck[..7]).rank().0);
    }
}
