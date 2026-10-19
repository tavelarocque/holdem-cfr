//! The public game tree of a postflop no-limit subgame.
//!
//! Everything here is public information: the board, the betting so far and the
//! chips each player has put in. Private hands live in [`crate::Spot`]. Player 0
//! is out of position (acts first on every street), player 1 is in position.

use std::fmt;

use eval::Card;

/// A betting action. Amounts are the acting player's *total* chips put in since
/// the start of the subgame after the action ("bet to"), which makes every node's
/// pot computable from the action alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Fold,
    Check,
    Call,
    Bet(i32),
    Raise(i32),
    AllIn(i32),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Fold => write!(f, "fold"),
            Action::Check => write!(f, "check"),
            Action::Call => write!(f, "call"),
            Action::Bet(a) => write!(f, "bet {a}"),
            Action::Raise(a) => write!(f, "raise {a}"),
            Action::AllIn(a) => write!(f, "allin {a}"),
        }
    }
}

/// Bet sizing on one street. Sizes are fractions of the pot, with separate
/// lists per player (0 = OOP, 1 = IP) so asymmetric trees are easy to express.
#[derive(Clone, Debug, Default)]
pub struct StreetBets {
    /// Bet sizes as fractions of the pot, when no bet is pending.
    pub bets: [Vec<f64>; 2],
    /// Raise sizes as fractions of the pot *after calling*.
    pub raises: [Vec<f64>; 2],
}

/// Bet sizing for the whole tree.
#[derive(Clone, Debug)]
pub struct BetConfig {
    /// Sizes on the flop, turn and river.
    pub streets: [StreetBets; 3],
    /// Maximum bets plus raises per street (1 = a bet but no raise).
    pub max_raises: u8,
    /// Always offer an all-in when the player can raise.
    pub allin: bool,
}

impl BetConfig {
    /// The same sizes for both players on every street.
    pub fn symmetric(bets: &[f64], raises: &[f64], max_raises: u8, allin: bool) -> BetConfig {
        let street = StreetBets {
            bets: [bets.to_vec(), bets.to_vec()],
            raises: [raises.to_vec(), raises.to_vec()],
        };
        BetConfig {
            streets: [street.clone(), street.clone(), street],
            max_raises,
            allin,
        }
    }

    /// Replaces both players' sizes on one street (0 = flop, 1 = turn, 2 = river).
    #[must_use]
    pub fn with_street(mut self, street: usize, bets: &[f64], raises: &[f64]) -> BetConfig {
        self.streets[street] = StreetBets {
            bets: [bets.to_vec(), bets.to_vec()],
            raises: [raises.to_vec(), raises.to_vec()],
        };
        self
    }

    /// Replaces one player's bet sizes on every street.
    #[must_use]
    pub fn with_player_bets(mut self, player: usize, bets: &[f64]) -> BetConfig {
        for s in &mut self.streets {
            s.bets[player] = bets.to_vec();
        }
        self
    }

    /// Replaces one player's raise sizes on every street.
    #[must_use]
    pub fn with_player_raises(mut self, player: usize, raises: &[f64]) -> BetConfig {
        for s in &mut self.streets {
            s.raises[player] = raises.to_vec();
        }
        self
    }

    fn street(&self, board_len: usize) -> &StreetBets {
        &self.streets[board_len - 3]
    }
}

/// A permutation of the four suits: suit `s` maps to `perm[s]`.
pub type SuitPerm = [u8; 4];

pub const IDENTITY: SuitPerm = [0, 1, 2, 3];

/// Applies a suit permutation to a card.
pub fn permute_card(perm: &SuitPerm, c: Card) -> Card {
    Card::new(c.rank(), perm[c.suit() as usize])
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    Action {
        player: usize,
        actions: Vec<Action>,
        children: Vec<usize>,
    },
    /// A public card is dealt. `children[k]` follows `cards[k]`. With suit
    /// isomorphism, `cards[k]` is the canonical card of a class of strategically
    /// identical cards: `members[k]` lists every card in the class with the index
    /// of a symmetry (into the spot's symmetry list) mapping `cards[k]` onto it.
    Chance {
        cards: Vec<Card>,
        children: Vec<usize>,
        members: Vec<Vec<(Card, usize)>>,
    },
    Fold {
        folder: usize,
    },
    Showdown,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    /// Chips each player has put in since the start of the subgame.
    pub invested: [i32; 2],
    /// Index into [`PublicTree::boards`].
    pub board: usize,
}

