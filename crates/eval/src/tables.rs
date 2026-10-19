//! Lookup tables for the fast evaluator, built once at first use (a few ms).
//!
//! Non-flush hands depend only on the multiset of ranks, i.e. a vector of 13
//! counts (each 0..=4) summing to n. We rank those vectors lexicographically with a
//! combinatorial number system, which gives a perfect, gap-free index:
//! 1,287 vectors for n=5, 6,175 for n=6 and 49,205 for n=7. Flush hands depend only
//! on the 13-bit rank mask of the flush suit, so they use a direct 8,192-entry table.

use std::sync::OnceLock;

use crate::reference::{Category, category_of, eval5_ranks};

/// N[m][k]: number of length-m vectors with entries in 0..=4 summing to k.
const fn count_vectors() -> [[u32; 8]; 14] {
    let mut n = [[0u32; 8]; 14];
    n[0][0] = 1;
    let mut m = 1;
    while m <= 13 {
        let mut k = 0;
        while k <= 7 {
            let mut c = 0;
            while c <= 4 && c <= k {
                n[m][k] += n[m - 1][k - c];
                c += 1;
            }
            k += 1;
        }
        m += 1;
    }
    n
}

const N: [[u32; 8]; 14] = count_vectors();

/// STEP[r][k][c]: index contribution of putting count `c` at rank `r` when `k`
/// cards remain to be placed on ranks r..=12. It counts the vectors that agree on
/// ranks below r but put fewer than `c` cards on rank r.
const fn steps() -> [[[u32; 5]; 8]; 13] {
    let mut t = [[[0u32; 5]; 8]; 13];
    let mut r = 0;
    while r < 13 {
        let rest = 12 - r;
        let mut k = 0;
        while k <= 7 {
            let mut c = 1;
            while c <= 4 {
                let mut sum = 0;
                let mut lower = 0;
                while lower < c {
                    if lower <= k {
                        sum += N[rest][k - lower];
                    }
                    lower += 1;
                }
                t[r][k][c] = sum;
                c += 1;
            }
            k += 1;
        }
        r += 1;
    }
    t
}

pub(crate) const STEP: [[[u32; 5]; 8]; 13] = steps();

/// Start of the n-card section in the non-flush table, for n = 5, 6, 7.
pub(crate) const BASE: [u32; 8] = {
    let mut b = [0u32; 8];
    b[6] = N[13][5];
    b[7] = N[13][5] + N[13][6];
    b
};

pub(crate) const NONFLUSH_LEN: usize = (N[13][5] + N[13][6] + N[13][7]) as usize;

/// Perfect index of a packed count vector (4 bits per rank) holding `n` cards.
#[inline(always)]
pub(crate) fn nonflush_index(counts: u64, n: u32) -> usize {
    // A fixed 13-step walk with no early exit: once every card is placed, k is 0
    // and every remaining STEP entry is 0. Branch-free, so it unrolls completely
    // and random hands cost no mispredictions. (A SWAR prefix-sum variant that
    // removes the k dependency chain measured ~20% slower; see the M3 walkthrough.)
    let mut k = n as usize;
    let mut idx = BASE[k];
    for (r, step) in STEP.iter().enumerate() {
        let c = ((counts >> (4 * r)) & 0xF) as usize;
        idx += step[k][c];
        k -= c;
    }
    idx as usize
}

pub(crate) struct Tables {
    pub nonflush: Vec<u16>,
    pub flush: Vec<u16>,
    /// Every distinct 5-card reference value, ascending. Dense rank = position + 1.
    pub classes: Vec<u32>,
}

impl Tables {
    pub fn dense(&self, reference_value: u32) -> u16 {
        let pos = self
            .classes
            .binary_search(&reference_value)
            .expect("reference value is not a 5-card class");
        (pos + 1) as u16
    }
}

pub(crate) fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(build)
}

/// Best reference value over every 5-subset of `ranks`.
fn best_of(ranks: &[u8], flush: bool) -> u32 {
    let n = ranks.len();
    let mut best = 0;
    for skip in 0..(1u32 << n) {
        if (n as u32) - skip.count_ones() != 5 {
            continue;
        }
        let mut five = [0u8; 5];
        let mut j = 0;
        for (i, &r) in ranks.iter().enumerate() {
            if skip & (1 << i) == 0 {
                five[j] = r;
                j += 1;
            }
        }
        best = best.max(eval5_ranks(five, flush));
    }
    best
}

/// Calls `f(counts_packed, ranks)` for every count vector with entries 0..=4 summing to n.
fn for_each_count_vector(n: usize, f: &mut impl FnMut(u64, &[u8])) {
    fn rec(
        r: usize,
        left: usize,
        packed: u64,
        ranks: &mut Vec<u8>,
        f: &mut impl FnMut(u64, &[u8]),
    ) {
        if r == 13 {
            if left == 0 {
                f(packed, ranks);
            }
            return;
        }
        for c in 0..=left.min(4) {
            for _ in 0..c {
                ranks.push(r as u8);
            }
            rec(r + 1, left - c, packed | (c as u64) << (4 * r), ranks, f);
            ranks.truncate(ranks.len() - c);
        }
    }
    rec(0, n, 0, &mut Vec::new(), f);
}

fn build() -> Tables {
    // 1. Every 5-card equivalence class, in strength order.
    let mut classes = Vec::with_capacity(7462);
    for_each_count_vector(5, &mut |_, ranks| {
        classes.push(best_of(ranks, false));
    });
    for mask in 0u32..8192 {
        if mask.count_ones() == 5 {
            let ranks: Vec<u8> = (0..13).filter(|r| mask & (1 << r) != 0).collect();
            classes.push(best_of(&ranks, true));
        }
    }
    classes.sort_unstable();
    classes.dedup();

    let mut t = Tables {
        nonflush: vec![0; NONFLUSH_LEN],
        flush: vec![0; 8192],
        classes,
    };

    // 2. Non-flush table: best 5 of the rank multiset, ignoring suits.
    for n in 5..=7 {
        let mut entries = Vec::new();
        for_each_count_vector(n, &mut |packed, ranks| {
            entries.push((nonflush_index(packed, n as u32), best_of(ranks, false)));
        });
        for (idx, value) in entries {
            debug_assert_eq!(t.nonflush[idx], 0, "index collision");
            t.nonflush[idx] = t.dense(value);
        }
    }

    // 3. Flush table. With at most 7 cards, a flush rules out quads and full houses,
    //    so the best hand is the best 5 of the flush suit's ranks.
    for mask in 0u32..8192 {
        if (5..=7).contains(&mask.count_ones()) {
            let ranks: Vec<u8> = (0..13).filter(|r| mask & (1 << r) != 0).collect();
            t.flush[mask as usize] = t.dense(best_of(&ranks, true));
        }
    }

    debug_assert!(t.classes.iter().is_sorted());
    debug_assert_eq!(category_of(t.classes[0]), Category::HighCard);
    t
}
