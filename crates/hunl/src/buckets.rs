//! Card abstraction: map every hand-board class to one of a few buckets per street.
//!
//! Preflop keeps all 169 classes (lossless). Flop and turn hands are bucketed by
//! E[HS²]; river hands by HS. Buckets are *percentile* buckets: thresholds are
//! quantiles of the metric over uniformly random deals, so every bucket is
//! reached about equally often in real play, regardless of how many isomorphism
//! classes it happens to contain. The abstraction has imperfect recall: a hand's
//! bucket on the turn does not remember its flop bucket.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use eval::Card;

use crate::iso::HandIndexer;
use crate::strength::{StrengthTables, dequantize};

const MAGIC: &[u8; 8] = b"HCFRBKT1";

pub struct Buckets {
    pub indexer: HandIndexer,
    /// Number of buckets per street (preflop, flop, turn, river).
    pub counts: [u16; 4],
    /// `tables[street][class]` is the class's bucket.
    pub tables: [Vec<u8>; 4],
}

/// Deterministic xorshift for sampling deals.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn deal(&mut self, n: usize) -> Vec<Card> {
        let mut deck: Vec<Card> = Card::deck().collect();
        for i in 0..n {
            let j = i + (self.next() % (52 - i) as u64) as usize;
            deck.swap(i, j);
        }
        deck.truncate(n);
        deck
    }
}

/// `k - 1` thresholds splitting `values` into `k` equally populated groups.
pub fn quantile_thresholds(mut values: Vec<f32>, k: usize) -> Vec<f32> {
    values.sort_unstable_by(f32::total_cmp);
    (1..k).map(|i| values[i * values.len() / k]).collect()
}

/// Bucket of `x` given ascending thresholds.
pub fn bucket_of(thresholds: &[f32], x: f32) -> u8 {
    thresholds.partition_point(|&t| t <= x) as u8
}

impl Buckets {
    /// The flop, turn and river percentile thresholds that [`Buckets::from_strength`]
    /// uses, placed with `samples` random deals per street. Deterministic: the
    /// same tables, counts and samples always give the same thresholds.
    pub fn thresholds(t: &StrengthTables, counts: [u16; 3], samples: usize) -> [Vec<f32>; 3] {
        assert!(counts.iter().all(|&c| (2..=256).contains(&c)));
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        std::array::from_fn(|i| {
            let street = i + 1;
            let board_len = street + 2;
            let sample: Vec<f32> = (0..samples)
                .map(|_| {
                    let cards = rng.deal(2 + board_len);
                    let class = t.indexer.index([cards[0], cards[1]], &cards[2..]) as usize;
                    Self::metric(t, street, class)
                })
                .collect();
            quantile_thresholds(sample, counts[i] as usize)
        })
    }

    /// The bucketing metric: E[HS²] on the flop and turn, HS on the river.
    pub fn metric(t: &StrengthTables, street: usize, class: usize) -> f32 {
        match street {
            1 => t.flop.ehs2[class],
            2 => t.turn.ehs2[class],
            _ => dequantize(t.river[class]),
        }
    }

    /// Percentile buckets from strength tables, with `samples` random deals per
    /// street used to place the thresholds.
    pub fn from_strength(t: &StrengthTables, counts: [u16; 3], samples: usize) -> Buckets {
        let th = Self::thresholds(t, counts, samples);
        let indexer = HandIndexer::new();
        let preflop: Vec<u8> = (0..169u8).collect();
        let mut tables = [preflop, Vec::new(), Vec::new(), Vec::new()];
        for street in 1..4 {
            let n = indexer.size(street) as usize;
            tables[street] = (0..n)
                .map(|c| bucket_of(&th[street - 1], Self::metric(t, street, c)))
                .collect();
        }
        Buckets {
            indexer,
            counts: [169, counts[0], counts[1], counts[2]],
            tables,
        }
    }

    /// Bucket of a hand on a board of 0, 3, 4 or 5 cards.
    pub fn bucket(&self, hole: [Card; 2], board: &[Card]) -> u8 {
        let street = HandIndexer::street(board.len());
        self.tables[street][self.indexer.index(hole, board) as usize]
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut w = BufWriter::new(File::create(path)?);
        w.write_all(MAGIC)?;
        for s in 0..4 {
            w.write_all(&self.counts[s].to_le_bytes())?;
            w.write_all(&(self.tables[s].len() as u64).to_le_bytes())?;
            w.write_all(&self.tables[s])?;
        }
        w.flush()
    }

    pub fn load(path: &Path) -> io::Result<Buckets> {
        let mut r = BufReader::new(File::open(path)?);
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a bucket file",
            ));
        }
        let indexer = HandIndexer::new();
        let mut counts = [0u16; 4];
        let mut tables: [Vec<u8>; 4] = Default::default();
        for s in 0..4 {
            let mut c = [0u8; 2];
            r.read_exact(&mut c)?;
            counts[s] = u16::from_le_bytes(c);
            let mut n = [0u8; 8];
            r.read_exact(&mut n)?;
            let n = u64::from_le_bytes(n);
            if n != indexer.size(s) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "table size mismatch",
                ));
            }
            tables[s] = vec![0; n as usize];
            r.read_exact(&mut tables[s])?;
        }
        Ok(Buckets {
            indexer,
            counts,
            tables,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_split_evenly() {
        let values: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let th = quantile_thresholds(values.clone(), 4);
        assert_eq!(th, vec![0.25, 0.5, 0.75]);
        let mut counts = [0; 4];
        for v in values {
            counts[bucket_of(&th, v) as usize] += 1;
        }
        assert_eq!(counts, [250; 4]);
    }
}