#[derive(Clone, Debug)]
pub struct PublicTree {
    pub nodes: Vec<Node>,
    /// Every board that occurs; `boards[0]` is the starting board.
    pub boards: Vec<Vec<Card>>,
    /// Pot at the start of the subgame (both players contributed half).
    pub pot: i32,
    /// Chips each player has behind at the start.
    pub stack: i32,
}

/// Betting state while building.
#[derive(Clone)]
struct Street {
    invested: [i32; 2],
    to_act: usize,
    raises: u8,
    /// Size of the last bet or raise increment, for the minimum-raise rule.
    last_increment: i32,
    /// Whether OOP has checked this street.
    checked: bool,
}

impl PublicTree {
    /// Builds the tree with every chance card dealt explicitly.
    pub fn build(board: &[Card], pot: i32, stack: i32, config: &BetConfig) -> PublicTree {
        PublicTree::build_with_symmetry(board, pot, stack, config, &[IDENTITY])
    }

    /// Builds the tree, merging chance cards that some permutation in `symmetries`
    /// maps onto each other while fixing the board. `symmetries[0]` must be the
    /// identity, and the caller guarantees every permutation preserves both ranges.
    pub fn build_with_symmetry(
        board: &[Card],
        pot: i32,
        stack: i32,
        config: &BetConfig,
        symmetries: &[SuitPerm],
    ) -> PublicTree {
        assert_eq!(symmetries.first(), Some(&IDENTITY));
        assert!((3..=5).contains(&board.len()), "board must have 3-5 cards");
        assert!(pot > 0 && stack >= 0);
        let tree = PublicTree {
            nodes: Vec::new(),
            boards: vec![board.to_vec()],
            pot,
            stack,
        };
        let start = Street {
            invested: [0, 0],
            to_act: 0,
            raises: 0,
            last_increment: 0,
            checked: false,
        };
        let mut builder = Builder {
            tree,
            cfg: config,
            symmetries,
        };
        builder.betting(0, start);
        builder.tree
    }

    pub fn root(&self) -> usize {
        0
    }

    /// Number of action nodes.
    pub fn num_action_nodes(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Action { .. }))
            .count()
    }
}

struct Builder<'a> {
    tree: PublicTree,
    cfg: &'a BetConfig,
    symmetries: &'a [SuitPerm],
}

