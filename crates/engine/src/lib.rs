//! Generic counterfactual regret minimization engine.
//!
//! The engine knows nothing about poker. A game plugs in by implementing [`Game`];
//! small games are then compiled once into a flat [`Tree`], and every exact algorithm
//! (CFR, CFR+, Discounted CFR, best response) runs on that tree with plain array
//! indexing instead of hashing.
//!
//! Conventions: two players (0 and 1), zero-sum, payoffs reported from player 0's
//! point of view. Player 1's payoff is the negation.

pub mod best_response;
pub mod cfr;
pub mod game;
pub mod mccfr;
pub mod regret;
pub mod tree;

pub use best_response::{best_response_value, expected_value, exploitability};
pub use cfr::{Solver, Variant};
pub use game::{Game, NodeType};
pub use mccfr::{BettingNode, Mccfr, SampledGame, SplitMix, Weighting};
pub use tree::{InfoSet, Node, Tree};
