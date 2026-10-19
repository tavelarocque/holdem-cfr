//! Hand-strength tables for every isomorphism class on every street.
//!
//! **Hand strength (HS)** on the river is the probability of beating a uniformly
//! random opponent hand, counting ties as half: (wins + ties / 2) / 990, since
//! 45 unseen cards make C(45, 2) = 990 opponent hands. Earlier streets use
//! expectations over the cards still to come: **E[HS]** (equity against a random
//! hand) and **E[HS²]**, which rewards hands whose strength *varies*, such as
//! draws, the way bucketing papers recommend (Johanson 2007).
//!
//! The tables are built bottom-up so that no work is repeated:
//!
//! - **River.** For each of the 134,459 distinct 5-card boards (up to suit
//!   isomorphism), sort all 1,081 possible hands by strength and sweep once,
//!   using the per-card running totals from the postflop solver to discount
//!   opponent hands that share a card. That gives HS for every river class.
//! - **Turn.** E[HS] and E[HS²] average the river table over the 46 rivers.
//! - **Flop.** Average the turn table over the 47 turns (law of total expectation).
//! - **Preflop.** Average the flop table over all 19,600 flops.

use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};

use eval::{Card, Hand};
use rayon::prelude::*;

use crate::iso::{HandIndexer, Indexer};

/// E[HS] and E[HS²] for each class of one street.
#[derive(Clone, Debug, Default)]
pub struct StreetStrength {
    pub ehs: Vec<f32>,
    pub ehs2: Vec<f32>,
}

/// Strength tables for all four streets, indexed by [`HandIndexer`] classes.
pub struct StrengthTables {
    pub indexer: HandIndexer,
    pub preflop: StreetStrength,
    pub flop: StreetStrength,
    pub turn: StreetStrength,
    /// River hand strength, quantized to u16 (value / 65535).
    pub river: Vec<u16>,
}

fn quantize(x: f32) -> u16 {
    (x.clamp(0.0, 1.0) * 65535.0).round() as u16
}

pub fn dequantize(q: u16) -> f32 {
    q as f32 / 65535.0
}

/// One representative board per suit-isomorphism class of `k`-card boards.
pub fn canonical_boards(k: usize) -> Vec<Vec<Card>> {
    let indexer = Indexer::new(&[k]);
    let mut seen = vec![false; indexer.size() as usize];
    let mut out = Vec::with_capacity(indexer.size() as usize);
    let mut cur = Vec::with_capacity(k);
    fn rec(start: u8, k: usize, cur: &mut Vec<Card>, f: &mut impl FnMut(&[Card])) {
        if cur.len() == k {
            f(cur);
            return;
        }
        for c in start..52 {
            cur.push(Card::from_index(c));
            rec(c + 1, k, cur, f);
            cur.pop();
        }
    }
    rec(0, k, &mut cur, &mut |b| {
        let i = indexer.index(&[b]) as usize;
        if !seen[i] {
            seen[i] = true;
            out.push(b.to_vec());
        }
    });
    out
}

/// All two-card hands not touching `board`.
pub fn holes_off(board: &[Card]) -> Vec<[Card; 2]> {
    let mut out = Vec::with_capacity(1326);
    for a in 0..52u8 {
        for b in 0..a {
            let (x, y) = (Card::from_index(a), Card::from_index(b));
            if !board.contains(&x) && !board.contains(&y) {
                out.push([x, y]);
            }
        }
    }
    out
}

/// River hand strength of every hand on a 5-card board, in the order of
/// [`holes_off`]. One sort plus two linear sweeps.
pub fn river_strengths(board: &[Card]) -> Vec<f32> {
    debug_assert_eq!(board.len(), 5);
    let holes = holes_off(board);
    let base = Hand::from_cards(board);
    let ranks: Vec<u16> = holes.iter().map(|h| base.with(h).rank().0).collect();
    let mut order: Vec<usize> = (0..holes.len()).collect();
    order.sort_unstable_by_key(|&i| ranks[i]);

    let opponents = 990.0; // C(45, 2)
    let mut out = vec![0f32; holes.len()];
    // Sweep ascending: strictly weaker hands (wins) and weaker-or-equal (wins + ties).
    let mut below = [0u32; 52];
    let mut below_total = 0u32;
    let mut i = 0;
    while i < order.len() {
        // Group of equal rank.
        let r = ranks[order[i]];
        let mut j = i;
        while j < order.len() && ranks[order[j]] == r {
            j += 1;
        }
        // Hands in the group *including* the group, for ties.
        let mut group = [0u32; 52];
        for &k in &order[i..j] {
            let [a, b] = holes[k];
            group[a.index() as usize] += 1;
            group[b.index() as usize] += 1;
        }
        let group_total = (j - i) as u32;
        for &k in &order[i..j] {
            let [a, b] = holes[k];
            let (a, b) = (a.index() as usize, b.index() as usize);
            let wins = below_total - below[a] - below[b];
            // Equal-rank hands sharing no card, excluding the hand itself
            // (it shares both cards, so it is subtracted twice and added back once).
            let ties = group_total + 1 - group[a] - group[b];
            out[k] = (wins as f32 + 0.5 * ties as f32) / opponents;
        }
        for c in 0..52 {
            below[c] += group[c];
        }
        below_total += group_total;
        i = j;
    }
    out
}

