//! Semi-synthetic check (design §11): real runs from the same nodes and time window,
//! split at random into two pseudo sides per source node; a **known** factor is
//! imposed on the second side and the full comparison of `report` must recover it.
//! This tests the method against the real dependence structure of a network, not
//! only against the simulated models.
//!
//! The data is never part of the repository: `cargo xtask semisynthetic <dir>` passes
//! the directory in `NETMEASURE_SEMISYNTHETIC_DIR`. Needs ≥ 2 runs per node and path
//! (e.g. a mesh series of 5 rounds).
#![forbid(clippy::too_many_arguments)]

mod realdata;

use std::path::Path;

use netmeasure_core::stats::compare::{Plan, compare};
use netmeasure_core::stats::side::{RunInput, Side};
use netmeasure_core::stats::verdict::{Margin, Verdict};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;

use realdata::{Run, load, scaled};

/// Imposed factors: none, at the 10 % margin, a clear regression.
const EFFECTS: [f64; 3] = [1.0, 1.1, 1.3];
const REPLICATES: usize = 1000;
/// Coverage below this fails. Looser than the simulation band: the splits of one
/// data set are not independent repetitions.
const MIN_COVERAGE: f64 = 0.90;

#[derive(Default)]
struct Tally {
    covered: usize,
    worse: usize,
    better: usize,
    equivalent: usize,
    width: Vec<f64>,
}

/// One random split per node, the second side scaled by `effect`.
fn split(nodes: &[&Vec<Run>], effect: f64, seed: u64) -> (Option<(f64, f64)>, Option<Verdict>) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for runs in nodes {
        let mut idx: Vec<usize> = (0..runs.len()).collect();
        idx.shuffle(&mut rng);
        let half = runs.len() / 2 + usize::from(seed % 2 == 1 && runs.len() % 2 == 1);
        a.push(idx[..half].iter().map(|&i| &runs[i]).collect::<Vec<_>>());
        b.push(
            idx[half..]
                .iter()
                .map(|&i| (scaled(&runs[i].targets, effect), runs[i].floor))
                .collect::<Vec<_>>(),
        );
    }
    let side_a = Side {
        nodes: a
            .iter()
            .map(|rs| {
                rs.iter()
                    .map(|r| RunInput {
                        series: &r.targets,
                        floor_samples: r.floor,
                    })
                    .collect()
            })
            .collect(),
    };
    let side_b = Side {
        nodes: b
            .iter()
            .map(|rs| {
                rs.iter()
                    .map(|(s, f)| RunInput {
                        series: s,
                        floor_samples: *f,
                    })
                    .collect()
            })
            .collect(),
    };
    let plan = Plan {
        quantiles: vec![0.99],
        primary: 0.99,
        margin: Some(Margin::Relative(0.10)),
        k: 1,
        replicates: REPLICATES,
        seed,
        block_override: None,
    };
    let q = &compare(&side_a, &side_b, &plan).quantiles[0];
    (q.interval, q.verdict)
}

/// Distinct ways to split `n` runs as `split` does (half, or half + 1 when odd).
fn splits_of(n: usize) -> u64 {
    let choose = |k: usize| (0..k).fold(1u64, |c, i| c * (n - i) as u64 / (i as u64 + 1));
    if n % 2 == 1 {
        2 * choose(n / 2)
    } else {
        choose(n / 2)
    }
}

#[test]
#[ignore = "needs real data: cargo xtask semisynthetic <dir>"]
fn semisynthetic() {
    let dir = std::env::var_os("NETMEASURE_SEMISYNTHETIC_DIR")
        .expect("set NETMEASURE_SEMISYNTHETIC_DIR (cargo xtask semisynthetic <dir>)");
    let splits: usize = std::env::var("NETMEASURE_SEMISYNTHETIC_SPLITS").map_or(100, |s| {
        s.parse()
            .expect("NETMEASURE_SEMISYNTHETIC_SPLITS: a number")
    });
    let data = load(Path::new(&dir));
    assert!(!data.is_empty(), "no result files below {dir:?}");
    let mut failures = Vec::new();
    println!("splits {splits}, replicates {REPLICATES}, p99, margin ±10 %, 95 % interval");
    println!(
        "{:<9} {:>6} {:>6} {:>9} {:>6} {:>6} {:>6} {:>12}",
        "path", "nodes", "effect", "coverage", "worse", "better", "equiv", "median width"
    );
    for (path, by_node) in &data {
        let nodes: Vec<&Vec<Run>> = by_node.values().filter(|r| r.len() >= 2).collect();
        if nodes.is_empty() {
            println!("{path:<9} skipped: no node with ≥ 2 runs");
            continue;
        }
        let distinct = nodes
            .iter()
            .fold(1u64, |acc, r| acc.saturating_mul(splits_of(r.len())));
        if distinct < splits as u64 {
            println!(
                "{path:<9} only {distinct} distinct splits: the {splits} splits repeat them, \
                 the coverage below is not informative (needs more nodes or runs)"
            );
        }
        for (e, &effect) in EFFECTS.iter().enumerate() {
            let results: Vec<_> = std::thread::scope(|s| {
                let hs: Vec<_> = (0..splits)
                    .map(|i| {
                        let nodes = &nodes;
                        s.spawn(move || split(nodes, effect, (e * 100_000 + i) as u64))
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let mut t = Tally::default();
            for (iv, verdict) in results {
                if let Some((lo, hi)) = iv {
                    t.covered += usize::from(lo <= effect && effect <= hi);
                    t.width.push(hi - lo);
                }
                match verdict {
                    Some(Verdict::Worse) => t.worse += 1,
                    Some(Verdict::Better) => t.better += 1,
                    Some(Verdict::Equivalent { .. }) => t.equivalent += 1,
                    _ => {}
                }
            }
            t.width.sort_by(f64::total_cmp);
            let r = |n: usize| n as f64 / splits as f64;
            let width = t.width.get(t.width.len() / 2).copied().unwrap_or(f64::NAN);
            println!(
                "{path:<9} {:>6} {effect:>6.2} {:>9.3} {:>6.3} {:>6.3} {:>6.3} {width:>12.3}",
                nodes.len(),
                r(t.covered),
                r(t.worse),
                r(t.better),
                r(t.equivalent)
            );
            if r(t.covered) < MIN_COVERAGE {
                failures.push(format!(
                    "{path} ×{effect}: coverage {:.3} < {MIN_COVERAGE}",
                    r(t.covered)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
