use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use engine::{Solver, Tree, Variant, expected_value, exploitability};
use games::{Kuhn, Leduc};

#[derive(Parser)]
#[command(name = "holdem-cfr", about = "CFR poker solver")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Solve a small game and report value, exploitability and (optionally) the strategy.
    Solve {
        #[arg(long, value_enum)]
        game: GameName,
        #[arg(long, value_enum, default_value = "cfr-plus")]
        algo: Algo,
        #[arg(long, default_value_t = 1000)]
        iters: u64,
        /// Print the average strategy at every information set.
        #[arg(long)]
        print: bool,
    },
    /// Record exploitability vs iterations for every algorithm into a CSV.
    Curves {
        #[arg(long, value_enum)]
        game: GameName,
        #[arg(long, default_value_t = 10_000)]
        iters: u64,
        #[arg(long, default_value = "results/curves.csv")]
        out: PathBuf,
    },
    /// Rank a 5-7 card hand, e.g. `holdem-cfr eval "As Ks Qs Js Ts"`.
    Eval { cards: String },
    /// Time the evaluator on every 7-card hand (133,784,560), single-threaded.
    BenchEval,
    /// Solve a no-limit postflop spot range-vs-range (river or turn).
    ///
    /// Example: holdem-cfr postflop --board "Qs Jh 7c 4d 2s" --oop "22+,AJs+,KQs" --ip "random"
    Postflop(Box<PostflopArgs>),
    /// Train the no-limit blueprint with external-sampling MCCFR.
    Blueprint {
        /// Bucket file from `holdem-cfr abstraction`.
        #[arg(long, default_value = "data/buckets.bin")]
        buckets: PathBuf,
        /// Use made-hand categories instead of a bucket file (quick smoke runs).
        #[arg(long)]
        category: bool,
        /// Wall-clock training time.
        #[arg(long, default_value_t = 10.0)]
        minutes: f64,
        /// Iterations per batch (progress, discounting and checkpoints happen between batches).
        #[arg(long, default_value_t = 2_000_000)]
        batch: u64,
        /// Apply Linear-CFR discounting after each batch for this many minutes.
        #[arg(long, default_value_t = 5.0)]
        linear_minutes: f64,
        /// Checkpoint file (written after every batch; resumed from if present).
        #[arg(long, default_value = "data/blueprint.bin")]
        checkpoint: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Worker threads (default: all cores). Fewer threads run cooler on fanless laptops.
        #[arg(long)]
        threads: Option<usize>,
        /// On resume, keep the regrets but zero the average strategy and rebuild it
        /// from here on (discards early iterations and f32-damaged sums).
        #[arg(long)]
        reset_average: bool,
    },
    /// Play the trained blueprint against baseline bots (duplicate matches, mbb/hand ± 95% CI).
    Arena {
        #[arg(long, default_value = "data/buckets.bin")]
        buckets: PathBuf,
        #[arg(long, default_value = "data/blueprint.bin")]
        checkpoint: PathBuf,
        /// Opponents: call, raise, random, equity, or a path to another checkpoint.
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "call,raise,random,equity"
        )]
        vs: Vec<String>,
        /// Duplicate pairs per opponent (each pair is two hands).
        #[arg(long, default_value_t = 100_000)]
        pairs: usize,
    },
    /// Local best response: a lower bound on each checkpoint's exploitability (mbb/hand ± 95% CI).
    Lbr {
        #[arg(long, default_value = "data/buckets.bin")]
        buckets: PathBuf,
        /// Checkpoints to measure.
        #[arg(long, value_delimiter = ',', default_value = "data/blueprint.bin")]
        checkpoints: Vec<PathBuf>,
        /// Duplicate pairs per checkpoint (each pair is two hands).
        #[arg(long, default_value_t = 20_000)]
        pairs: usize,
        /// Sampled runouts for LBR's flop equities.
        #[arg(long, default_value_t = 128)]
        flop_samples: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Export a trained blueprint as JSON for the web table, plus a fixture of
    /// random deals with their buckets so the browser code can be checked.
    ExportWeb {
        #[arg(long, default_value = "data/buckets.bin")]
        buckets: PathBuf,
        #[arg(long, default_value = "data/blueprint.bin")]
        checkpoint: PathBuf,
        /// Must match the `abstraction --samples` that built the bucket file.
        #[arg(long, default_value_t = 2_000_000)]
        samples: usize,
        #[arg(long, default_value = "web/table/blueprint.json")]
        out: PathBuf,
        #[arg(long, default_value = "web/table/fixtures/buckets.json")]
        fixture: PathBuf,
        #[arg(long, default_value_t = 300)]
        fixture_deals: usize,
    },
    /// Build hand-strength tables and E[HS²] card buckets for the full game.
    Abstraction {
        /// Buckets on the flop, turn and river.
        #[arg(long, value_delimiter = ',', default_value = "50,50,50")]
        buckets: Vec<u16>,
        /// Random deals per street used to place bucket thresholds.
        #[arg(long, default_value_t = 2_000_000)]
        samples: usize,
        #[arg(long, default_value = "data/buckets.bin")]
        out: PathBuf,
    },
}

