//! A postflop spot: public tree plus both players' ranges, with the per-board data
//! the solver needs for fast terminal evaluation.

use eval::{Card, Hand};

use crate::range::{NUM_COMBOS, Range, combo_cards, combo_index};
use crate::tree::{BetConfig, IDENTITY, NodeKind, PublicTree, SuitPerm, permute_card};

const NONE: u32 = u32::MAX;

/// Hands of one player that survive the starting board, sorted by strength on a
/// particular river board.
#[derive(Clone, Debug)]
pub(crate) struct Showdown {
    /// (rank, hand index), ascending by rank. Hands blocked by the board are absent.
    pub sorted: [Vec<(u16, u32)>; 2],
}

/// Everything needed to describe a subgame.
#[derive(Clone, Debug)]
pub struct SpotConfig {
    pub board: Vec<Card>,
    /// `ranges[0]` is OOP, `ranges[1]` is IP.
    pub ranges: [Range; 2],
    pub pot: i32,
    pub stack: i32,
    pub bets: BetConfig,
    /// Merge chance cards that are identical up to a suit permutation. Only
    /// permutations that preserve the board and both ranges are used, so this is
    /// exact; it saves work on boards missing two or more suits.
    pub isomorphism: bool,
}

#[derive(Debug)]
pub struct Spot {
    pub tree: PublicTree,
    /// Each player's hands: the combos with positive weight not blocked by the board.
    pub hands: [Vec<(Card, Card)>; 2],
    /// Combo index (0..1326) of each hand.
    pub combos: [Vec<usize>; 2],
    pub weights: [Vec<f32>; 2],
    /// `same[p][h]`: index of the identical combo in the other player's list.
    pub(crate) same: [Vec<u32>; 2],
    /// `with_card[p][c]`: player p's hands containing card c.
    pub(crate) with_card: [Vec<Vec<u32>>; 2],
    /// Showdown data per board id (only for 5-card boards).
    pub(crate) showdowns: Vec<Option<Showdown>>,
    /// Sum over compatible hand pairs of w0 * w1: normalizes values.
    pub norm: f64,
    /// Suit permutations preserving the starting board and both ranges;
    /// `symmetries[0]` is the identity.
    pub symmetries: Vec<SuitPerm>,
    /// `inverse[g][p][h]`: index of the hand that symmetry g maps onto hand h.
    pub(crate) inverse: Vec<[Vec<u32>; 2]>,
}

impl Spot {
    pub fn new(cfg: &SpotConfig) -> Spot {
        let mut hands: [Vec<(Card, Card)>; 2] = Default::default();
        let mut combos: [Vec<usize>; 2] = Default::default();
        let mut weights: [Vec<f32>; 2] = Default::default();
        for p in 0..2 {
            for i in 0..NUM_COMBOS {
                let w = cfg.ranges[p].weights[i];
                let (a, b) = combo_cards(i);
                if w > 0.0 && !cfg.board.contains(&a) && !cfg.board.contains(&b) {
                    hands[p].push((a, b));
                    combos[p].push(i);
                    weights[p].push(w);
                }
            }
            assert!(
                !hands[p].is_empty(),
                "player {p} has no hands left on this board"
            );
        }

        let mut same: [Vec<u32>; 2] = Default::default();
        for p in 0..2 {
            let mut pos = vec![NONE; NUM_COMBOS];
            for (h, &c) in combos[1 - p].iter().enumerate() {
                pos[c] = h as u32;
            }
            same[p] = combos[p].iter().map(|&c| pos[c]).collect();
        }

        // Symmetries: suit permutations fixing the board (as a set) under which every
        // hand of each player maps to a hand of the same player with the same weight.
        let mut symmetries = vec![IDENTITY];
        let mut inverse: Vec<[Vec<u32>; 2]> = Vec::new();
        let index_of = |p: usize| {
            let mut pos = vec![NONE; NUM_COMBOS];
            for (h, &c) in combos[p].iter().enumerate() {
                pos[c] = h as u32;
            }
            pos
        };
        let pos = [index_of(0), index_of(1)];
        let identity_inverse: [Vec<u32>; 2] =
            std::array::from_fn(|p| (0..hands[p].len() as u32).collect());
        inverse.push(identity_inverse);
        if cfg.isomorphism {
            for perm in all_suit_perms().into_iter().filter(|&q| q != IDENTITY) {
                let fixes_board = cfg
                    .board
                    .iter()
                    .all(|&c| cfg.board.contains(&permute_card(&perm, c)));
                if !fixes_board {
                    continue;
                }
                let mut inv: [Vec<u32>; 2] = Default::default();
                let mut ok = true;
                for p in 0..2 {
                    inv[p] = vec![NONE; hands[p].len()];
                    for (h, &(a, b)) in hands[p].iter().enumerate() {
                        let image = combo_index(permute_card(&perm, a), permute_card(&perm, b));
                        let j = pos[p][image];
                        if j == NONE || weights[p][j as usize] != weights[p][h] {
                            ok = false;
                            break;
                        }
                        inv[p][j as usize] = h as u32;
                    }
                    if !ok {
                        break;
                    }
                }
                if ok {
                    symmetries.push(perm);
                    inverse.push(inv);
                }
            }
        }
        let tree =
            PublicTree::build_with_symmetry(&cfg.board, cfg.pot, cfg.stack, &cfg.bets, &symmetries);

        let mut with_card: [Vec<Vec<u32>>; 2] = [vec![Vec::new(); 52], vec![Vec::new(); 52]];
        for p in 0..2 {
            for (h, &(a, b)) in hands[p].iter().enumerate() {
                with_card[p][a.index() as usize].push(h as u32);
                with_card[p][b.index() as usize].push(h as u32);
            }
        }

        let mut showdowns = vec![None; tree.boards.len()];
        for node in &tree.nodes {
            if matches!(node.kind, NodeKind::Showdown) && showdowns[node.board].is_none() {
                let board = &tree.boards[node.board];
                let base = Hand::from_cards(board);
                let sorted = std::array::from_fn(|p| {
                    let mut v: Vec<(u16, u32)> = hands[p]
                        .iter()
                        .enumerate()
                        .filter(|(_, (a, b))| !board.contains(a) && !board.contains(b))
                        .map(|(h, &(a, b))| (base.plus(a).plus(b).rank().0, h as u32))
                        .collect();
                    v.sort_unstable();
                    v
                });
                showdowns[node.board] = Some(Showdown { sorted });
            }
        }

        let mut spot = Spot {
            tree,
            hands,
            combos,
            weights,
            same,
            with_card,
            showdowns,
            norm: 0.0,
            symmetries,
            inverse,
        };
        let mut compat = vec![0.0; spot.hands[0].len()];
        spot.compatible_sums(0, &spot.weights[1], &mut compat);
        spot.norm = spot.weights[0]
            .iter()
            .zip(&compat)
            .map(|(&w, &c)| w as f64 * c as f64)
            .sum();
        assert!(spot.norm > 0.0, "the ranges have no compatible hand pairs");
        spot
    }