impl Builder<'_> {
    fn push(&mut self, invested: [i32; 2], board: usize) -> usize {
        let tree = &mut self.tree;
        tree.nodes.push(Node {
            kind: NodeKind::Showdown,
            invested,
            board,
        });
        tree.nodes.len() - 1
    }

    fn pot_now(&self, invested: [i32; 2]) -> i32 {
        self.tree.pot + invested[0] + invested[1]
    }

    /// Builds the subtree where `s.to_act` must act. Returns the node index.
    fn betting(&mut self, board: usize, s: Street) -> usize {
        let cfg = self.cfg;
        let sizes = cfg.street(self.tree.boards[board].len());
        let id = self.push(s.invested, board);
        let me = s.to_act;
        let opp = 1 - me;
        let stack = self.tree.stack;
        let behind = stack - s.invested[me];
        let to_call = s.invested[opp] - s.invested[me];
        let mut options: Vec<(Action, Street)> = Vec::new();

        if to_call > 0 {
            options.push((Action::Fold, s.clone()));
            options.push((Action::Call, s.clone()));
        } else {
            options.push((Action::Check, s.clone()));
        }

        let can_raise = s.raises < cfg.max_raises && behind > to_call && s.invested[opp] < stack;
        if can_raise {
            let mut amounts: Vec<i32> = Vec::new();
            if to_call == 0 {
                let pot = self.pot_now(s.invested);
                for &f in &sizes.bets[me] {
                    let add = ((f * pot as f64).round() as i32).max(1);
                    amounts.push(s.invested[me] + add);
                }
            } else {
                let pot_after_call = self.pot_now(s.invested) + to_call;
                for &f in &sizes.raises[me] {
                    let inc = ((f * pot_after_call as f64).round() as i32).max(s.last_increment);
                    amounts.push(s.invested[opp] + inc);
                }
            }
            if cfg.allin {
                amounts.push(stack);
            }
            let mut seen = Vec::new();
            for to in amounts {
                let to = to.min(stack);
                if to <= s.invested[opp] || seen.contains(&to) {
                    continue;
                }
                seen.push(to);
                let action = if to == stack {
                    Action::AllIn(to)
                } else if to_call == 0 {
                    Action::Bet(to)
                } else {
                    Action::Raise(to)
                };
                let mut next = s.clone();
                next.last_increment = (to - s.invested[opp]).max(s.last_increment);
                next.invested[me] = to;
                next.raises += 1;
                next.to_act = opp;
                options.push((action, next));
            }
            // All-in last, other sizes ascending.
            let tail = &mut options[if to_call > 0 { 2 } else { 1 }..];
            tail.sort_by_key(|(a, _)| match a {
                Action::Bet(x) | Action::Raise(x) => *x,
                Action::AllIn(_) => i32::MAX,
                _ => 0,
            });
        }

        let mut actions = Vec::with_capacity(options.len());
        let mut children = Vec::with_capacity(options.len());
        for (action, mut next) in options {
            let child = match action {
                Action::Fold => {
                    let c = self.push(next.invested, board);
                    self.tree.nodes[c].kind = NodeKind::Fold { folder: me };
                    c
                }
                Action::Check if me == 0 => {
                    next.checked = true;
                    next.to_act = 1;
                    self.betting(board, next)
                }
                Action::Check => self.street_over(board, next.invested),
                Action::Call => {
                    next.invested[me] = next.invested[opp];
                    self.street_over(board, next.invested)
                }
                _ => self.betting(board, next),
            };
            actions.push(action);
            children.push(child);
        }
        self.tree.nodes[id].kind = NodeKind::Action {
            player: me,
            actions,
            children,
        };
        id
    }

    /// The betting round has closed with equal investments.
    fn street_over(&mut self, board: usize, invested: [i32; 2]) -> usize {
        debug_assert_eq!(invested[0], invested[1]);
        let id = self.push(invested, board);
        let cards_out = self.tree.boards[board].clone();
        if cards_out.len() == 5 {
            return id; // Showdown
        }
        let all_in = invested[0] == self.tree.stack;

        // Symmetries that fix the current board as a set.
        let fixing: Vec<usize> = (0..self.symmetries.len())
            .filter(|&g| {
                cards_out
                    .iter()
                    .all(|&c| cards_out.contains(&permute_card(&self.symmetries[g], c)))
            })
            .collect();
        let live: Vec<Card> = Card::deck().filter(|c| !cards_out.contains(c)).collect();
        // Canonical card of each live card: the lowest image under the fixing group.
        let canonical = |c: Card| {
            fixing
                .iter()
                .map(|&g| permute_card(&self.symmetries[g], c))
                .min()
                .expect("identity is always present")
        };

        let mut cards = Vec::new();
        let mut members = Vec::new();
        for &c in &live {
            if canonical(c) != c {
                continue;
            }
            let mut class = Vec::new();
            for &other in &live {
                if let Some(&g) = fixing
                    .iter()
                    .find(|&&g| permute_card(&self.symmetries[g], c) == other)
                {
                    class.push((other, g));
                }
            }
            cards.push(c);
            members.push(class);
        }

        let mut children = Vec::with_capacity(cards.len());
        for &c in &cards {
            let mut b = cards_out.clone();
            b.push(c);
            self.tree.boards.push(b);
            let nb = self.tree.boards.len() - 1;
            let child = if all_in {
                self.street_over(nb, invested)
            } else {
                self.betting(
                    nb,
                    Street {
                        invested,
                        to_act: 0,
                        raises: 0,
                        last_increment: 0,
                        checked: false,
                    },
                )
            };
            children.push(child);
        }
        self.tree.nodes[id].kind = NodeKind::Chance {
            cards,
            children,
            members,
        };
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eval::parse_cards;

    fn river() -> Vec<Card> {
        parse_cards("Kh 9d 5c 3s 2h").unwrap()
    }

    fn actions_at(t: &PublicTree, n: usize) -> (Vec<Action>, Vec<usize>) {
        match &t.nodes[n].kind {
            NodeKind::Action {
                actions, children, ..
            } => (actions.clone(), children.clone()),
            k => panic!("not an action node: {k:?}"),
        }
    }

    #[test]
    fn check_check_reaches_showdown() {
        let t = PublicTree::build(
            &river(),
            100,
            400,
            &BetConfig::symmetric(&[0.5], &[], 1, false),
        );
        let (a, c) = actions_at(&t, 0);
        assert_eq!(a, vec![Action::Check, Action::Bet(50)]);
        let (a1, c1) = actions_at(&t, c[0]);
        assert_eq!(a1, vec![Action::Check, Action::Bet(50)]);
        assert!(matches!(t.nodes[c1[0]].kind, NodeKind::Showdown));
        // Facing a bet with max_raises = 1: fold or call only.
        let (a2, c2) = actions_at(&t, c[1]);
        assert_eq!(a2, vec![Action::Fold, Action::Call]);
        assert!(matches!(t.nodes[c2[0]].kind, NodeKind::Fold { folder: 1 }));
        assert_eq!(t.nodes[c2[1]].invested, [50, 50]);
    }

    #[test]
    fn raises_use_pot_after_call_and_cap() {
        // Pot 100, OOP bets 100 (pot); IP raises pot: call 100 -> pot 300 -> raise to 400.
        let t = PublicTree::build(
            &river(),
            100,
            1000,
            &BetConfig::symmetric(&[1.0], &[1.0], 2, false),
        );
        let (_, c) = actions_at(&t, 0);
        let (a, c) = actions_at(&t, c[1]);
        assert_eq!(a, vec![Action::Fold, Action::Call, Action::Raise(400)]);
        // After one bet and one raise the cap of 2 is reached.
        let (a, _) = actions_at(&t, c[2]);
        assert_eq!(a, vec![Action::Fold, Action::Call]);
    }

    #[test]
    fn oversized_bets_become_one_allin() {
        // Stack 120: a pot bet (100) is fine, 2x pot (200) and the all-in collapse.
        let t = PublicTree::build(
            &river(),
            100,
            120,
            &BetConfig::symmetric(&[1.0, 2.0], &[1.0], 3, true),
        );
        let (a, c) = actions_at(&t, 0);
        assert_eq!(a, vec![Action::Check, Action::Bet(100), Action::AllIn(120)]);
        // Facing the 100 bet, IP's only raise is all-in.
        let (a, c2) = actions_at(&t, c[1]);
        assert_eq!(a, vec![Action::Fold, Action::Call, Action::AllIn(120)]);
        // Facing all-in: fold or call, nothing else.
        let (a, _) = actions_at(&t, c2[2]);
        assert_eq!(a, vec![Action::Fold, Action::Call]);
        let _ = c;
    }

    #[test]
    fn minimum_raise_is_enforced() {
        // OOP bets 100 into 100; a 0.1-pot raise (30) is below the 100 increment.
        let t = PublicTree::build(
            &river(),
            100,
            1000,
            &BetConfig::symmetric(&[1.0], &[0.1], 2, false),
        );
        let (_, c) = actions_at(&t, 0);
        let (a, _) = actions_at(&t, c[1]);
        assert_eq!(a, vec![Action::Fold, Action::Call, Action::Raise(200)]);
    }

    #[test]
    fn per_street_sizes_apply_on_their_street() {
        let board = parse_cards("Kh 9d 5c 3s").unwrap();
        let cfg = BetConfig::symmetric(&[0.5], &[], 1, false).with_street(2, &[1.0], &[]);
        let t = PublicTree::build(&board, 100, 1000, &cfg);
        let (a, c) = actions_at(&t, 0);
        assert_eq!(a, vec![Action::Check, Action::Bet(50)]); // turn: half pot
        let (_, c) = actions_at(&t, c[0]);
        let NodeKind::Chance { children, .. } = &t.nodes[c[0]].kind else {
            panic!("expected chance");
        };
        let (a, _) = actions_at(&t, children[0]);
        assert_eq!(a, vec![Action::Check, Action::Bet(100)]); // river: pot
    }

    #[test]
    fn turn_tree_deals_every_river() {
        let board = parse_cards("Kh 9d 5c 3s").unwrap();
        let t = PublicTree::build(
            &board,
            100,
            100,
            &BetConfig::symmetric(&[0.5], &[], 1, true),
        );
        let (_, c) = actions_at(&t, 0);
        let (_, c) = actions_at(&t, c[0]); // OOP checked, IP acts
        match &t.nodes[c[0]].kind {
            NodeKind::Chance {
                cards,
                children,
                members,
            } => {
                assert_eq!(cards.len(), 48);
                assert!(members.iter().all(|m| m.len() == 1));
                assert_eq!(t.boards[t.nodes[children[0]].board].len(), 5);
            }
            k => panic!("expected chance, got {k:?}"),
        }
        // After an all-in and call on the turn, the river runs out with no betting.
        let (_, c) = actions_at(&t, 0);
        let allin = c.len() - 1;
        let (_, c2) = actions_at(&t, c[allin]);
        match &t.nodes[c2[1]].kind {
            NodeKind::Chance { children, .. } => {
                assert!(matches!(t.nodes[children[0]].kind, NodeKind::Showdown));
            }
            k => panic!("expected chance, got {k:?}"),
        }
    }
}