fn atomic_f32_vec(n: usize) -> Vec<AtomicU32> {
    (0..n).map(|_| AtomicU32::new(0)).collect()
}

fn unwrap_f32(v: Vec<AtomicU32>) -> Vec<f32> {
    v.into_iter()
        .map(|a| f32::from_bits(a.into_inner()))
        .collect()
}

impl StrengthTables {
    /// Builds every table. Uses all cores; prints progress through `log`.
    pub fn build(log: &(dyn Fn(&str) + Sync)) -> StrengthTables {
        let indexer = HandIndexer::new();
        let t0 = std::time::Instant::now();

        // River.
        let river_boards = canonical_boards(5);
        let river: Vec<AtomicU16> = (0..indexer.size(3)).map(|_| AtomicU16::new(0)).collect();
        river_boards.par_iter().for_each(|board| {
            let hs = river_strengths(board);
            for (hole, &x) in holes_off(board).iter().zip(&hs) {
                let i = indexer.index(*hole, board) as usize;
                river[i].store(quantize(x), Ordering::Relaxed);
            }
        });
        let river: Vec<u16> = river.into_iter().map(AtomicU16::into_inner).collect();
        log(&format!(
            "river: {} boards, {} classes ({:.1}s)",
            river_boards.len(),
            river.len(),
            t0.elapsed().as_secs_f64()
        ));

        // Turn: average river HS and HS² over the 46 river cards.
        let n = indexer.size(2) as usize;
        let (ehs, ehs2) = (atomic_f32_vec(n), atomic_f32_vec(n));
        canonical_boards(4).par_iter().for_each(|board| {
            let mut five = board.clone();
            five.push(board[0]);
            for hole in holes_off(board) {
                let (mut s, mut s2) = (0f64, 0f64);
                let mut count = 0;
                for r in Card::deck() {
                    if board.contains(&r) || hole.contains(&r) {
                        continue;
                    }
                    five[4] = r;
                    let hs = dequantize(river[indexer.index(hole, &five) as usize]) as f64;
                    s += hs;
                    s2 += hs * hs;
                    count += 1;
                }
                let i = indexer.index(hole, board) as usize;
                ehs[i].store(((s / count as f64) as f32).to_bits(), Ordering::Relaxed);
                ehs2[i].store(((s2 / count as f64) as f32).to_bits(), Ordering::Relaxed);
            }
        });
        let turn = StreetStrength {
            ehs: unwrap_f32(ehs),
            ehs2: unwrap_f32(ehs2),
        };
        log(&format!(
            "turn: {} classes ({:.1}s)",
            n,
            t0.elapsed().as_secs_f64()
        ));

        // Flop: average the turn table over the 47 turn cards.
        let n = indexer.size(1) as usize;
        let (ehs, ehs2) = (atomic_f32_vec(n), atomic_f32_vec(n));
        canonical_boards(3).par_iter().for_each(|board| {
            let mut four = board.clone();
            four.push(board[0]);
            for hole in holes_off(board) {
                let (mut s, mut s2) = (0f64, 0f64);
                let mut count = 0;
                for t in Card::deck() {
                    if board.contains(&t) || hole.contains(&t) {
                        continue;
                    }
                    four[3] = t;
                    let j = indexer.index(hole, &four) as usize;
                    s += turn.ehs[j] as f64;
                    s2 += turn.ehs2[j] as f64;
                    count += 1;
                }
                let i = indexer.index(hole, board) as usize;
                ehs[i].store(((s / count as f64) as f32).to_bits(), Ordering::Relaxed);
                ehs2[i].store(((s2 / count as f64) as f32).to_bits(), Ordering::Relaxed);
            }
        });
        let flop = StreetStrength {
            ehs: unwrap_f32(ehs),
            ehs2: unwrap_f32(ehs2),
        };
        log(&format!(
            "flop: {} classes ({:.1}s)",
            n,
            t0.elapsed().as_secs_f64()
        ));

        // Preflop: average the flop table over every flop.
        let mut reps: Vec<Option<[Card; 2]>> = vec![None; 169];
        for hole in holes_off(&[]) {
            let i = indexer.index(hole, &[]) as usize;
            reps[i].get_or_insert(hole);
        }
        let results: Vec<(f32, f32)> = reps
            .par_iter()
            .map(|hole| {
                let hole = hole.expect("every preflop class has a hand");
                let (mut s, mut s2) = (0f64, 0f64);
                let mut count = 0;
                let live: Vec<Card> = Card::deck().filter(|c| !hole.contains(c)).collect();
                for a in 0..live.len() {
                    for b in a + 1..live.len() {
                        for c in b + 1..live.len() {
                            let j = indexer.index(hole, &[live[a], live[b], live[c]]) as usize;
                            s += flop.ehs[j] as f64;
                            s2 += flop.ehs2[j] as f64;
                            count += 1;
                        }
                    }
                }
                ((s / count as f64) as f32, (s2 / count as f64) as f32)
            })
            .collect();
        let preflop = StreetStrength {
            ehs: results.iter().map(|r| r.0).collect(),
            ehs2: results.iter().map(|r| r.1).collect(),
        };
        log(&format!(
            "preflop: 169 classes ({:.1}s)",
            t0.elapsed().as_secs_f64()
        ));

        StrengthTables {
            indexer,
            preflop,
            flop,
            turn,
            river,
        }
    }

