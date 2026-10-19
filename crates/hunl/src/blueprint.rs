//! The no-limit blueprint: an abstract betting tree over the real rules, a card
//! abstraction, and the glue that lets external-sampling MCCFR train on it.
//!
//! The abstract game differs from the real one in two ways only:
//!
//! - **Bets.** At each decision the player may fold, check or call, raise by one
//!   of a few pot fractions for that street, or go all-in. Every abstract action
//!   is applied through [`State::apply`], so an abstract line is always a legal
//!   real line.
//! - **Cards.** A player observes a bucket per street (169 lossless classes
//!   preflop, E[HS²] / HS percentiles afterwards) instead of exact cards.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use eval::{Card, Hand};

use engine::{BettingNode, Mccfr, SampledGame, SplitMix};

use crate::buckets::Buckets;
use crate::iso::HandIndexer;
use crate::rules::{Action, Params, State};

/// Raise sizes on one street, as fractions of the pot. `bets` apply when there is
/// nothing to call; `raises` when facing a bet, measured on the pot after calling.
#[derive(Clone, Debug)]
pub struct StreetSizes {
    pub bets: Vec<f64>,
    pub raises: Vec<f64>,
    /// Maximum bets plus raises on this street (all-in always remains available
    /// until the cap is reached).
    pub max_raises: usize,
}

#[derive(Clone, Debug)]
pub struct ActionAbstraction {
    pub streets: [StreetSizes; 4],
}

impl ActionAbstraction {
    /// A compact menu for 100 big blinds:
    /// preflop opens to 2.5bb or 3.5bb, 3-bets to about 3x, 4-bets and all-in;
    /// flop 33% / 75%; turn 66%; river 66% / 125%; pot-sized raises; all-in always.
    pub fn default_100bb() -> ActionAbstraction {
        let s = |bets: &[f64], raises: &[f64], max_raises| StreetSizes {
            bets: bets.to_vec(),
            raises: raises.to_vec(),
            max_raises,
        };
        ActionAbstraction {
            streets: [
                s(&[1.0], &[0.75, 1.25], 4),
                s(&[0.33, 0.75], &[1.0], 3),
                s(&[0.66], &[1.0], 2),
                s(&[0.66, 1.25], &[1.0], 2),
            ],
        }
    }

    /// The abstract actions available at `state`, in a fixed order:
    /// fold, check/call, raises ascending, all-in last.
    pub fn actions(&self, state: &State) -> Vec<Action> {
        let mut out = Vec::with_capacity(6);
        let to_call = state.to_call();
        if to_call > 0 {
            out.push(Action::Fold);
            out.push(Action::Call);
        } else {
            out.push(Action::Check);
        }
        let street = state.street;
        let sizes = &self.streets[street];
        let raises_so_far = state.history[street]
            .iter()
            .filter(|a| matches!(a, Action::RaiseTo(_)))
            .count();
        if let Some((lo, hi)) = state.raise_range()
            && raises_so_far < sizes.max_raises
        {
            let me = state.to_act;
            let pot_after_call = state.pot() + to_call;
            let fractions = if to_call > 0 {
                &sizes.raises
            } else {
                &sizes.bets
            };
            let mut amounts: Vec<i32> = fractions
                .iter()
                .map(|f| state.contributed[1 - me] + (f * pot_after_call as f64).round() as i32)
                .map(|x| x.clamp(lo, hi))
                .collect();
            amounts.push(hi);
            amounts.sort_unstable();
            amounts.dedup();
            out.extend(amounts.into_iter().map(Action::RaiseTo));
        }
        out
    }
}

/// How a player's cards are summarised on each street.
pub trait CardAbstraction: Sync {
    /// Buckets on preflop, flop, turn and river.
    fn counts(&self) -> [usize; 4];
    /// Bucket of `hole` with a board of 0, 3, 4 or 5 cards.
    fn bucket(&self, hole: [Card; 2], board: &[Card]) -> usize;
}