#[derive(clap::Args)]
struct PostflopArgs {
    /// Board cards (3-5), e.g. "Qs Jh 7c 4d".
    #[arg(long)]
    board: String,
    /// Out-of-position range, e.g. "22+,ATs+,KQo".
    #[arg(long)]
    oop: String,
    /// In-position range.
    #[arg(long)]
    ip: String,
    /// Pot at the start of the spot, in chips.
    #[arg(long, default_value_t = 100)]
    pot: i32,
    /// Effective stack behind, in chips.
    #[arg(long, default_value_t = 400)]
    stack: i32,
    /// Bet sizes in percent of pot, for both players.
    #[arg(long, value_delimiter = ',', default_value = "33,75")]
    bets: Vec<f64>,
    /// Raise sizes in percent of the pot after calling.
    #[arg(long, value_delimiter = ',', default_value = "60")]
    raises: Vec<f64>,
    /// Override OOP's bet sizes (percent of pot).
    #[arg(long, value_delimiter = ',')]
    oop_bets: Option<Vec<f64>>,
    /// Override IP's bet sizes (percent of pot).
    #[arg(long, value_delimiter = ',')]
    ip_bets: Option<Vec<f64>>,
    /// Flop bet sizes (percent of pot), overriding --bets on the flop.
    #[arg(long, value_delimiter = ',')]
    flop_bets: Option<Vec<f64>>,
    /// Turn bet sizes (percent of pot), overriding --bets on the turn.
    #[arg(long, value_delimiter = ',')]
    turn_bets: Option<Vec<f64>>,
    /// River bet sizes (percent of pot), overriding --bets on the river.
    #[arg(long, value_delimiter = ',')]
    river_bets: Option<Vec<f64>>,
    /// Flop raise sizes, overriding --raises on the flop.
    #[arg(long, value_delimiter = ',')]
    flop_raises: Option<Vec<f64>>,
    /// Turn raise sizes, overriding --raises on the turn.
    #[arg(long, value_delimiter = ',')]
    turn_raises: Option<Vec<f64>>,
    /// River raise sizes, overriding --raises on the river.
    #[arg(long, value_delimiter = ',')]
    river_raises: Option<Vec<f64>>,
    /// Bets plus raises allowed per street.
    #[arg(long, default_value_t = 3)]
    max_raises: u8,
    /// Disable suit isomorphism.
    #[arg(long)]
    no_iso: bool,
    /// Worker threads (default: all cores).
    #[arg(long)]
    threads: Option<usize>,
    /// Refuse to solve if regrets and strategy sums would exceed this many MB.
    #[arg(long, default_value_t = 4096)]
    max_memory_mb: usize,
    /// Do not add an all-in option.
    #[arg(long)]
    no_allin: bool,
    #[arg(long, value_enum, default_value = "dcfr")]
    algo: PostflopAlgo,
    /// Maximum iterations.
    #[arg(long, default_value_t = 1000)]
    iters: u32,
    /// Stop once exploitability falls below this percent of the pot.
    #[arg(long, default_value_t = 0.3)]
    target: f64,
    /// Hand classes to print in the strategy table.
    #[arg(long, default_value_t = 30)]
    show: usize,
    /// Write the convergence log (iteration, seconds, exploitability %) here.
    #[arg(long)]
    log: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum PostflopAlgo {
    Dcfr,
    CfrPlus,
}

#[derive(Clone, Copy, ValueEnum)]
enum GameName {
    Kuhn,
    Leduc,
}

#[derive(Clone, Copy, ValueEnum)]
enum Algo {
    Cfr,
    CfrPlus,
    Linear,
    Dcfr,
}

impl Algo {
    const ALL: [Algo; 4] = [Algo::Cfr, Algo::CfrPlus, Algo::Linear, Algo::Dcfr];