    /// E[HS] of a hand on a board of 0, 3, 4 or 5 cards.
    pub fn ehs(&self, hole: [Card; 2], board: &[Card]) -> f32 {
        let i = self.indexer.index(hole, board) as usize;
        match board.len() {
            0 => self.preflop.ehs[i],
            3 => self.flop.ehs[i],
            4 => self.turn.ehs[i],
            _ => dequantize(self.river[i]),
        }
    }

    /// E[HS²] of a hand on a board of 0, 3, 4 or 5 cards.
    pub fn ehs2(&self, hole: [Card; 2], board: &[Card]) -> f32 {
        let i = self.indexer.index(hole, board) as usize;
        match board.len() {
            0 => self.preflop.ehs2[i],
            3 => self.flop.ehs2[i],
            4 => self.turn.ehs2[i],
            _ => dequantize(self.river[i]).powi(2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_class_counts() {
        assert_eq!(canonical_boards(3).len(), 1_755);
        assert_eq!(canonical_boards(4).len(), 16_432);
        assert_eq!(canonical_boards(5).len(), 134_459);
    }

    /// Brute force: compare against all 990 opponents with the evaluator.
    fn brute_hs(hole: [Card; 2], board: &[Card]) -> f32 {
        let base = Hand::from_cards(board);
        let me = base.with(&hole).rank();
        let mut score = 0.0;
        let mut n = 0;
        for opp in holes_off(board) {
            if opp.iter().any(|c| hole.contains(c)) {
                continue;
            }
            let them = base.with(&opp).rank();
            score += match me.cmp(&them) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Less => 0.0,
            };
            n += 1;
        }
        assert_eq!(n, 990);
        score / 990.0
    }

    #[test]
    fn river_sweep_matches_brute_force() {
        for b in [
            "As Ks Qs Js 2d", // royal draws, board straight
            "7c 7d 7h 2s 2c", // full house on board: many ties
            "2c 5d 9h Jc Kd",
            "Ah Kh Qh Jh Th", // royal on board: every hand ties
        ] {
            let board = eval::parse_cards(b).unwrap();
            let fast = river_strengths(&board);
            for (k, hole) in holes_off(&board).iter().enumerate().step_by(7) {
                let slow = brute_hs(*hole, &board);
                assert!(
                    (fast[k] - slow).abs() < 1e-6,
                    "{b} {hole:?}: {} vs {slow}",
                    fast[k]
                );
            }
        }
        let royal = eval::parse_cards("Ah Kh Qh Jh Th").unwrap();
        assert!(
            river_strengths(&royal)
                .iter()
                .all(|&x| (x - 0.5).abs() < 1e-6)
        );
    }
}