impl CardAbstraction for Buckets {
    fn counts(&self) -> [usize; 4] {
        self.counts.map(|c| c as usize)
    }
    fn bucket(&self, hole: [Card; 2], board: &[Card]) -> usize {
        Buckets::bucket(self, hole, board) as usize
    }
}

/// A cheap abstraction for tests and smoke runs: the 169 preflop classes, then
/// the made-hand category (high card .. straight flush) on later streets.
#[derive(Default)]
pub struct CategoryBuckets {
    indexer: HandIndexer,
}

impl CardAbstraction for CategoryBuckets {
    fn counts(&self) -> [usize; 4] {
        [169, 9, 9, 9]
    }
    fn bucket(&self, hole: [Card; 2], board: &[Card]) -> usize {
        if board.is_empty() {
            self.indexer.index(hole, board) as usize
        } else {
            Hand::from_cards(board).with(&hole).rank().category() as usize
        }
    }
}

/// Public facts about a node needed to score terminals.
#[derive(Clone, Copy, Debug)]
pub struct NodeInfo {
    pub contributed: [i32; 2],
    pub folded: Option<usize>,
    pub street: usize,
}

/// One sampled hand: all nine cards plus everything derived from them.
#[derive(Clone, Debug)]
pub struct Deal {
    pub hole: [[Card; 2]; 2],
    pub board: [Card; 5],
    /// `buckets[player][street]`.
    pub buckets: [[u16; 4]; 2],
    /// Showdown strength of each player's seven cards.
    pub ranks: [u16; 2],
}

pub struct Blueprint<A: CardAbstraction> {
    pub params: Params,
    pub actions_abstraction: ActionAbstraction,
    pub cards: A,
    nodes: Vec<BettingNode>,
    info: Vec<NodeInfo>,
    /// The real action taken to reach each child, per action node.
    actions: Vec<Vec<Action>>,
}

impl<A: CardAbstraction> Blueprint<A> {
    pub fn new(params: Params, actions: ActionAbstraction, cards: A) -> Blueprint<A> {
        let mut bp = Blueprint {
            params,
            actions_abstraction: actions,
            cards,
            nodes: Vec::new(),
            info: Vec::new(),
            actions: Vec::new(),
        };
        bp.build(&State::new(params));
        bp
    }

    fn build(&mut self, state: &State) -> usize {
        let id = self.nodes.len();
        self.nodes.push(BettingNode::Terminal);
        self.info.push(NodeInfo {
            contributed: state.contributed,
            folded: state.folded,
            street: state.street,
        });
        self.actions.push(Vec::new());
        if state.is_terminal() {
            return id;
        }
        let acts = self.actions_abstraction.actions(state);
        let children = acts
            .iter()
            .map(|&a| {
                let next = state.apply(a).expect("abstract action must be legal");
                self.build(&next)
            })
            .collect();
        self.nodes[id] = BettingNode::Action {
            player: state.to_act,
            street: state.street,
            children,
        };
        self.actions[id] = acts;
        id
    }

    pub fn info(&self, node: usize) -> &NodeInfo {
        &self.info[node]
    }

    /// The real actions leading to each child of an action node.
    pub fn node_actions(&self, node: usize) -> &[Action] {
        &self.actions[node]
    }

    /// Action nodes per street.
    pub fn action_nodes_per_street(&self) -> [usize; 4] {
        let mut c = [0; 4];
        for n in &self.nodes {
            if let BettingNode::Action { street, .. } = n {
                c[*street] += 1;
            }
        }
        c
    }

    /// Regret plus strategy-sum entries, and the bytes they take as f64.
    pub fn table_entries(&self) -> (usize, usize) {
        let counts = self.cards.counts();
        let entries: usize = self
            .nodes
            .iter()
            .map(|n| match n {
                BettingNode::Action {
                    street, children, ..
                } => children.len() * counts[*street],
                BettingNode::Terminal => 0,
            })
            .sum();
        (entries, entries * 2 * 8)
    }
}

impl<A: CardAbstraction> SampledGame for Blueprint<A> {
    type Deal = Deal;

