//! M6.1 gate: the hand indexer is a bijection between suit-isomorphism classes
//! and 0..size, checked against brute-force canonicalization.
//!
//! The brute-force canonical form of a hand tries all 24 suit relabellings,
//! sorts the hole cards and the board within themselves, and keeps the smallest
//! encoding. Two hands are isomorphic exactly when their canonical forms match.
//! The indexer is right when equal canonical forms always get equal indices and
//! different canonical forms always get different ones.

#![allow(clippy::needless_range_loop)]

use std::sync::atomic::{AtomicU64, Ordering};

use eval::Card;
use hunl::HandIndexer;

const UNSEEN: u64 = u64::MAX;

fn perms() -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let p = [a, b, c, d];
                    if (0..4u8).all(|x| p.contains(&x)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

fn canonical(perms: &[[u8; 4]], hole: [Card; 2], board: &[Card]) -> u64 {
    let mut best = u64::MAX;
    for p in perms {
        let map = |c: Card| c.rank() * 4 + p[c.suit() as usize];
        let mut h = [map(hole[0]), map(hole[1])];
        h.sort_unstable();
        let mut b: Vec<u8> = board.iter().map(|&c| map(c)).collect();
        b.sort_unstable();
        let mut code = 0u64;
        for x in h.iter().chain(&b) {
            code = code * 64 + *x as u64 + 1;
        }
        best = best.min(code);
    }
    best
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn cards(&mut self, n: usize) -> Vec<Card> {
        let mut deck: Vec<Card> = Card::deck().collect();
        for i in 0..n {
            let j = i + (self.next() % (52 - i) as u64) as usize;
            deck.swap(i, j);
        }
        deck.truncate(n);
        deck
    }
}

#[test]
fn preflop_has_169_classes() {
    let h = HandIndexer::new();
    let p = perms();
    let mut by_index = vec![UNSEEN; 169];
    for a in 0..52u8 {
        for b in 0..a {
            let hole = [Card::from_index(a), Card::from_index(b)];
            let i = h.index(hole, &[]) as usize;
            let c = canonical(&p, hole, &[]);
            assert!(
                by_index[i] == UNSEEN || by_index[i] == c,
                "two classes share index {i}"
            );
            by_index[i] = c;
        }
    }
    assert!(
        by_index.iter().all(|&c| c != UNSEEN),
        "an index was never used"
    );
    let mut distinct = by_index.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 169, "two indices share a class");
}

/// All 25,989,600 (hole, flop) situations: every index in 0..1,286,792 is hit,
/// and each index corresponds to exactly one canonical form.
#[test]
fn flop_index_is_a_bijection_exhaustively() {
    let h = HandIndexer::new();
    let p = perms();
    let size = h.size(1) as usize;
    let by_index: Vec<AtomicU64> = (0..size).map(|_| AtomicU64::new(UNSEEN)).collect();
    let cards: Vec<Card> = Card::deck().collect();
    std::thread::scope(|s| {
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get());
        for t in 0..threads {
            let (h, p, by_index, cards) = (&h, &p, &by_index, &cards);
            s.spawn(move || {
                for a in (t..52).step_by(threads) {
                    for b in 0..a {
                        let hole = [cards[a], cards[b]];
                        for x in 0..52 {
                            for y in x + 1..52 {
                                for z in y + 1..52 {
                                    if [x, y, z].iter().any(|&k| k == a || k == b) {
                                        continue;
                                    }
                                    let board = [cards[x], cards[y], cards[z]];
                                    let i = h.index(hole, &board) as usize;
                                    assert!(i < size);
                                    let c = canonical(p, hole, &board);
                                    let prev = by_index[i].swap(c, Ordering::Relaxed);
                                    assert!(
                                        prev == UNSEEN || prev == c,
                                        "index {i} shared by two classes"
                                    );
                                }
                            }
                        }
                    }
                }
            });
        }
    });
    let mut codes: Vec<u64> = by_index.iter().map(|a| a.load(Ordering::Relaxed)).collect();
    assert!(
        codes.iter().all(|&c| c != UNSEEN),
        "an index was never used"
    );
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), size, "two indices share a class");
}

/// Turn and river: random situations, their random relabellings, and a check
/// that index equality and canonical equality always agree.
#[test]
fn turn_and_river_agree_with_brute_force_on_samples() {
    let h = HandIndexer::new();
    let p = perms();
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for board_len in [4usize, 5] {
        let size = h.size(HandIndexer::street(board_len));
        let mut seen: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        let mut canon_seen: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for _ in 0..400_000 {
            let cards = rng.cards(2 + board_len);
            let hole = [cards[0], cards[1]];
            let board = &cards[2..];
            let i = h.index(hole, board);
            assert!(i < size);
            // A random relabelling with the hole and board shuffled must agree.
            let perm = p[(rng.next() % 24) as usize];
            let map = |c: Card| Card::new(c.rank(), perm[c.suit() as usize]);
            let mut b2: Vec<Card> = board.iter().rev().map(|&c| map(c)).collect();
            b2.rotate_left((rng.next() % board_len as u64) as usize);
            assert_eq!(h.index([map(hole[1]), map(hole[0])], &b2), i);
            // Same index <=> same canonical form, across all samples so far.
            let c = canonical(&p, hole, board);
            assert_eq!(
                *seen.entry(i).or_insert(c),
                c,
                "index {i} shared by two classes"
            );
            assert_eq!(
                *canon_seen.entry(c).or_insert(i),
                i,
                "one class got two indices"
            );
        }
        // Enough samples landed in the same class twice for the check to bite.
        assert!(seen.len() < 400_000);
    }
}
