//! Two-card combos and ranges.
//!
//! There are C(52, 2) = 1,326 starting combos. Combo `i` is the pair of card
//! indices (hi, lo), hi > lo, in colex order: `i = hi * (hi - 1) / 2 + lo`.

use std::fmt;

use eval::{Card, card::RANK_CHARS};

pub const NUM_COMBOS: usize = 1326;

/// Index of the combo holding cards `a` and `b` (in either order).
pub fn combo_index(a: Card, b: Card) -> usize {
    let (x, y) = (a.index() as usize, b.index() as usize);
    debug_assert_ne!(x, y);
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    hi * (hi - 1) / 2 + lo
}

/// The two cards of combo `i`, higher card index first.
pub fn combo_cards(i: usize) -> (Card, Card) {
    static TABLE: std::sync::OnceLock<Vec<(Card, Card)>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Vec::with_capacity(NUM_COMBOS);
        for hi in 1..52u8 {
            for lo in 0..hi {
                t.push((Card::from_index(hi), Card::from_index(lo)));
            }
        }
        t
    })[i]
}

/// Strategic class of a combo, e.g. `AKs`, `T9o`, `77`.
pub fn hand_class(i: usize) -> String {
    let (a, b) = combo_cards(i);
    let (hi, lo) = if a.rank() >= b.rank() { (a, b) } else { (b, a) };
    let (rh, rl) = (
        RANK_CHARS[hi.rank() as usize],
        RANK_CHARS[lo.rank() as usize],
    );
    if hi.rank() == lo.rank() {
        format!("{rh}{rl}")
    } else if hi.suit() == lo.suit() {
        format!("{rh}{rl}s")
    } else {
        format!("{rh}{rl}o")
    }
}

/// A weight in [0, 1] for each of the 1,326 combos.
#[derive(Clone, PartialEq)]
pub struct Range {
    pub weights: Vec<f32>,
}

impl fmt::Debug for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.weights.iter().filter(|&&w| w > 0.0).count();
        write!(f, "Range({n} combos)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRangeError(pub String);

impl fmt::Display for ParseRangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid range token {:?}", self.0)
    }
}

impl std::error::Error for ParseRangeError {}

fn rank_of(c: char) -> Option<u8> {
    RANK_CHARS
        .iter()
        .position(|&r| r == c.to_ascii_uppercase())
        .map(|p| p as u8)
}

impl Range {
    pub fn empty() -> Range {
        Range {
            weights: vec![0.0; NUM_COMBOS],
        }
    }

    /// Every combo with weight 1.
    pub fn full() -> Range {
        Range {
            weights: vec![1.0; NUM_COMBOS],
        }
    }

