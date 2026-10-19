//! Perfect hand-isomorphism indexing.
//!
//! Two hold'em situations are strategically identical if a relabelling of the
//! suits turns one into the other: (A♠K♠ on Q♠7♥2♦) and (A♥K♥ on Q♥7♠2♦) play
//! the same. An [`Indexer`] maps every situation to a dense index such that
//! isomorphic situations get the same index, and the index space has no gaps.
//! The sizes are classic numbers: 169 preflop, 1,286,792 on the flop,
//! 13,960,050 on the turn and 123,156,254 on the river (Waugh 2013).
//!
//! **How it works.** Look at one suit at a time. A suit's contribution is a
//! *signature*: which ranks of that suit appear in each round (hole cards, flop,
//! turn, river). A suit permutation only shuffles which suit carries which
//! signature, so the situation up to isomorphism is exactly the *multiset* of
//! four signatures. The index is built in three layers:
//!
//! 1. A suit's *shape* is how many of its cards fall in each round, e.g. (1, 2, 0)
//!    for one hole card and two flop cards. Within a shape, the signature is ranked
//!    with a mixed-radix product of colex ranks, one per round.
//! 2. The four shapes, sorted, form a *configuration*. Suits with the same shape
//!    are interchangeable, so their signatures are ranked as a multiset
//!    (combinations with repetition).
//! 3. Configurations are laid out back to back, each at its own offset.
//!
//! Every layer is a bijection, so the whole map is.

use std::collections::HashMap;

use eval::Card;

/// Cards dealt in each round of hold'em: hole, flop, turn, river.
pub const ROUND_SIZES: [usize; 4] = [2, 3, 1, 1];

/// Binomial coefficients up to C(64, k).
fn binom(n: u64, k: u64) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut r = 1u64;
    for i in 0..k {
        r = r * (n - i) / (i + 1);
    }
    r
}

/// Colex rank of a set of positions: sum of C(p_i, i) for p_1 < p_2 < ...
fn colex(mut mask: u32) -> u64 {
    let mut rank = 0;
    let mut i = 1;
    while mask != 0 {
        let p = mask.trailing_zeros() as u64;
        rank += binom(p, i);
        i += 1;
        mask &= mask - 1;
    }
    rank
}

/// Removes the bits of `used` from `mask`, packing the remaining positions down.
fn compress(mask: u32, used: u32) -> u32 {
    let mut out = 0;
    let mut bit = 0;
    for p in 0..13 {
        if used & (1 << p) != 0 {
            continue;
        }
        if mask & (1 << p) != 0 {
            out |= 1 << bit;
        }
        bit += 1;
    }
    out
}

/// Cards of one suit per round: a count vector, padded to 4 rounds.
type Shape = [u8; 4];

struct Group {
    shape: Shape,
    /// How many suits share this shape.
    suits: usize,
    /// Number of signatures one suit of this shape can have.
    signatures: u64,
    /// Number of multisets: C(signatures + suits - 1, suits).
    size: u64,
}

struct Config {
    offset: u64,
    groups: Vec<Group>,
}

/// Indexes situations whose cards arrive in rounds of the given sizes, e.g.
/// `[2, 3, 1]` for hole cards, flop and turn with the deal order remembered, or
/// `[2, 4]` for hole cards and a 4-card board treated as one set.
pub struct Indexer {
    sizes: Vec<usize>,
    rounds: usize,
    configs: Vec<Config>,
    /// Configuration id by packed key of the sorted shapes (dense table).
    lookup: Vec<u32>,
    /// Mixed radix for packing one suit's shape.
    shape_radix: [u64; 4],
    /// Number of distinct packed shapes per suit.
    shape_space: u64,
    size: u64,
}

