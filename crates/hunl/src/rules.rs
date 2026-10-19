//! Heads-up no-limit hold'em betting rules.
//!
//! Player 0 is the button: posts the small blind, acts first preflop and last on
//! every later street. Player 1 posts the big blind. All amounts are chips, and
//! raises are expressed as "raise to" the player's total contribution for the
//! hand, as in the ACPC protocol. Cards are dealt by the caller: this module only
//! says how many board cards the current street needs.

use std::fmt;

use eval::{Card, Hand};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub small_blind: i32,
    pub big_blind: i32,
    /// Starting stack of each player (the effective stack).
    pub stack: i32,
}

impl Params {
    /// 100 big blinds with 50/100 blinds.
    pub const DEFAULT: Params = Params {
        small_blind: 50,
        big_blind: 100,
        stack: 10_000,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Fold,
    Check,
    Call,
    /// Raise (or bet) so that the player's total contribution becomes this amount.
    RaiseTo(i32),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Fold => write!(f, "f"),
            Action::Check | Action::Call => write!(f, "c"),
            Action::RaiseTo(x) => write!(f, "r{x}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IllegalAction(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub params: Params,
    /// 0 preflop, 1 flop, 2 turn, 3 river, 4 showdown.
    pub street: usize,
    /// Total chips each player has put in this hand.
    pub contributed: [i32; 2],
    pub to_act: usize,
    /// Size of the last bet or raise increment on this street.
    last_increment: i32,
    /// Whether each player has acted since the last bet or raise.
    acted: [bool; 2],
    pub folded: Option<usize>,
    /// Actions per street, for display and history keys.
    pub history: [Vec<Action>; 4],
}

impl State {
    pub fn new(params: Params) -> State {
        State {
            params,
            street: 0,
            contributed: [params.small_blind, params.big_blind],
            to_act: 0,
            last_increment: params.big_blind,
            acted: [false; 2],
            folded: None,
            history: Default::default(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.folded.is_some() || self.street == 4
    }

    /// Board cards that should be out on the current street (0, 3, 4 or 5).
    pub fn board_len(&self) -> usize {
        [0, 3, 4, 5, 5][self.street]
    }

    pub fn pot(&self) -> i32 {
        self.contributed[0] + self.contributed[1]
    }

    /// Chips the player to act must add to call.
    pub fn to_call(&self) -> i32 {
        self.contributed[1 - self.to_act] - self.contributed[self.to_act]
    }

    /// Smallest and largest legal raise-to amounts, if raising is allowed.
    pub fn raise_range(&self) -> Option<(i32, i32)> {
        if self.is_terminal() {
            return None;
        }
        let me = self.to_act;
        let opp = 1 - me;
        let max = self.params.stack;
        if self.contributed[opp] >= max || self.contributed[me] >= max {
            return None; // opponent all-in: nothing to raise against
        }
        if max <= self.contributed[opp] {
            return None;
        }
        let min = (self.contributed[opp] + self.last_increment).min(max);
        Some((min, max))
    }

    pub fn legal(&self, a: Action) -> bool {
        if self.is_terminal() {
            return false;
        }
        let facing = self.to_call() > 0;
        match a {
            Action::Fold => facing,
            Action::Check => !facing,
            Action::Call => facing,
            Action::RaiseTo(x) => self
                .raise_range()
                .is_some_and(|(lo, hi)| (lo..=hi).contains(&x)),
        }
    }

    /// The state after `a`, or an error if `a` is illegal here.
    pub fn apply(&self, a: Action) -> Result<State, IllegalAction> {
        if !self.legal(a) {
            return Err(IllegalAction(format!("{a:?} at {self}")));
        }
        let mut s = self.clone();
        let me = s.to_act;
        let opp = 1 - me;
        s.history[s.street].push(a);
        match a {
            Action::Fold => {
                s.folded = Some(me);
                return Ok(s);
            }
            Action::Check => s.acted[me] = true,
            Action::Call => {
                s.contributed[me] = s.contributed[opp];
                s.acted[me] = true;
            }
            Action::RaiseTo(x) => {
                s.last_increment = s.last_increment.max(x - s.contributed[opp]);
                s.contributed[me] = x;
                s.acted = [false; 2];
                s.acted[me] = true;
            }
        }
        if s.contributed[0] == s.contributed[1] && s.acted[0] && s.acted[1] {
            s.next_street();
        } else {
            s.to_act = opp;
        }
        Ok(s)
    }

    fn next_street(&mut self) {
        let all_in = self.contributed[0] == self.params.stack;
        self.street = if all_in { 4 } else { self.street + 1 };
        self.to_act = 1; // big blind acts first after the flop
        self.last_increment = self.params.big_blind;
        self.acted = [false; 2];
    }

    /// Net chips won by each player at a terminal state. `board` must hold five
    /// cards when the hand reached showdown.
    pub fn payoff(&self, hole: [[Card; 2]; 2], board: &[Card]) -> [i32; 2] {
        assert!(self.is_terminal());
        if let Some(f) = self.folded {
            let won = self.contributed[f];
            return if f == 0 { [-won, won] } else { [won, -won] };
        }
        assert_eq!(board.len(), 5, "showdown needs a full board");
        let b = Hand::from_cards(board);
        let r0 = b.with(&hole[0]).rank();
        let r1 = b.with(&hole[1]).rank();
        let stake = self.contributed[0];
        match r0.cmp(&r1) {
            std::cmp::Ordering::Greater => [stake, -stake],
            std::cmp::Ordering::Less => [-stake, stake],
            std::cmp::Ordering::Equal => [0, 0],
        }
    }
}

impl fmt::Display for State {
    /// ACPC-style betting string, e.g. `r300c/cr600c/`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, street) in self.history.iter().enumerate() {
            if i > 0 && i <= self.street.min(3) {
                write!(f, "/")?;
            }
            for a in street {
                write!(f, "{a}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;

    fn play(actions: &[Action]) -> State {
        actions
            .iter()
            .fold(State::new(Params::DEFAULT), |s, &a| s.apply(a).unwrap())
    }

    #[test]
    fn blinds_and_first_action() {
        let s = State::new(Params::DEFAULT);
        assert_eq!(s.contributed, [50, 100]);
        assert_eq!(s.to_act, 0);
        assert_eq!(s.to_call(), 50);
        assert_eq!(s.raise_range(), Some((200, 10_000)));
        assert!(!s.legal(Check));
        assert!(!s.legal(RaiseTo(150)));
    }

    #[test]
    fn limp_gives_big_blind_the_option() {
        let s = play(&[Call]);
        assert_eq!(s.street, 0);
        assert_eq!(s.to_act, 1);
        assert!(s.legal(Check));
        let s = s.apply(Check).unwrap();
        assert_eq!((s.street, s.to_act, s.board_len()), (1, 1, 3));
    }

    #[test]
    fn min_raise_tracks_the_last_increment() {
        // SB raises to 300 (increment 200): BB's minimum re-raise is to 500.
        let s = play(&[RaiseTo(300)]);
        assert_eq!(s.raise_range(), Some((500, 10_000)));
        // BB re-raises to 900 (increment 600): SB's minimum is 1500.
        let s = s.apply(RaiseTo(900)).unwrap();
        assert_eq!(s.raise_range(), Some((1_500, 10_000)));
    }

    #[test]
    fn postflop_order_and_bet_sizes() {
        let s = play(&[RaiseTo(300), Call]);
        assert_eq!((s.street, s.to_act, s.pot()), (1, 1, 600));
        assert_eq!(
            s.raise_range(),
            Some((400, 10_000)),
            "min bet is one big blind"
        );
        let s = s.apply(Check).unwrap().apply(Check).unwrap();
        assert_eq!((s.street, s.to_act), (2, 1));
        assert_eq!(s.to_string(), "r300c/cc/");
    }

    #[test]
    fn all_in_and_call_runs_out_the_board() {
        let s = play(&[RaiseTo(10_000)]);
        assert_eq!(s.raise_range(), None);
        assert_eq!(s.apply(Call).unwrap().street, 4);
        assert!(s.apply(Call).unwrap().is_terminal());
    }

    #[test]
    fn short_all_in_below_a_min_raise_is_allowed() {
        // Stacks of 500: SB raises to 400; BB may only shove to 500.
        let p = Params {
            small_blind: 50,
            big_blind: 100,
            stack: 500,
        };
        let s = State::new(p).apply(RaiseTo(400)).unwrap();
        assert_eq!(s.raise_range(), Some((500, 500)));
    }

    #[test]
    fn payoffs() {
        let cards = |s: &str| eval::parse_cards(s).unwrap();
        let hole = [
            [cards("As")[0], cards("Ad")[0]],
            [cards("Ks")[0], cards("Kd")[0]],
        ];
        let board = cards("2c 7h 9d Jc 3s");
        let s = play(&[Fold]);
        assert_eq!(s.payoff(hole, &[]), [-50, 50]);
        let s = play(&[RaiseTo(300), Call, Check, Check, Check, Check, Check, Check]);
        assert!(s.is_terminal());
        assert_eq!(s.payoff(hole, &board), [300, -300]);
        let s = play(&[RaiseTo(300), Fold]);
        assert_eq!(s.payoff(hole, &[]), [100, -100]);
    }
}
