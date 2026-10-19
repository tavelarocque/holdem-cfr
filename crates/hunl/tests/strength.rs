//! M6.2 gate (opt-in, about 4 minutes on 2 cores, about 1 on 8):
//! `cargo test --release -p hunl --test strength -- --ignored`
//!
//! Builds every strength table from scratch and checks the result against
//! published preflop equities versus a random hand, plus bucket balance.

use eval::{Card, parse_cards};
use hunl::{Buckets, StrengthTables};

/// Published all-in equity versus one random hand.
const ANCHORS: [(&str, f32); 8] = [
    ("AsAh", 0.8520),
    ("KsKh", 0.8240),
    ("QsQh", 0.7993),
    ("AsKs", 0.6704),
    ("AsKh", 0.6532),
    ("Ts9s", 0.5403),
    ("7s2h", 0.3458),
    ("3s2h", 0.3230),
];

#[test]
#[ignore]
fn full_tables_match_published_equities_and_buckets_balance() {
    let t = StrengthTables::build(&|m: &str| eprintln!("{m}"));
    for (hand, equity) in ANCHORS {
        let c = parse_cards(hand).unwrap();
        let got = t.ehs([c[0], c[1]], &[]);
        assert!((got - equity).abs() < 0.0006, "{hand}: {got} vs {equity}");
        // Suit-isomorphic versions agree.
        let swapped: Vec<Card> = c
            .iter()
            .map(|x| Card::new(x.rank(), (x.suit() + 1) % 4))
            .collect();
        assert_eq!(t.ehs([swapped[0], swapped[1]], &[]), got);
    }
    // E[HS^2] >= E[HS]^2 everywhere (Jensen), checked on the flop table.
    for (m, s) in t.flop.ehs.iter().zip(&t.flop.ehs2) {
        assert!(*s + 1e-5 >= m * m);
    }

    let b = Buckets::from_strength(&t, [50, 50, 50], 1_000_000);
    // Fresh random deals land in each bucket about 2% of the time.
    let mut state = 0x1234_5678_9ABC_DEF1u64;
    for street in 1..4usize {
        let mut counts = [0u32; 50];
        let n = 200_000;
        for _ in 0..n {
            let mut deck: Vec<Card> = Card::deck().collect();
            for i in 0..street + 4 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let j = i + (state % (52 - i) as u64) as usize;
                deck.swap(i, j);
            }
            counts[b.bucket([deck[0], deck[1]], &deck[2..street + 4]) as usize] += 1;
        }
        let expected = n as f64 / 50.0;
        for (k, &c) in counts.iter().enumerate() {
            // River ties make a few buckets lumpy; everything stays within 2x.
            assert!(
                (c as f64) > expected * 0.5 && (c as f64) < expected * 2.0,
                "street {street} bucket {k}: {c} of {n}"
            );
        }
    }
}