    fn variant(self) -> Variant {
        match self {
            Algo::Cfr => Variant::Vanilla,
            Algo::CfrPlus => Variant::CfrPlus,
            Algo::Linear => Variant::LINEAR,
            Algo::Dcfr => Variant::DCFR,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Algo::Cfr => "CFR",
            Algo::CfrPlus => "CFR+",
            Algo::Linear => "LCFR",
            Algo::Dcfr => "DCFR",
        }
    }
}

fn build(game: GameName) -> Tree {
    match game {
        GameName::Kuhn => Tree::build(&Kuhn),
        GameName::Leduc => Tree::build(&Leduc),
    }
}

/// Checkpoints roughly evenly spaced on a log scale, always including `max`.
fn checkpoints(max: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut x = 1.0f64;
    while (x as u64) < max {
        let k = x as u64;
        if out.last() != Some(&k) {
            out.push(k);
        }
        x *= 1.15;
    }
    out.push(max);
    out
}

fn main() -> std::io::Result<()> {
    match Cli::parse().command {
        Command::Solve {
            game,
            algo,
            iters,
            print,
        } => {
            let tree = build(game);
            println!(
                "{} nodes, {} information sets ({} / {})",
                tree.nodes.len(),
                tree.infosets.len(),
                tree.infosets_of(0),
                tree.infosets_of(1)
            );
            let start = Instant::now();
            let mut solver = Solver::new(&tree, algo.variant());
            solver.run(iters);
            let secs = start.elapsed().as_secs_f64();
            let avg = solver.average_strategy();
            println!(
                "{} x {iters}: {secs:.2}s ({:.0} it/s)",
                algo.name(),
                iters as f64 / secs
            );
            println!("game value (player 0): {:+.6}", expected_value(&tree, &avg));
            println!("exploitability:        {:.3e}", exploitability(&tree, &avg));
            if print {
                for info in &tree.infosets {
                    let probs: Vec<String> = info
                        .actions
                        .iter()
                        .zip(&avg[info.range()])
                        .map(|(a, p)| format!("{a}={p:.3}"))
                        .collect();
                    println!("  P{} {:<14} {}", info.player, info.label, probs.join("  "));
                }
            }
        }
        Command::Eval { cards } => match eval::parse_cards(&cards) {
            Ok(c) if (5..=7).contains(&c.len()) => println!("{}", eval::evaluate(&c)),
            Ok(c) => eprintln!("need 5 to 7 cards, got {}", c.len()),
            Err(e) => eprintln!("{e}"),
        },
        Command::BenchEval => bench_eval(),
        Command::Postflop(args) => postflop_cmd(*args)?,
        Command::Blueprint {
            buckets,
            category,
            minutes,
            batch,
            linear_minutes,
            checkpoint,
            seed,
            threads,
            reset_average,
        } => {
            if let Some(t) = threads {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(t)
                    .build_global()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            let opts = TrainOpts {
                minutes,
                batch,
                linear_minutes,
                checkpoint,
                seed,
                reset_average,
            };
            if category {
                train(hunl::CategoryBuckets::default(), &opts)?
            } else {
                let b = hunl::Buckets::load(&buckets).map_err(|e| {
                    std::io::Error::new(
                        e.kind(),
                        format!(
                            "{}: {e} (run `holdem-cfr abstraction` first)",
                            buckets.display()
                        ),
                    )
                })?;
                train(b, &opts)?
            }
        }
        Command::Abstraction {
            buckets,
            samples,
            out,
        } => abstraction_cmd(&buckets, samples, &out)?,
        Command::Arena {
            buckets,
            checkpoint,
            vs,
            pairs,
        } => arena_cmd(&buckets, &checkpoint, &vs, pairs)?,
        Command::Lbr {
            buckets,
            checkpoints,
            pairs,
            flop_samples,
            seed,
        } => lbr_cmd(&buckets, &checkpoints, pairs, flop_samples, seed)?,
        Command::ExportWeb {
            buckets,
            checkpoint,
            samples,
            out,
            fixture,
            fixture_deals,
        } => export_web_cmd(
            &buckets,
            &checkpoint,
            samples,
            &out,
            &fixture,
            fixture_deals,
        )?,
        Command::Curves { game, iters, out } => {
            let tree = build(game);
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut w = BufWriter::new(File::create(&out)?);
            writeln!(w, "algo,iteration,exploitability,seconds")?;
            let points = checkpoints(iters);
            for algo in Algo::ALL {
                let mut solver = Solver::new(&tree, algo.variant());
                let start = Instant::now();
                let mut solve_secs = 0.0;
                for &target in &points {
                    let t0 = Instant::now();
                    solver.run(target - solver.iteration());
                    solve_secs += t0.elapsed().as_secs_f64();
                    let e = exploitability(&tree, &solver.average_strategy());
                    writeln!(w, "{},{target},{e:.6e},{solve_secs:.4}", algo.name())?;
                }
                let e = exploitability(&tree, &solver.average_strategy());
                println!(
                    "{:<5} {iters} iterations: exploitability {e:.3e} ({:.1}s wall)",
                    algo.name(),
                    start.elapsed().as_secs_f64()
                );
            }
            println!("wrote {}", out.display());
        }
    }
    Ok(())
}

/// Exhaustive single-threaded enumeration of all 7-card hands.
fn bench_eval() {
    use eval::{Card, Hand};
    eval::warm_up();
    let cards: Vec<Card> = Card::deck().collect();
    let start = Instant::now();
    let mut counts = [0u64; 9];
    for a in 0..52 {
        let h1 = Hand::EMPTY.plus(cards[a]);
        for b in a + 1..52 {
            let h2 = h1.plus(cards[b]);
            for c in b + 1..52 {
                let h3 = h2.plus(cards[c]);
                for d in c + 1..52 {
                    let h4 = h3.plus(cards[d]);
                    for e in d + 1..52 {
                        let h5 = h4.plus(cards[e]);
                        for f in e + 1..52 {
                            let h6 = h5.plus(cards[f]);
                            for &card in &cards[f + 1..] {
                                counts[h6.plus(card).rank().category() as usize] += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    let secs = start.elapsed().as_secs_f64();
    let total: u64 = counts.iter().sum();
    for (cat, n) in eval::Category::ALL.iter().zip(counts) {
        println!("{:<16} {n:>11}", cat.name());
    }
    println!(
        "{total} hands in {secs:.2}s = {:.1}M hands/s (single thread)",
        total as f64 / secs / 1e6
    );
}

fn postflop_cmd(a: PostflopArgs) -> std::io::Result<()> {
    use postflop::{Algorithm, BetConfig, NodeKind, Range, Solver, Spot, SpotConfig};

    let fail = |msg: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg);
    let board = eval::parse_cards(&a.board).map_err(|e| fail(e.to_string()))?;
    if !(3..=5).contains(&board.len()) {
        return Err(fail("board needs 3 to 5 cards".into()));
    }
    let oop = Range::parse(&a.oop).map_err(|e| fail(e.to_string()))?;
    let ip = Range::parse(&a.ip).map_err(|e| fail(e.to_string()))?;
    if let Some(t) = a.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .map_err(|e| fail(e.to_string()))?;
    }
    let pct = |v: &[f64]| v.iter().map(|x| x / 100.0).collect::<Vec<_>>();
    let mut bets = BetConfig::symmetric(&pct(&a.bets), &pct(&a.raises), a.max_raises, !a.no_allin);
    let per_street = [
        (&a.flop_bets, &a.flop_raises),
        (&a.turn_bets, &a.turn_raises),
        (&a.river_bets, &a.river_raises),
    ];
    for (street, (b, r)) in per_street.into_iter().enumerate() {
        let b = pct(b.as_deref().unwrap_or(&a.bets));
        let r = pct(r.as_deref().unwrap_or(&a.raises));
        bets = bets.with_street(street, &b, &r);
    }
    if let Some(b) = &a.oop_bets {
        bets = bets.with_player_bets(0, &pct(b));
    }
    if let Some(b) = &a.ip_bets {
        bets = bets.with_player_bets(1, &pct(b));
    }
    let build = Instant::now();
    let spot = Spot::new(&SpotConfig {
        board: board.clone(),
        ranges: [oop, ip],
        pot: a.pot,
        stack: a.stack,
        bets,
        isomorphism: !a.no_iso,
    });
    let need = postflop::solver::required_memory(&spot);
    if need > a.max_memory_mb * 1_000_000 {
        return Err(fail(format!(
            "this tree needs {:.0} MB (limit {} MB): use fewer bet sizes or raise --max-memory-mb",
            need as f64 / 1e6,
            a.max_memory_mb
        )));
    }
    let algorithm = match a.algo {
        PostflopAlgo::Dcfr => Algorithm::DCFR,
        PostflopAlgo::CfrPlus => Algorithm::CfrPlus,
    };
    let mut solver = Solver::new(&spot, algorithm);
    println!(
        "{} OOP hands, {} IP hands, {} public nodes ({} action), {:.1} MB, {} suit symmetries, {} threads, built in {:.2}s",
        spot.num_hands(0),
        spot.num_hands(1),
        spot.tree.nodes.len(),
        spot.tree.num_action_nodes(),
        solver.memory_bytes() as f64 / 1e6,
        spot.symmetries.len(),
        rayon::current_num_threads(),
        build.elapsed().as_secs_f64()
    );

    let mut log = match &a.log {
        Some(path) => {
            let mut w = BufWriter::new(File::create(path)?);
            writeln!(w, "iteration,seconds,exploitability_pct")?;
            Some(w)
        }
        None => None,
    };
    let start = Instant::now();
    let mut solve_secs = 0.0;
    let mut expl = solver.exploitability_pct();
    let mut next_check = 1u32;
    while solver.iteration() < a.iters {
        let t0 = Instant::now();
        solver.iterate();
        solve_secs += t0.elapsed().as_secs_f64();
        if solver.iteration() == next_check || solver.iteration() == a.iters {
            expl = solver.exploitability_pct();
            println!(
                "iter {:>5}  {:>7.2}s  exploitability {:.3}% of pot",
                solver.iteration(),
                solve_secs,
                expl
            );
            if let Some(w) = log.as_mut() {
                writeln!(w, "{},{solve_secs:.4},{expl:.6}", solver.iteration())?;
            }
            if expl < a.target {
                break;
            }
            next_check = (next_check * 2).min(next_check + 50);
        }
    }
    let half = a.pot as f64 / 2.0;
    println!(
        "done: {} iterations, {:.2}s, exploitability {:.3}% of pot",
        solver.iteration(),
        start.elapsed().as_secs_f64(),
        expl
    );
    println!(
        "EV (share of the {} pot): OOP {:.2}, IP {:.2}",
        a.pot,
        solver.expected_value(0) + half,
        solver.expected_value(1) + half
    );

    // Strategy at the root, by hand class.
    let NodeKind::Action {
        actions, children, ..
    } = &spot.tree.nodes[0].kind
    else {
        return Ok(());
    };
    let freqs = solver.action_frequencies(0, &spot.weights[0]);
    println!("\nOOP at the root:");
    for (act, f) in &freqs {
        println!("  {:<12} {:>6.1}%", act.to_string(), f * 100.0);
    }
    print_class_table(&spot, &solver, 0, 0, &spot.weights[0], &board, a.show);

    for (k, &child) in children.iter().enumerate() {
        if let NodeKind::Action { player: 1, .. } = spot.tree.nodes[child].kind {
            let line: Vec<String> = solver
                .action_frequencies(child, &spot.weights[1])
                .iter()
                .map(|(act, f)| format!("{act} {:.1}%", f * 100.0))
                .collect();
            println!("IP after OOP {}: {}", actions[k], line.join(", "));
        }
    }
    Ok(())
}

/// Range-weighted strategy per hand class at `node`, strongest classes first.
fn print_class_table(
    spot: &postflop::Spot,
    solver: &postflop::Solver,
    node: usize,
    player: usize,
    reach: &[f32],
    board: &[eval::Card],
    show: usize,
) {
    use std::collections::HashMap;
    let postflop::NodeKind::Action { actions, .. } = &spot.tree.nodes[node].kind else {
        return;
    };
    let n = spot.num_hands(player);
    let sigma = solver.average_strategy(node);
    // class -> (weight, strength sum, per-action weight)
    let mut classes: HashMap<String, (f64, f64, Vec<f64>)> = HashMap::new();
    let base = eval::Hand::from_cards(board);
    for h in 0..n {
        let w = reach[h] as f64;
        if w == 0.0 {
            continue;
        }
        let (c1, c2) = spot.hands[player][h];
        let strength = if board.len() == 5 {
            base.plus(c1).plus(c2).rank().0 as f64
        } else {
            (c1.rank().max(c2.rank()) * 13 + c1.rank().min(c2.rank())) as f64
                + if c1.rank() == c2.rank() { 200.0 } else { 0.0 }
        };
        let e = classes
            .entry(postflop::hand_class(spot.combos[player][h]))
            .or_insert((0.0, 0.0, vec![0.0; actions.len()]));
        e.0 += w;
        e.1 += w * strength;
        for (a, slot) in e.2.iter_mut().enumerate() {
            *slot += w * sigma[a * n + h] as f64;
        }
    }
    let mut rows: Vec<_> = classes.into_iter().collect();
    rows.sort_by(|a, b| (b.1.1 / b.1.0).total_cmp(&(a.1.1 / a.1.0)));
    let header: Vec<String> = actions
        .iter()
        .map(|a| format!("{:>10}", a.to_string()))
        .collect();
    println!("  {:<6} {:>7} {}", "class", "combos", header.join(""));
    for (class, (w, _, acts)) in rows.iter().take(show) {
        let cells: Vec<String> = acts
            .iter()
            .map(|x| format!("{:>9.0}%", 100.0 * x / w))
            .collect();
        println!("  {:<6} {:>7.2} {}", class, w, cells.join(""));
    }
    if rows.len() > show {
        println!("  ... {} more classes (use --show)", rows.len() - show);
    }
}

fn abstraction_cmd(counts: &[u16], samples: usize, out: &std::path::Path) -> std::io::Result<()> {
    let fail = |msg: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg.to_string());
    let [f, t, r] = counts else {
        return Err(fail("--buckets needs three numbers: flop,turn,river"));
    };
    let start = Instant::now();
    println!(
        "building strength tables on {} threads",
        rayon::current_num_threads()
    );
    let tables = hunl::StrengthTables::build(&|m: &str| println!("  {m}"));

    println!("\npreflop equity vs a random hand (E[HS]) and E[HS^2]:");
    for hand in [
        "AsAh", "KsKh", "QsQh", "AsKs", "AsKh", "Ts9s", "7s2h", "3s2h",
    ] {
        let c = eval::parse_cards(hand).map_err(|e| fail(&e.to_string()))?;
        println!(
            "  {hand}  E[HS] {:.4}  E[HS^2] {:.4}",
            tables.ehs([c[0], c[1]], &[]),
            tables.ehs2([c[0], c[1]], &[])
        );
    }

    let buckets = hunl::Buckets::from_strength(&tables, [*f, *t, *r], samples);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    buckets.save(out)?;
    println!(
        "\nbuckets {:?} written to {} ({:.1}s total)",
        buckets.counts,
        out.display(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

struct TrainOpts {
    minutes: f64,
    batch: u64,
    linear_minutes: f64,
    checkpoint: PathBuf,
    seed: u64,
    reset_average: bool,
}

fn train<A: hunl::CardAbstraction>(cards: A, o: &TrainOpts) -> std::io::Result<()> {
    use hunl::blueprint::{ChartKind, load_checkpoint, preflop_chart, save_checkpoint};
    let bp = hunl::Blueprint::new(
        hunl::Params::DEFAULT,
        hunl::ActionAbstraction::default_100bb(),
        cards,
    );
    let (entries, bytes) = bp.table_entries();
    println!(
        "abstract tree: action nodes per street {:?}, buckets {:?}, {} entries ({:.0} MB)",
        bp.action_nodes_per_street(),
        bp.cards.counts(),
        entries,
        bytes as f64 / 1e6
    );
    let m = engine::Mccfr::new(&bp, engine::Weighting::Uniform);
    if o.checkpoint.exists() {
        match load_checkpoint(&m, &o.checkpoint) {
            Ok(()) => println!(
                "resumed from {} at {} iterations",
                o.checkpoint.display(),
                m.iterations()
            ),
            Err(e) => println!("ignoring checkpoint {}: {e}", o.checkpoint.display()),
        }
        if o.reset_average {
            m.reset_average();
            println!("average strategy reset; regrets kept");
        }
    }
    if let Some(dir) = o.checkpoint.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let start = Instant::now();
    let mut batch_no = m.iterations() / o.batch;
    while start.elapsed().as_secs_f64() < o.minutes * 60.0 {
        let t0 = Instant::now();
        m.run(o.batch, o.seed.wrapping_add(m.iterations()));
        batch_no += 1;
        let elapsed_min = start.elapsed().as_secs_f64() / 60.0;
        let linear = elapsed_min < o.linear_minutes;
        if linear {
            let f = batch_no as f64 / (batch_no + 1) as f64;
            m.discount(f, f);
        }
        save_checkpoint(&m, &o.checkpoint)?;
        println!(
            "{:>6.1} min  {:>12} iterations  {:>8.0} it/s{}",
            elapsed_min,
            m.iterations(),
            o.batch as f64 / t0.elapsed().as_secs_f64(),
            if linear { "  (linear discount)" } else { "" }
        );
    }
    println!("\n{}", preflop_chart(&bp, &m, ChartKind::Play));
    println!("{}", preflop_chart(&bp, &m, ChartKind::Raise));
    println!("checkpoint: {}", o.checkpoint.display());
    Ok(())
}

fn arena_cmd(
    buckets: &std::path::Path,
    checkpoint: &std::path::Path,
    vs: &[String],
    pairs: usize,
) -> std::io::Result<()> {
    use hunl::arena::{
        Agent, BlueprintAgent, CallingStation, EquityAgent, Raiser, RandomAgent, duplicate_match,
    };
    use hunl::blueprint::load_checkpoint;
    let b = hunl::Buckets::load(buckets)?;
    let bp = hunl::Blueprint::new(
        hunl::Params::DEFAULT,
        hunl::ActionAbstraction::default_100bb(),
        b,
    );
    let m = engine::Mccfr::new(&bp, engine::Weighting::Uniform);
    load_checkpoint(&m, checkpoint)?;
    println!(
        "blueprint: {} ({} iterations); {} duplicate pairs per match on {} threads",
        checkpoint.display(),
        m.iterations(),
        pairs,
        rayon::current_num_threads()
    );
    for opp in vs {
        let start = Instant::now();
        let other = engine::Mccfr::new(&bp, engine::Weighting::Uniform);
        let me = || {
            Box::new(BlueprintAgent {
                bp: &bp,
                m: &m,
                label: "blueprint".into(),
            }) as Box<dyn Agent>
        };
        let r = match opp.as_str() {
            "call" => duplicate_match(
                &bp,
                me,
                || Box::new(CallingStation) as Box<dyn Agent>,
                pairs,
                1,
            ),
            "raise" => duplicate_match(&bp, me, || Box::new(Raiser) as Box<dyn Agent>, pairs, 2),
            "random" => duplicate_match(
                &bp,
                me,
                || Box::new(RandomAgent) as Box<dyn Agent>,
                pairs,
                3,
            ),
            "equity" => duplicate_match(
                &bp,
                me,
                || Box::new(EquityAgent { samples: 60 }) as Box<dyn Agent>,
                pairs / 5,
                4,
            ),
            path => {
                load_checkpoint(&other, std::path::Path::new(path))?;
                let label = format!("{path} ({} it)", other.iterations());
                duplicate_match(
                    &bp,
                    me,
                    || {
                        Box::new(BlueprintAgent {
                            bp: &bp,
                            m: &other,
                            label: label.clone(),
                        }) as Box<dyn Agent>
                    },
                    pairs,
                    5,
                )
            }
        };
        println!("{r}  ({:.1}s)", start.elapsed().as_secs_f64());
    }
    Ok(())
}

fn export_web_cmd(
    buckets: &std::path::Path,
    checkpoint: &std::path::Path,
    samples: usize,
    out: &std::path::Path,
    fixture: &std::path::Path,
    fixture_deals: usize,
) -> std::io::Result<()> {
    use engine::SampledGame;
    use hunl::blueprint::load_checkpoint;
    use hunl::buckets::bucket_of;
    let start = Instant::now();
    let b = hunl::Buckets::load(buckets)?;
    println!("rebuilding strength tables to recover the bucket thresholds");
    let t = hunl::StrengthTables::build(&|s| println!("  {s}"));
    let counts = [b.counts[1], b.counts[2], b.counts[3]];
    let th = hunl::Buckets::thresholds(&t, counts, samples);
    // The thresholds must reproduce the bucket file exactly, class by class.
    for street in 1..4 {
        let n = b.indexer.size(street) as usize;
        let bad = (0..n)
            .filter(|&c| {
                bucket_of(&th[street - 1], hunl::Buckets::metric(&t, street, c))
                    != b.tables[street][c]
            })
            .count();
        if bad > 0 {
            return Err(std::io::Error::other(format!(
                "street {street}: {bad} of {n} classes disagree with {} (wrong --samples?)",
                buckets.display()
            )));
        }
        println!("  street {street}: thresholds reproduce all {n} classes");
    }
    // Fixture: random deals with the metric and bucket per street.
    let mut rng = engine::SplitMix(0xC0FFEE);
    let mut fx = String::from("[");
    for i in 0..fixture_deals {
        let mut deck: Vec<u8> = (0..52).collect();
        for k in 0..7 {
            let j = k + rng.below(52 - k);
            deck.swap(k, j);
        }
        let cards: Vec<eval::Card> = deck[..7]
            .iter()
            .map(|&c| eval::Card::from_index(c))
            .collect();
        let hole = [cards[0], cards[1]];
        let mut bks = Vec::new();
        let mut vals = Vec::new();
        for (street, len) in [0usize, 3, 4, 5].into_iter().enumerate() {
            let board = &cards[2..2 + len];
            bks.push(b.bucket(hole, board));
            if street > 0 {
                let class = t.indexer.index(hole, board) as usize;
                vals.push(format!("{:?}", hunl::Buckets::metric(&t, street, class)));
            }
        }
        let names: Vec<String> = cards.iter().map(|c| c.to_string()).collect();
        if i > 0 {
            fx.push(',');
        }
        fx.push_str(&format!(
            "{{\"cards\":\"{}\",\"buckets\":{:?},\"values\":[{}]}}",
            names.join(" "),
            bks,
            vals.join(",")
        ));
    }
    fx.push(']');
    drop(t);
    let bp = hunl::Blueprint::new(
        hunl::Params::DEFAULT,
        hunl::ActionAbstraction::default_100bb(),
        b,
    );
    let m = engine::Mccfr::new(&bp, engine::Weighting::Uniform);
    load_checkpoint(&m, checkpoint)?;
    let json = hunl::web::export_json(&bp, &m, &th);
    for path in [out, fixture] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(out, &json)?;
    std::fs::write(fixture, &fx)?;
    println!(
        "{} ({} iterations, {} nodes): {:.1} MB -> {}\nfixture: {} deals -> {} ({:.0}s)",
        checkpoint.display(),
        m.iterations(),
        bp.nodes().len(),
        json.len() as f64 / 1e6,
        out.display(),
        fixture_deals,
        fixture.display(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn lbr_cmd(
    buckets: &std::path::Path,
    checkpoints: &[PathBuf],
    pairs: usize,
    flop_samples: usize,
    seed: u64,
) -> std::io::Result<()> {
    use hunl::arena::{Agent, BlueprintAgent, duplicate_match};
    use hunl::blueprint::load_checkpoint;
    use hunl::lbr::LbrAgent;
    let b = hunl::Buckets::load(buckets)?;
    let bp = hunl::Blueprint::new(
        hunl::Params::DEFAULT,
        hunl::ActionAbstraction::default_100bb(),
        b,
    );
    println!(
        "local best response, {pairs} duplicate pairs per checkpoint, {flop_samples} flop runouts, {} threads",
        rayon::current_num_threads()
    );
    for path in checkpoints {
        let m = engine::Mccfr::new(&bp, engine::Weighting::Uniform);
        load_checkpoint(&m, path)?;
        let t = Instant::now();
        let r = duplicate_match(
            &bp,
            || Box::new(LbrAgent::new(&bp, &m, flop_samples)) as Box<dyn Agent>,
            || {
                Box::new(BlueprintAgent {
                    bp: &bp,
                    m: &m,
                    label: format!("{} ({} it)", path.display(), m.iterations()),
                }) as Box<dyn Agent>
            },
            pairs,
            seed,
        );
        println!("{r}  ({:.0}s)", t.elapsed().as_secs_f64());
    }
    Ok(())
}
