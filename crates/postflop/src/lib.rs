//! Range-vs-range no-limit hold'em postflop solver.
//!
//! Give it a board, two ranges, a pot, effective stacks and a bet-size menu, and
//! it computes an equilibrium of that betting tree with vectorized CFR, reporting
//! exact exploitability (in chips and as a percentage of the pot) inside the tree.
//!
//! ```
//! use postflop::{Algorithm, BetConfig, Range, Solver, Spot, SpotConfig};
//! let cfg = SpotConfig {
//!     board: eval::parse_cards("Kh 9d 5c 3s 2h").unwrap(),
//!     ranges: [Range::parse("KK,76").unwrap(), Range::parse("QQ").unwrap()],
//!     pot: 100,
//!     stack: 100,
//!     bets: BetConfig::symmetric(&[1.0], &[], 1, false),
//!     isomorphism: true,
//! };
//! let spot = Spot::new(&cfg);
//! let mut solver = Solver::new(&spot, Algorithm::DCFR);
//! for _ in 0..500 { solver.iterate(); }
//! assert!(solver.exploitability_pct() < 0.5);
//! ```

pub mod range;
pub mod solver;
pub mod spot;
pub mod tree;

pub use range::{NUM_COMBOS, Range, combo_cards, combo_index, hand_class};
pub use solver::{Algorithm, Solver};
pub use spot::{Spot, SpotConfig};
pub use tree::{Action, BetConfig, Node, NodeKind, PublicTree};
