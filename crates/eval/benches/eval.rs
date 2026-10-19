//! Evaluator throughput.
//!
//! `random/*` measures one evaluation of an unpredictable hand (cache and branch
//! predictor cold-ish), which is what MCCFR rollouts see. `enumerate` measures the
//! incremental pattern used for exhaustive equity: shared prefixes are added once.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use eval::{Card, Hand};

const N: usize = 1 << 16;

fn random_hands(n_cards: usize) -> Vec<[Card; 7]> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    (0..N)
        .map(|_| {
            let mut deck: Vec<Card> = Card::deck().collect();
            for i in 0..n_cards {
                let j = i + (next() % (52 - i) as u64) as usize;
                deck.swap(i, j);
            }
            let mut h = [deck[0]; 7];
            h[..n_cards].copy_from_slice(&deck[..n_cards]);
            h
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    eval::warm_up();

    let mut g = c.benchmark_group("random");
    g.throughput(Throughput::Elements(N as u64));
    let sevens = random_hands(7);
    let built: Vec<Hand> = sevens.iter().map(|h| Hand::from_cards(h)).collect();
    g.bench_function("rank_prebuilt_7", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for h in &built {
                acc = acc.wrapping_add(black_box(h).rank().0 as u32);
            }
            acc
        })
    });
    g.bench_function("build_and_rank_7", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for h in &sevens {
                acc = acc.wrapping_add(Hand::from_cards(black_box(h)).rank().0 as u32);
            }
            acc
        })
    });
    let fives = random_hands(5);
    g.bench_function("build_and_rank_5", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for h in &fives {
                acc = acc.wrapping_add(Hand::from_cards(black_box(&h[..5])).rank().0 as u32);
            }
            acc
        })
    });
    g.finish();

    // Every 7-card hand containing the 2c: C(51, 6) = 18,009,460 hands.
    let cards: Vec<Card> = Card::deck().collect();
    let mut g = c.benchmark_group("enumerate");
    g.sample_size(10);
    g.throughput(Throughput::Elements(18_009_460));
    g.bench_function("all_7_with_2c", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            let h1 = Hand::EMPTY.plus(cards[0]);
            for a in 1..52 {
                let h2 = h1.plus(cards[a]);
                for bb in a + 1..52 {
                    let h3 = h2.plus(cards[bb]);
                    for c in bb + 1..52 {
                        let h4 = h3.plus(cards[c]);
                        for d in c + 1..52 {
                            let h5 = h4.plus(cards[d]);
                            for e in d + 1..52 {
                                let h6 = h5.plus(cards[e]);
                                for &card in &cards[e + 1..] {
                                    acc += h6.plus(card).rank().0 as u64;
                                }
                            }
                        }
                    }
                }
            }
            black_box(acc)
        })
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
