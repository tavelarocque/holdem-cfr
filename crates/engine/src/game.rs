//! The interface every game implements.

use std::fmt::Debug;
use std::hash::Hash;

/// What kind of node a state is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    /// The game is over; ask [`Game::payoff`].
    Terminal,
    /// Nature moves; ask [`Game::chance_outcomes`].
    Chance,
    /// Player 0 or 1 must choose an action.
    Decision(usize),
}

/// A two-player, zero-sum, perfect-recall extensive-form game.
///
/// States are values: `apply` returns a new state rather than mutating, which keeps
/// tree construction and recursive traversals trivially correct. Games whose states
/// are expensive to clone can switch to an undo-based interface later without
/// touching the algorithms that only consume the compiled [`crate::Tree`].
pub trait Game {
    /// Full game state (both players' private information included).
    type State: Clone;
    /// A player action.
    type Action: Copy + Debug + Eq;
    /// Identifies an information set: everything the acting player can observe.
    /// Two states with equal keys must offer the same legal actions in the same order.
    type InfoKey: Clone + Eq + Hash + Debug;

    /// The state before anything has happened.
    fn root(&self) -> Self::State;

    /// Terminal, chance, or whose turn it is.
    fn node_type(&self, state: &Self::State) -> NodeType;

    /// Payoff to player 0 at a terminal state.
    fn payoff(&self, state: &Self::State) -> f64;

    /// Every chance outcome as (probability, next state). Probabilities sum to 1.
    fn chance_outcomes(&self, state: &Self::State) -> Vec<(f64, Self::State)>;

    /// Legal actions for the player to act, in a fixed order.
    fn actions(&self, state: &Self::State) -> Vec<Self::Action>;

    /// The state after the acting player takes `action`.
    fn apply(&self, state: &Self::State, action: Self::Action) -> Self::State;

    /// Information-set key for the player to act.
    fn info_key(&self, state: &Self::State) -> Self::InfoKey;
}