impl Indexer {
    pub fn new(sizes: &[usize]) -> Indexer {
        let rounds = sizes.len();
        assert!((1..=4).contains(&rounds) && sizes.iter().sum::<usize>() <= 7);
        // Every way to distribute each round's cards over 4 suits, sorted.
        let mut shapes: Vec<[Shape; 4]> = Vec::new();
        fn compositions(n: u8, parts: usize, cur: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
            if parts == 1 {
                cur.push(n);
                out.push(cur.clone());
                cur.pop();
                return;
            }
            for a in 0..=n {
                cur.push(a);
                compositions(n - a, parts - 1, cur, out);
                cur.pop();
            }
        }
        fn assign(sizes: &[usize], round: usize, cur: &mut [Shape; 4], out: &mut Vec<[Shape; 4]>) {
            if round == sizes.len() {
                let mut sorted = *cur;
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                out.push(sorted);
                return;
            }
            // Every way to split this round's cards over the 4 suits.
            let mut splits = Vec::new();
            compositions(sizes[round] as u8, 4, &mut Vec::new(), &mut splits);
            for parts in splits {
                let fits = (0..4).all(|s| {
                    let used: u8 = cur[s][..round].iter().sum();
                    used + parts[s] <= 13
                });
                if !fits {
                    continue;
                }
                for s in 0..4 {
                    cur[s][round] = parts[s];
                }
                assign(sizes, round + 1, cur, out);
                for s in cur.iter_mut() {
                    s[round] = 0;
                }
            }
        }
        assign(sizes, 0, &mut [[0; 4]; 4], &mut shapes);
        shapes.sort_unstable();
        shapes.dedup();

        let mut configs: HashMap<[Shape; 4], Config> = HashMap::new();
        let mut offset = 0;
        for cfg in shapes {
            let mut groups: Vec<Group> = Vec::new();
            for &shape in &cfg {
                if let Some(g) = groups.last_mut().filter(|g| g.shape == shape) {
                    g.suits += 1;
                    continue;
                }
                let mut signatures = 1;
                let mut used = 0u64;
                for &c in &shape[..rounds] {
                    signatures *= binom(13 - used, c as u64);
                    used += c as u64;
                }
                groups.push(Group {
                    shape,
                    suits: 1,
                    signatures,
                    size: 0,
                });
            }
            let mut size = 1;
            for g in &mut groups {
                g.size = binom(g.signatures + g.suits as u64 - 1, g.suits as u64);
                size *= g.size;
            }
            configs.insert(cfg, Config { offset, groups });
            offset += size;
        }
        // Pack each sorted configuration into a dense key so lookups are one
        // array read instead of a hash.
        let mut shape_radix = [0u64; 4];
        let mut r = 1u64;
        for (i, &n) in sizes.iter().enumerate() {
            shape_radix[i] = r;
            r *= n as u64 + 1;
        }
        let shape_space = r;
        let pack = |cfg: &[Shape; 4]| -> u64 {
            cfg.iter().fold(0, |key, sh| {
                let code: u64 = (0..rounds).map(|i| sh[i] as u64 * shape_radix[i]).sum();
                key * shape_space + code
            })
        };
        let table_len = shape_space.pow(4) as usize;
        assert!(table_len <= 1 << 23, "shape table too large");
        let mut lookup = vec![u32::MAX; table_len];
        let mut list = Vec::with_capacity(configs.len());
        for (cfg, c) in configs {
            lookup[pack(&cfg) as usize] = list.len() as u32;
            list.push(c);
        }
        Indexer {
            sizes: sizes.to_vec(),
            rounds,
            configs: list,
            lookup,
            shape_radix,
            shape_space,
            size: offset,
        }
    }

    /// Number of distinct indices.
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn rounds(&self) -> usize {
        self.rounds
    }

