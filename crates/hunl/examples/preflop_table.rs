//! Prints exact preflop E[HS] and E[HS^2] for all 169 classes as JSON.
use hunl::StrengthTables;
use hunl::blueprint::preflop_class_names;

fn main() {
    let t = StrengthTables::build(&|m: &str| eprintln!("{m}"));
    let names = preflop_class_names();
    let rows: Vec<String> = (0..169)
        .map(|i| {
            format!(
                "\"{}\":[{:.4},{:.4}]",
                names[i], t.preflop.ehs[i], t.preflop.ehs2[i]
            )
        })
        .collect();
    println!("{{{}}}", rows.join(","));
}
