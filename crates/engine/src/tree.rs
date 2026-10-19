//! A game compiled into a flat arena of nodes.
//!
//! Building the tree once and then iterating over indices makes every exact
//! algorithm cache-friendly and free of hashing. Nodes are stored in pre-order,
//! so a parent's index is always smaller than its children's.

use std::collections::HashMap;

use crate::game::{Game, NodeType};

/// One node of the compiled game tree.
#[derive(Clone, Debug)]
pub enum Node {
    /// Game over. `payoff` is to player 0.
    Terminal { payoff: f64 },
    /// Nature moves to `children[k]` with probability `probs[k]`.
    Chance {
        children: Vec<usize>,
        probs: Vec<f64>,
    },
    /// `player` chooses action `a`, leading to `children[a]`.
    Decision {
        player: usize,
        infoset: usize,
        children: Vec<usize>,
    },
}

/// Everything known about one information set.
#[derive(Clone, Debug)]
pub struct InfoSet {
    /// Who acts here.
    pub player: usize,
    /// Human-readable key, for printing strategies.
    pub label: String,
    /// Human-readable action names, in action order.
    pub actions: Vec<String>,
    /// Every tree node (history) that belongs to this information set.
    pub nodes: Vec<usize>,
    /// Start of this information set's slice in flat per-action arrays.
    pub offset: usize,
}

impl InfoSet {
    pub fn num_actions(&self) -> usize {
        self.actions.len()
    }

    /// The range of this information set inside a flat per-action array.
    pub fn range(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.actions.len()
    }
}

/// A compiled game tree.
#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub infosets: Vec<InfoSet>,
    /// Total number of (infoset, action) pairs: the length of every flat array.
    pub num_actions: usize,
}

impl Tree {
    /// Expands the full game tree of `game`.
    ///
    /// Panics if two histories in the same information set disagree about the legal
    /// actions, which would mean the game's `info_key` leaks or hides information.
    pub fn build<G: Game>(game: &G) -> Self {
        let mut builder = Builder {
            game,
            nodes: Vec::new(),
            infosets: Vec::new(),
            index: HashMap::new(),
            num_actions: 0,
        };
        builder.expand(&game.root());
        Tree {
            nodes: builder.nodes,
            infosets: builder.infosets,
            num_actions: builder.num_actions,
        }
    }

    pub fn root(&self) -> usize {
        0
    }

    /// Number of information sets belonging to `player`.
    pub fn infosets_of(&self, player: usize) -> usize {
        self.infosets.iter().filter(|i| i.player == player).count()
    }

    /// A strategy that plays uniformly at every information set.
    pub fn uniform_strategy(&self) -> Vec<f64> {
        let mut s = vec![0.0; self.num_actions];
        for info in &self.infosets {
            let p = 1.0 / info.num_actions() as f64;
            s[info.range()].fill(p);
        }
        s
    }

    /// Finds an information set by its label.
    pub fn find(&self, label: &str) -> Option<&InfoSet> {
        self.infosets.iter().find(|i| i.label == label)
    }
}

struct Builder<'g, G: Game> {
    game: &'g G,
    nodes: Vec<Node>,
    infosets: Vec<InfoSet>,
    index: HashMap<G::InfoKey, usize>,
    num_actions: usize,
}

impl<G: Game> Builder<'_, G> {
    fn expand(&mut self, state: &G::State) -> usize {
        // Reserve this node's slot first so parents precede children (pre-order).
        let id = self.nodes.len();
        self.nodes.push(Node::Terminal { payoff: 0.0 });

        let node = match self.game.node_type(state) {
            NodeType::Terminal => Node::Terminal {
                payoff: self.game.payoff(state),
            },
            NodeType::Chance => {
                let outcomes = self.game.chance_outcomes(state);
                let mut children = Vec::with_capacity(outcomes.len());
                let mut probs = Vec::with_capacity(outcomes.len());
                for (p, next) in &outcomes {
                    probs.push(*p);
                    children.push(self.expand(next));
                }
                Node::Chance { children, probs }
            }
            NodeType::Decision(player) => {
                let actions = self.game.actions(state);
                let infoset = self.infoset_for(state, player, &actions);
                self.infosets[infoset].nodes.push(id);
                let children = actions
                    .iter()
                    .map(|&a| {
                        let next = self.game.apply(state, a);
                        self.expand(&next)
                    })
                    .collect();
                Node::Decision {
                    player,
                    infoset,
                    children,
                }
            }
        };
        self.nodes[id] = node;
        id
    }

    fn infoset_for(&mut self, state: &G::State, player: usize, actions: &[G::Action]) -> usize {
        let key = self.game.info_key(state);
        let labels: Vec<String> = actions.iter().map(|a| format!("{a:?}")).collect();
        if let Some(&i) = self.index.get(&key) {
            let info = &self.infosets[i];
            assert_eq!(
                info.player, player,
                "infoset {key:?} has two different actors"
            );
            assert_eq!(
                info.actions, labels,
                "infoset {key:?} has inconsistent actions"
            );
            return i;
        }
        let i = self.infosets.len();
        self.infosets.push(InfoSet {
            player,
            label: format!("{key:?}").trim_matches('"').to_string(),
            actions: labels,
            nodes: Vec::new(),
            offset: self.num_actions,
        });
        self.num_actions += actions.len();
        self.index.insert(key, i);
        i
    }
}