    pub fn num_hands(&self, player: usize) -> usize {
        self.hands[player].len()
    }

    /// For each of `trav`'s hands, the total opponent reach over hands that share
    /// no card with it. Inclusion-exclusion: all, minus those sharing either card,
    /// plus the identical combo (subtracted twice).
    pub(crate) fn compatible_sums(&self, trav: usize, opp_reach: &[f32], out: &mut [f32]) {
        let opp = 1 - trav;
        let mut card_sum = [0f32; 52];
        let mut total = 0f32;
        for (o, &r) in opp_reach.iter().enumerate() {
            if r != 0.0 {
                let (a, b) = self.hands[opp][o];
                total += r;
                card_sum[a.index() as usize] += r;
                card_sum[b.index() as usize] += r;
            }
        }
        for (h, out) in out.iter_mut().enumerate() {
            let (a, b) = self.hands[trav][h];
            let same = self.same[trav][h];
            let back = if same == NONE {
                0.0
            } else {
                opp_reach[same as usize]
            };
            *out = total - card_sum[a.index() as usize] - card_sum[b.index() as usize] + back;
        }
    }

    /// Counterfactual values for `trav` at a fold node.
    pub(crate) fn fold_values(
        &self,
        trav: usize,
        folder: usize,
        invested: [i32; 2],
        opp_reach: &[f32],
        out: &mut [f32],
    ) {
        self.compatible_sums(trav, opp_reach, out);
        let half = self.tree.pot as f32 / 2.0;
        let payoff = if folder == trav {
            -(half + invested[trav] as f32)
        } else {
            half + invested[1 - trav] as f32
        };
        for v in out.iter_mut() {
            *v *= payoff;
        }
    }

    /// Counterfactual values for `trav` at a showdown, in O(n) after the sort:
    /// sweep the opponent's hands in rank order keeping a running total and a
    /// per-card total, so card removal is a subtraction rather than a search.
    pub(crate) fn showdown_values(
        &self,
        trav: usize,
        board: usize,
        invested: [i32; 2],
        opp_reach: &[f32],
        out: &mut [f32],
    ) {
        debug_assert_eq!(invested[0], invested[1]);
        let opp = 1 - trav;
        let stake = self.tree.pot as f32 / 2.0 + invested[0] as f32;
        let sd = self.showdowns[board].as_ref().expect("showdown data");
        let (mine, theirs) = (&sd.sorted[trav], &sd.sorted[opp]);
        out.fill(0.0);

        // Wins: opponent hands strictly weaker.
        let mut card_sum = [0f32; 52];
        let mut total = 0f32;
        let mut j = 0;
        for &(rank, h) in mine {
            while j < theirs.len() && theirs[j].0 < rank {
                let o = theirs[j].1 as usize;
                let r = opp_reach[o];
                let (a, b) = self.hands[opp][o];
                total += r;
                card_sum[a.index() as usize] += r;
                card_sum[b.index() as usize] += r;
                j += 1;
            }
            let (a, b) = self.hands[trav][h as usize];
            out[h as usize] =
                stake * (total - card_sum[a.index() as usize] - card_sum[b.index() as usize]);
        }

        // Losses: opponent hands strictly stronger.
        let mut card_sum = [0f32; 52];
        let mut total = 0f32;
        let mut j = theirs.len();
        for &(rank, h) in mine.iter().rev() {
            while j > 0 && theirs[j - 1].0 > rank {
                j -= 1;
                let o = theirs[j].1 as usize;
                let r = opp_reach[o];
                let (a, b) = self.hands[opp][o];
                total += r;
                card_sum[a.index() as usize] += r;
                card_sum[b.index() as usize] += r;
            }
            let (a, b) = self.hands[trav][h as usize];
            out[h as usize] -=
                stake * (total - card_sum[a.index() as usize] - card_sum[b.index() as usize]);
        }
    }

    /// Probability of each specific card being dealt next, for a fixed pair of
    /// hands, given the current board length.
    pub(crate) fn deal_probability(&self, board: usize) -> f32 {
        1.0 / (52 - self.tree.boards[board].len() - 4) as f32
    }
}

/// All 24 permutations of the four suits.
fn all_suit_perms() -> Vec<SuitPerm> {
    let mut out = Vec::with_capacity(24);
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; 4];
                    p.iter().for_each(|&x| seen[x as usize] = true);
                    if seen.iter().all(|&x| x) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}