    fn nodes(&self) -> &[BettingNode] {
        &self.nodes
    }

    fn num_buckets(&self, street: usize) -> usize {
        self.cards.counts()[street]
    }

    fn sample(&self, rng: &mut SplitMix) -> Deal {
        let mut deck: [u8; 52] = std::array::from_fn(|i| i as u8);
        for i in 0..9 {
            let j = i + rng.below(52 - i);
            deck.swap(i, j);
        }
        let c = |i: usize| Card::from_index(deck[i]);
        let hole = [[c(0), c(1)], [c(2), c(3)]];
        let board = [c(4), c(5), c(6), c(7), c(8)];
        let mut buckets = [[0u16; 4]; 2];
        for p in 0..2 {
            for (street, len) in [0usize, 3, 4, 5].into_iter().enumerate() {
                buckets[p][street] = self.cards.bucket(hole[p], &board[..len]) as u16;
            }
        }
        let b = Hand::from_cards(&board);
        let ranks = [b.with(&hole[0]).rank().0, b.with(&hole[1]).rank().0];
        Deal {
            hole,
            board,
            buckets,
            ranks,
        }
    }

    fn bucket(&self, deal: &Deal, player: usize, street: usize) -> usize {
        deal.buckets[player][street] as usize
    }

    fn utility(&self, deal: &Deal, node: usize) -> f64 {
        let info = &self.info[node];
        if let Some(f) = info.folded {
            let lost = info.contributed[f] as f64;
            return if f == 0 { -lost } else { lost };
        }
        let stake = info.contributed[0] as f64;
        match deal.ranks[0].cmp(&deal.ranks[1]) {
            std::cmp::Ordering::Greater => stake,
            std::cmp::Ordering::Less => -stake,
            std::cmp::Ordering::Equal => 0.0,
        }
    }
}

/// Checkpoint format 2 stores f64 regrets and sums; format 1 (f32) still loads.
const MAGIC: &[u8; 8] = b"HCFRBP02";
const MAGIC_V1: &[u8; 8] = b"HCFRBP01";

/// Writes a training checkpoint.
pub fn save_checkpoint<A: CardAbstraction>(m: &Mccfr<Blueprint<A>>, path: &Path) -> io::Result<()> {
    let (regrets, sums) = m.export();
    let mut w = BufWriter::new(File::create(path)?);
    w.write_all(MAGIC)?;
    w.write_all(&m.iterations().to_le_bytes())?;
    w.write_all(&(regrets.len() as u64).to_le_bytes())?;
    for x in regrets.iter().chain(&sums) {
        w.write_all(&x.to_le_bytes())?;
    }
    w.flush()
}

/// Loads a checkpoint (format 1 or 2) into a fresh trainer for the same tree.
pub fn load_checkpoint<A: CardAbstraction>(m: &Mccfr<Blueprint<A>>, path: &Path) -> io::Result<()> {
    let mut r = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    let width = match &magic {
        m if m == MAGIC => 8,
        m if m == MAGIC_V1 => 4,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a blueprint checkpoint",
            ));
        }
    };
    let mut u = [0u8; 8];
    r.read_exact(&mut u)?;
    let iterations = u64::from_le_bytes(u);
    r.read_exact(&mut u)?;
    let n = u64::from_le_bytes(u) as usize;
    if n != m.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("checkpoint has {n} entries, tree has {}", m.len()),
        ));
    }
    let mut read_vec = || -> io::Result<Vec<f64>> {
        let mut bytes = vec![0u8; n * width];
        r.read_exact(&mut bytes)?;
        Ok(bytes
            .chunks_exact(width)
            .map(|c| match width {
                8 => f64::from_le_bytes(c.try_into().unwrap()),
                _ => f32::from_le_bytes(c.try_into().unwrap()) as f64,
            })
            .collect())
    };
    let regrets = read_vec()?;
    let sums = read_vec()?;
    m.import(&regrets, &sums, iterations);
    Ok(())
}

