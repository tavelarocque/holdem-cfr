//! Export a trained blueprint for the browser table.
//!
//! The page cannot hold the 138 MB bucket tables, so it recomputes each bucket
//! from the cards: river HS against all 990 opponent hands, turn E[HS²] over the
//! 46 rivers, flop E[HS²] over the 47 × 46 runouts, with the same summation
//! order and the same f32 roundings as [`crate::strength`]. It then places the
//! value with the exported percentile thresholds. Preflop needs only the map from
//! hand class name to class index.
//!
//! The export is a single JSON object:
//!
//! - `nodes`: the abstract betting tree in pre-order. An action node is
//!   `[player, street, children, actions, offset]` where actions are `-1` fold,
//!   `0` check or call, and a positive raise-to total. A terminal is
//!   `[c0, c1, folded]` with chips contributed and the folding seat (or -1).
//! - `strategy`: base64 bytes, the average strategy quantized to 0..255 per
//!   (node, bucket, action), laid out by `offset + bucket * actions + action`.
//! - `thresholds`: flop, turn and river percentile thresholds.
//! - `preflop`: class names by preflop bucket.

use std::fmt::Write as _;

use engine::{BettingNode, Mccfr, SampledGame};

use crate::blueprint::{Blueprint, CardAbstraction, preflop_class_names};
use crate::rules::Action;

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Quantizes a probability vector to bytes that still sum to about 255 and
/// never drop the most likely action to zero.
pub fn quantize_strategy(p: &[f64]) -> Vec<u8> {
    let mut q: Vec<u8> = p.iter().map(|x| (x * 255.0).round() as u8).collect();
    if q.iter().all(|&x| x == 0) {
        let best = p
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0);
        q[best] = 255;
    }
    q
}

/// The blueprint as a JSON string for the web table.
pub fn export_json<A: CardAbstraction>(
    bp: &Blueprint<A>,
    m: &Mccfr<Blueprint<A>>,
    thresholds: &[Vec<f32>; 3],
) -> String {
    let counts = bp.cards.counts();
    let mut nodes = String::from("[");
    let mut strategy = Vec::with_capacity(m.len());
    for (id, node) in bp.nodes().iter().enumerate() {
        if id > 0 {
            nodes.push(',');
        }
        match node {
            BettingNode::Action {
                player,
                street,
                children,
            } => {
                let acts: Vec<i32> = bp
                    .node_actions(id)
                    .iter()
                    .map(|a| match a {
                        Action::Fold => -1,
                        Action::Check | Action::Call => 0,
                        Action::RaiseTo(x) => *x,
                    })
                    .collect();
                let offset = strategy.len();
                for bucket in 0..counts[*street] {
                    strategy.extend(quantize_strategy(&m.average_strategy(id, bucket)));
                }
                write!(nodes, "[{player},{street},{children:?},{acts:?},{offset}]").unwrap();
            }
            BettingNode::Terminal => {
                let info = bp.info(id);
                let folded = info.folded.map_or(-1, |f| f as i32);
                write!(
                    nodes,
                    "[{},{},{folded}]",
                    info.contributed[0], info.contributed[1]
                )
                .unwrap();
            }
        }
    }
    nodes.push(']');
    assert_eq!(strategy.len(), m.len());
    let names: Vec<String> = preflop_class_names()
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect();
    let th: Vec<String> = thresholds
        .iter()
        .map(|t| {
            let v: Vec<String> = t.iter().map(|x| format!("{x:?}")).collect();
            format!("[{}]", v.join(","))
        })
        .collect();
    format!(
        "{{\"iterations\":{},\"params\":{{\"sb\":{},\"bb\":{},\"stack\":{}}},\"counts\":{:?},\
         \"thresholds\":[{}],\"preflop\":[{}],\"nodes\":{},\"strategy\":\"{}\"}}",
        m.iterations(),
        bp.params.small_blind,
        bp.params.big_blind,
        bp.params.stack,
        counts,
        th.join(","),
        names.join(","),
        nodes,
        base64(&strategy)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (raw, enc) in cases {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn quantized_strategy_keeps_the_mode() {
        assert_eq!(quantize_strategy(&[0.5, 0.5]), vec![128, 128]);
        assert_eq!(quantize_strategy(&[0.001, 0.001, 0.0015]), vec![0, 0, 255]);
    }
}