    /// Index of a situation. `rounds[r]` holds the cards dealt in round r; only
    /// the first `self.rounds()` entries are used and each must have the right size.
    pub fn index(&self, rounds: &[&[Card]]) -> u64 {
        debug_assert!(rounds.len() >= self.rounds);
        // Per suit: rank mask per round, shape and signature rank.
        let mut masks = [[0u32; 4]; 4];
        for (r, cards) in rounds.iter().take(self.rounds).enumerate() {
            debug_assert_eq!(cards.len(), self.sizes[r]);
            for c in cards.iter() {
                masks[c.suit() as usize][r] |= 1 << c.rank();
            }
        }
        let mut suits: [(Shape, u64); 4] = [([0; 4], 0); 4];
        for s in 0..4 {
            let mut shape = [0u8; 4];
            let mut sig = 0u64;
            let mut radix = 1u64;
            let mut used = 0u32;
            let mut avail = 13u64;
            for r in 0..self.rounds {
                let m = masks[s][r];
                debug_assert_eq!(m & used, 0, "duplicate card");
                let c = m.count_ones() as u64;
                shape[r] = c as u8;
                sig += colex(compress(m, used)) * radix;
                radix *= binom(avail, c);
                avail -= c;
                used |= m;
            }
            suits[s] = (shape, sig);
        }
        // Configuration: shapes sorted descending; ties broken by signature so that
        // each group's signatures come out grouped.
        suits.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let key = [suits[0].0, suits[1].0, suits[2].0, suits[3].0];
        let packed = key.iter().fold(0u64, |acc, sh| {
            let code: u64 = (0..self.rounds)
                .map(|i| sh[i] as u64 * self.shape_radix[i])
                .sum();
            acc * self.shape_space + code
        });
        let cfg = &self.configs[self.lookup[packed as usize] as usize];

        let mut index = 0u64;
        let mut radix = 1u64;
        let mut s = 0;
        for g in &cfg.groups {
            // Signatures of this group's suits, ascending: multiset rank.
            let mut rank = 0u64;
            for i in 0..g.suits {
                let x = suits[s + i].1;
                rank += binom(x + i as u64, i as u64 + 1);
            }
            s += g.suits;
            index += rank * radix;
            radix *= g.size;
        }
        cfg.offset + index
    }
}

/// Index of hole cards plus a board of 0, 3, 4 or 5 cards, with the board
/// treated as one set (the deal order of board cards is forgotten). This is the
/// imperfect-recall view used for card abstraction: 169, 1,286,792, 13,960,050
/// and 123,156,254 classes per street.
pub struct HandIndexer {
    indexers: [Indexer; 4],
}

impl Default for HandIndexer {
    fn default() -> Self {
        Self::new()
    }
}

impl HandIndexer {
    pub fn new() -> HandIndexer {
        HandIndexer {
            indexers: [
                Indexer::new(&[2]),
                Indexer::new(&[2, 3]),
                Indexer::new(&[2, 4]),
                Indexer::new(&[2, 5]),
            ],
        }
    }

    /// Street of a board length: 0 preflop, 1 flop, 2 turn, 3 river.
    pub fn street(board_len: usize) -> usize {
        match board_len {
            0 => 0,
            3 => 1,
            4 => 2,
            5 => 3,
            n => panic!("invalid board length {n}"),
        }
    }

    pub fn size(&self, street: usize) -> u64 {
        self.indexers[street].size()
    }

    pub fn index(&self, hole: [Card; 2], board: &[Card]) -> u64 {
        let street = Self::street(board.len());
        self.indexers[street].index(&[&hole, board])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binomials_and_colex() {
        assert_eq!(binom(13, 2), 78);
        assert_eq!(binom(52, 5), 2_598_960);
        assert_eq!(colex(0b1), 0);
        assert_eq!(colex(0b11), 0);
        assert_eq!(colex(0b101), 1);
        assert_eq!(colex(0b110), 2);
        assert_eq!(compress(0b1010, 0b0010), 0b100);
    }

    #[test]
    fn perfect_recall_sizes_match_waugh() {
        // Board cards remembered in deal order: rounds of 2, 3, 1, 1.
        let sizes: Vec<u64> = (1..=4)
            .map(|r| Indexer::new(&ROUND_SIZES[..r]).size())
            .collect();
        assert_eq!(sizes, vec![169, 1_286_792, 55_190_538, 2_428_287_420]);
    }

    #[test]
    fn board_as_set_sizes_match_waugh() {
        let h = HandIndexer::new();
        let sizes: Vec<u64> = (0..4).map(|s| h.size(s)).collect();
        assert_eq!(sizes, vec![169, 1_286_792, 13_960_050, 123_156_254]);
    }
}
