//! Leduc hold'em (Southey et al. 2005).
//!
//! A six-card deck: two suits of J, Q, K. Both players ante 1 and receive one
//! private card. Two betting rounds with fixed bet sizes 2 then 4, at most two
//! raises per round (a bet and one re-raise), player 0 acting first in each round.
//! A public card is dealt between the rounds. At showdown a player whose private
//! card pairs the board wins; otherwise the higher rank wins; equal ranks split.
//!
//! Cards are kept distinct by suit, which gives the standard count of
//! 936 information sets (288 if suits were merged).

use engine::{Game, NodeType};

const RANKS: [char; 3] = ['J', 'Q', 'K'];
const SUITS: [char; 2] = ['s', 'h'];
const BET_SIZES: [u32; 2] = [2, 4];
const MAX_RAISES: u8 = 2;

fn rank(card: u8) -> u8 {
    card / 2
}

fn card_name(card: u8) -> String {
    format!(
        "{}{}",
        RANKS[rank(card) as usize],
        SUITS[(card % 2) as usize]
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeducAction {
    Fold,
    /// Check when there is nothing to call.
    Call,
    /// Bet when there is nothing to call.
    Raise,
}

impl LeducAction {
    fn symbol(self) -> char {
        match self {
            LeducAction::Fold => 'f',
            LeducAction::Call => 'c',
            LeducAction::Raise => 'r',
        }
    }
}

#[derive(Clone, Debug)]
pub struct LeducState {
    /// Private cards, `None` until dealt.
    pub cards: Option<[u8; 2]>,
    pub board: Option<u8>,
    pub round: usize,
    /// Betting history per round.
    pub history: [Vec<LeducAction>; 2],
    /// Chips each player has put in the pot.
    pub contrib: [u32; 2],
    pub raises: u8,
    pub to_act: usize,
    /// Player who folded, if any.
    pub folded: Option<usize>,
    /// Both betting rounds complete.
    pub showdown: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Leduc;

impl LeducState {
    fn awaiting_board(&self) -> bool {
        self.round == 1 && self.board.is_none()
    }
}

impl Game for Leduc {
    type State = LeducState;
    type Action = LeducAction;
    type InfoKey = String;

    fn root(&self) -> LeducState {
        LeducState {
            cards: None,
            board: None,
            round: 0,
            history: [Vec::new(), Vec::new()],
            contrib: [1, 1],
            raises: 0,
            to_act: 0,
            folded: None,
            showdown: false,
        }
    }

    fn node_type(&self, s: &LeducState) -> NodeType {
        if s.folded.is_some() || s.showdown {
            NodeType::Terminal
        } else if s.cards.is_none() || s.awaiting_board() {
            NodeType::Chance
        } else {
            NodeType::Decision(s.to_act)
        }
    }

    fn payoff(&self, s: &LeducState) -> f64 {
        if let Some(f) = s.folded {
            return if f == 0 {
                -(s.contrib[0] as f64)
            } else {
                s.contrib[1] as f64
            };
        }
        let [c0, c1] = s.cards.expect("showdown has cards");
        let board = rank(s.board.expect("showdown has a board"));
        let strength = |c: u8| {
            let r = rank(c);
            if r == board { 10 + r } else { r }
        };
        let (s0, s1) = (strength(c0), strength(c1));
        debug_assert_eq!(s.contrib[0], s.contrib[1]);
        match s0.cmp(&s1) {
            std::cmp::Ordering::Greater => s.contrib[1] as f64,
            std::cmp::Ordering::Less => -(s.contrib[0] as f64),
            std::cmp::Ordering::Equal => 0.0,
        }
    }

    fn chance_outcomes(&self, s: &LeducState) -> Vec<(f64, LeducState)> {
        match s.cards {
            None => {
                let mut out = Vec::with_capacity(30);
                for c0 in 0..6u8 {
                    for c1 in (0..6u8).filter(|&c| c != c0) {
                        let mut next = s.clone();
                        next.cards = Some([c0, c1]);
                        out.push((1.0 / 30.0, next));
                    }
                }
                out
            }
            Some([c0, c1]) => {
                let remaining: Vec<u8> = (0..6u8).filter(|&c| c != c0 && c != c1).collect();
                let p = 1.0 / remaining.len() as f64;
                remaining
                    .into_iter()
                    .map(|b| {
                        let mut next = s.clone();
                        next.board = Some(b);
                        (p, next)
                    })
                    .collect()
            }
        }
    }

    fn actions(&self, s: &LeducState) -> Vec<LeducAction> {
        let me = s.to_act;
        let facing_bet = s.contrib[1 - me] > s.contrib[me];
        let mut a = Vec::with_capacity(3);
        if facing_bet {
            a.push(LeducAction::Fold);
        }
        a.push(LeducAction::Call);
        if s.raises < MAX_RAISES {
            a.push(LeducAction::Raise);
        }
        a
    }

    fn apply(&self, s: &LeducState, action: LeducAction) -> LeducState {
        let mut n = s.clone();
        let me = s.to_act;
        let opp = 1 - me;
        n.history[s.round].push(action);
        match action {
            LeducAction::Fold => n.folded = Some(me),
            LeducAction::Raise => {
                n.contrib[me] = n.contrib[opp] + BET_SIZES[s.round];
                n.raises += 1;
                n.to_act = opp;
            }
            LeducAction::Call => {
                let was_facing_bet = n.contrib[opp] > n.contrib[me];
                n.contrib[me] = n.contrib[opp];
                // A check by the first player keeps the round open; anything else closes it.
                let round_over = was_facing_bet || !s.history[s.round].is_empty();
                if !round_over {
                    n.to_act = opp;
                } else if s.round == 0 {
                    n.round = 1;
                    n.raises = 0;
                    n.to_act = 0;
                } else {
                    n.showdown = true;
                }
            }
        }
        n
    }

    fn info_key(&self, s: &LeducState) -> String {
        let cards = s.cards.expect("decision state has cards");
        let h = |r: usize| s.history[r].iter().map(|a| a.symbol()).collect::<String>();
        let board = s.board.map(card_name).unwrap_or_else(|| "--".into());
        format!("{}|{}|{}/{}", card_name(cards[s.to_act]), board, h(0), h(1))
    }
}

#[cfg(test)]
mod tests {
    use super::LeducAction::{Call as C, Fold as F, Raise as R};
    use super::*;

    /// Deals `cards`, plays `round0`, deals `board` (if reached), plays `round1`.
    fn play(
        cards: [u8; 2],
        board: u8,
        round0: &[LeducAction],
        round1: &[LeducAction],
    ) -> LeducState {
        let g = Leduc;
        let mut s = g.root();
        s.cards = Some(cards);
        for &a in round0 {
            assert!(g.actions(&s).contains(&a), "{a:?} illegal in round 0");
            s = g.apply(&s, a);
        }
        if g.node_type(&s) == NodeType::Chance {
            s.board = Some(board);
        }
        for &a in round1 {
            assert!(g.actions(&s).contains(&a), "{a:?} illegal in round 1");
            s = g.apply(&s, a);
        }
        s
    }

    // Card ids: 0,1 = J; 2,3 = Q; 4,5 = K.

    #[test]
    fn fold_to_opening_bet_loses_the_ante() {
        let s = play([0, 4], 2, &[R, F], &[]);
        assert_eq!(Leduc.node_type(&s), NodeType::Terminal);
        assert_eq!(Leduc.payoff(&s), 1.0);
    }

    #[test]
    fn check_check_moves_to_round_two() {
        let s = play([0, 4], 2, &[C, C], &[]);
        assert_eq!(s.round, 1);
        assert_eq!(Leduc.node_type(&s), NodeType::Decision(0));
    }

    #[test]
    fn raises_are_capped_at_two_per_round() {
        let s = play([0, 4], 2, &[R, R], &[]);
        assert_eq!(Leduc.actions(&s), vec![F, C]);
        assert_eq!(s.contrib, [3, 5]);
    }

    #[test]
    fn pair_beats_higher_card() {
        // Player 0 holds J and pairs the board J; player 1 holds K.
        let s = play([0, 4], 1, &[R, C], &[R, C]);
        assert_eq!(s.contrib, [7, 7]);
        assert_eq!(Leduc.payoff(&s), 7.0);
    }

    #[test]
    fn higher_card_wins_without_pair_and_equal_ranks_split() {
        let s = play([2, 4], 0, &[C, C], &[C, C]);
        assert_eq!(Leduc.payoff(&s), -1.0);
        let s = play([2, 3], 0, &[C, R, C], &[C, C]);
        assert_eq!(Leduc.payoff(&s), 0.0);
    }

    #[test]
    fn maximum_pot_is_thirteen_each() {
        let s = play([0, 4], 2, &[C, R, R, C], &[C, R, R, C]);
        assert_eq!(s.contrib, [13, 13]);
    }
}