    /// Number of combos with positive weight.
    pub fn len(&self) -> usize {
        self.weights.iter().filter(|&&w| w > 0.0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn set_class(&mut self, r1: u8, r2: u8, suited: Option<bool>, w: f32) {
        for s1 in 0..4 {
            for s2 in 0..4 {
                let (a, b) = (Card::new(r1, s1), Card::new(r2, s2));
                if a == b {
                    continue;
                }
                if r1 == r2 && s1 >= s2 {
                    continue; // each pair combo once
                }
                match suited {
                    Some(true) if s1 != s2 => continue,
                    Some(false) if s1 == s2 => continue,
                    _ => {}
                }
                self.weights[combo_index(a, b)] = w;
            }
        }
    }

    /// Parses standard range notation. Tokens are comma-separated:
    ///
    /// - `random` / `any`: every combo
    /// - `QQ`, `QQ+`, `22-55`: pairs
    /// - `AK`, `AKs`, `AKo`, `ATs+` (AT..AK suited), `KQo-K9o`
    /// - `AsKs`: one specific combo
    /// - any token may end in `:0.5` to set a weight
    pub fn parse(text: &str) -> Result<Range, ParseRangeError> {
        let mut range = Range::empty();
        for raw in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let err = || ParseRangeError(raw.to_string());
            let (token, w) = match raw.split_once(':') {
                Some((t, w)) => (t.trim(), w.trim().parse::<f32>().map_err(|_| err())?),
                None => (raw, 1.0),
            };
            if !(0.0..=1.0).contains(&w) {
                return Err(err());
            }
            let lower = token.to_ascii_lowercase();
            if lower == "random" || lower == "any" {
                range.weights.fill(w);
                continue;
            }
            let chars: Vec<char> = token.chars().collect();
            // Specific combo like AsKs.
            if chars.len() == 4 && chars[1].is_ascii_lowercase() && chars[3].is_ascii_lowercase() {
                let a: Card = token[0..2].parse().map_err(|_| err())?;
                let b: Card = token[2..4].parse().map_err(|_| err())?;
                if a == b {
                    return Err(err());
                }
                range.weights[combo_index(a, b)] = w;
                continue;
            }
            // Dash ranges: 22-55, KQo-K9o.
            if let Some((lo_tok, hi_tok)) = token.split_once('-') {
                let p = |t: &str| -> Option<(u8, u8, Option<bool>)> {
                    let c: Vec<char> = t.chars().collect();
                    let r1 = rank_of(*c.first()?)?;
                    let r2 = rank_of(*c.get(1)?)?;
                    let s = match c.get(2) {
                        Some('s') => Some(true),
                        Some('o') => Some(false),
                        None => None,
                        _ => return None,
                    };
                    Some((r1, r2, s))
                };
                let (a1, a2, sa) = p(lo_tok).ok_or_else(err)?;
                let (b1, b2, sb) = p(hi_tok).ok_or_else(err)?;
                if sa != sb {
                    return Err(err());
                }
                if a1 == a2 && b1 == b2 {
                    for r in a1.min(b1)..=a1.max(b1) {
                        range.set_class(r, r, None, w);
                    }
                } else if a1 == b1 && a1 != a2 && b1 != b2 {
                    for r in a2.min(b2)..=a2.max(b2) {
                        if r >= a1 {
                            return Err(err());
                        }
                        range.set_class(a1, r, sa, w);
                    }
                } else {
                    return Err(err());
                }
                continue;
            }
            // Single class, optionally with +.
            let (body, plus) = match token.strip_suffix('+') {
                Some(b) => (b, true),
                None => (token, false),
            };
            let c: Vec<char> = body.chars().collect();
            if !(2..=3).contains(&c.len()) {
                return Err(err());
            }
            let r1 = rank_of(c[0]).ok_or_else(err)?;
            let r2 = rank_of(c[1]).ok_or_else(err)?;
            let suited = match c.get(2) {
                Some('s') => Some(true),
                Some('o') => Some(false),
                None => None,
                _ => return Err(err()),
            };
            let (r1, r2) = if r1 >= r2 { (r1, r2) } else { (r2, r1) };
            if r1 == r2 {
                if suited.is_some() {
                    return Err(err());
                }
                let top = if plus { 12 } else { r1 };
                for r in r1..=top {
                    range.set_class(r, r, None, w);
                }
            } else {
                let top = if plus { r1 - 1 } else { r2 };
                for r in r2..=top {
                    range.set_class(r1, r, suited, w);
                }
            }
        }
        Ok(range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_indexing_is_a_bijection() {
        let mut seen = vec![false; NUM_COMBOS];
        for i in 0..NUM_COMBOS {
            let (a, b) = combo_cards(i);
            assert!(a.index() > b.index());
            assert_eq!(combo_index(a, b), i);
            assert_eq!(combo_index(b, a), i);
            seen[combo_index(a, b)] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn class_counts() {
        let count = |s: &str| Range::parse(s).unwrap().len();
        assert_eq!(count("AA"), 6);
        assert_eq!(count("AKs"), 4);
        assert_eq!(count("AKo"), 12);
        assert_eq!(count("AK"), 16);
        assert_eq!(count("QQ+"), 18);
        assert_eq!(count("22-44"), 18);
        assert_eq!(count("ATs+"), 16);
        assert_eq!(count("KQo-K9o"), 48);
        assert_eq!(count("AsKs"), 1);
        assert_eq!(count("random"), 1326);
        assert_eq!(count("AA, AA, KK"), 12);
    }

    #[test]
    fn weights_and_errors() {
        let r = Range::parse("AA:0.5,KK").unwrap();
        let aa = combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
        let kk = combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap());
        assert_eq!(r.weights[aa], 0.5);
        assert_eq!(r.weights[kk], 1.0);
        for bad in ["AAs", "AX", "AK:2", "AsAs", "22-AKs", "AKs-AQo", "A"] {
            assert!(Range::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn classes_print_canonically() {
        let i = combo_index("Ts".parse().unwrap(), "As".parse().unwrap());
        assert_eq!(hand_class(i), "ATs");
        let i = combo_index("9h".parse().unwrap(), "Tc".parse().unwrap());
        assert_eq!(hand_class(i), "T9o");
    }
}