/// Name of each preflop class, e.g. `AKs`, by class index.
pub fn preflop_class_names() -> Vec<String> {
    let indexer = HandIndexer::new();
    let mut names = vec![String::new(); 169];
    for a in 0..52u8 {
        for b in 0..a {
            let (x, y) = (Card::from_index(a), Card::from_index(b));
            let i = indexer.index([x, y], &[]) as usize;
            if names[i].is_empty() {
                let r = eval::card::RANK_CHARS;
                let (hi, lo) = if x.rank() >= y.rank() { (x, y) } else { (y, x) };
                let (h, l) = (r[hi.rank() as usize], r[lo.rank() as usize]);
                names[i] = if hi.rank() == lo.rank() {
                    format!("{h}{l}")
                } else if hi.suit() == lo.suit() {
                    format!("{h}{l}s")
                } else {
                    format!("{h}{l}o")
                };
            }
        }
    }
    names
}

/// What a preflop chart shows for each hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartKind {
    /// Probability of raising (any size, including all-in).
    Raise,
    /// Probability of not folding.
    Play,
}

/// A 13x13 chart of the button's first action, from the average strategy:
/// pairs on the diagonal, suited hands above it, offsuit below.
pub fn preflop_chart<A: CardAbstraction>(
    bp: &Blueprint<A>,
    m: &Mccfr<Blueprint<A>>,
    kind: ChartKind,
) -> String {
    let names = preflop_class_names();
    let acts = bp.node_actions(0);
    let mut by_name = std::collections::HashMap::new();
    for (class, name) in names.iter().enumerate() {
        let s = m.average_strategy(0, class);
        let keep = |a: &Action| match kind {
            ChartKind::Raise => matches!(a, Action::RaiseTo(_)),
            ChartKind::Play => !matches!(a, Action::Fold),
        };
        let p: f64 = acts
            .iter()
            .zip(&s)
            .filter(|(a, _)| keep(a))
            .map(|(_, p)| p)
            .sum();
        by_name.insert(name.clone(), p);
    }
    let r = eval::card::RANK_CHARS;
    let title = match kind {
        ChartKind::Raise => "button raise %",
        ChartKind::Play => "button play % (call or raise)",
    };
    let mut out = format!("{title}, suited above the diagonal:\n    ");
    for j in (0..13).rev() {
        out.push_str(&format!("{:>4}", r[j]));
    }
    out.push('\n');
    for i in (0..13).rev() {
        out.push_str(&format!("{:>4}", r[i]));
        for j in (0..13).rev() {
            let name = if i == j {
                format!("{}{}", r[i], r[j])
            } else if j < i {
                format!("{}{}s", r[i], r[j])
            } else {
                format!("{}{}o", r[j], r[i])
            };
            out.push_str(&format!("{:>4.0}", 100.0 * by_name[&name]));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_menu_and_three_bet_sizes() {
        let a = ActionAbstraction::default_100bb();
        let s = State::new(Params::DEFAULT);
        assert_eq!(
            a.actions(&s),
            vec![
                Action::Fold,
                Action::Call,
                Action::RaiseTo(250),
                Action::RaiseTo(350),
                Action::RaiseTo(10_000)
            ]
        );
        let s = s.apply(Action::RaiseTo(250)).unwrap();
        // Facing 250: call makes the pot 500, so the 0.75 and 1.25 pot raises
        // go to 625 and 875.
        assert_eq!(
            a.actions(&s),
            vec![
                Action::Fold,
                Action::Call,
                Action::RaiseTo(625),
                Action::RaiseTo(875),
                Action::RaiseTo(10_000)
            ]
        );
    }

    #[test]
    fn category_abstraction_buckets() {
        let c = CategoryBuckets::default();
        let cards = eval::parse_cards("As Ad Ah Kd 2c 7s 9h").unwrap();
        assert!(c.bucket([cards[0], cards[1]], &[]) < 169);
        assert_eq!(
            c.bucket([cards[0], cards[1]], &cards[2..5]),
            eval::Category::ThreeOfAKind as usize
        );
    }
}
