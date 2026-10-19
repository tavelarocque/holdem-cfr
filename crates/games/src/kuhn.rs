//! Kuhn poker (Kuhn 1950).
//!
//! Three cards J < Q < K, one dealt to each player. Both ante 1. Player 0 may
//! pass or bet 1; facing a bet a player may fold (pass) or call (bet); after
//! pass-pass, or any call, the higher card wins the pot. 12 information sets.
//! The game value for player 0 is -1/18.

use engine::{Game, NodeType};

pub const CARD_NAMES: [char; 3] = ['J', 'Q', 'K'];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KuhnAction {
    /// Check, or fold when facing a bet.
    Pass,
    /// Bet, or call when facing a bet.
    Bet,
}

#[derive(Clone, Debug, Default)]
pub struct KuhnState {
    /// `cards[p]` is player p's card; `None` until chance deals.
    pub cards: Option<[u8; 2]>,
    pub history: Vec<KuhnAction>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Kuhn;

impl KuhnState {
    fn history_string(&self) -> String {
        self.history
            .iter()
            .map(|a| match a {
                KuhnAction::Pass => 'p',
                KuhnAction::Bet => 'b',
            })
            .collect()
    }
}

impl Game for Kuhn {
    type State = KuhnState;
    type Action = KuhnAction;
    type InfoKey = String;

    fn root(&self) -> KuhnState {
        KuhnState::default()
    }

    fn node_type(&self, s: &KuhnState) -> NodeType {
        if s.cards.is_none() {
            return NodeType::Chance;
        }
        match s.history_string().as_str() {
            "pp" | "bp" | "bb" | "pbp" | "pbb" => NodeType::Terminal,
            _ => NodeType::Decision(s.history.len() % 2),
        }
    }

    fn payoff(&self, s: &KuhnState) -> f64 {
        let [c0, c1] = s.cards.expect("terminal state has cards");
        let showdown = if c0 > c1 { 1.0 } else { -1.0 };
        match s.history_string().as_str() {
            "pp" => showdown,
            "bp" => 1.0,
            "pbp" => -1.0,
            "bb" | "pbb" => 2.0 * showdown,
            h => panic!("payoff asked at non-terminal history {h:?}"),
        }
    }

    fn chance_outcomes(&self, _s: &KuhnState) -> Vec<(f64, KuhnState)> {
        let mut out = Vec::with_capacity(6);
        for c0 in 0..3u8 {
            for c1 in (0..3u8).filter(|&c| c != c0) {
                out.push((
                    1.0 / 6.0,
                    KuhnState {
                        cards: Some([c0, c1]),
                        history: Vec::new(),
                    },
                ));
            }
        }
        out
    }

    fn actions(&self, _s: &KuhnState) -> Vec<KuhnAction> {
        vec![KuhnAction::Pass, KuhnAction::Bet]
    }

    fn apply(&self, s: &KuhnState, a: KuhnAction) -> KuhnState {
        let mut next = s.clone();
        next.history.push(a);
        next
    }

    fn info_key(&self, s: &KuhnState) -> String {
        let cards = s.cards.expect("decision state has cards");
        let me = s.history.len() % 2;
        format!("{}{}", CARD_NAMES[cards[me] as usize], s.history_string())
    }
}
