//! Simulation suite, smoke grid (design §11): synthetic scenarios with a known
//! effect, the full comparison of milestone 4c repeated R times, checked against the
//! smoke-band acceptance criteria. Release build only: `cargo xtask sim`.
//!
//! Smoke band at R = 200 (SE of a 95 % coverage ≈ 1.5 pp): coverage ≥ 89 %, false
//! `better`/`worse` and false `equivalent` ≤ 9.5 %. The weekly full grid (R = 2000,
//! about 40 scenarios) follows.
#![forbid(clippy::too_many_arguments)]

mod synth;

use netmeasure_core::stats::compare::{Plan, compare};
use netmeasure_core::stats::side::{RunInput, Side};
use netmeasure_core::stats::verdict::{Margin, Verdict};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use synth::grid::Scenario;

const R: usize = 200;
const B: usize = 400;

#[derive(Debug, Default)]
struct Tally {
    covered: usize,
    worse: usize,
    better: usize,
    equivalent: usize,
    equivalent_open: usize,
}

fn tally(s: &Scenario, seed: u64) -> Tally {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut t = Tally::default();
    for rep in 0..R {
        let before = synth::side(&s.model, 1.0, &mut rng);
        let after = synth::side(&s.model, s.effect, &mut rng);
        let plan = Plan {
            quantiles: vec![0.99],
            primary: 0.99,
            margin: Some(Margin::Relative(0.10)),
            k: 1,
            replicates: B,
            seed: seed ^ rep as u64,
            block_override: s.block,
            threads: 1,
        };
        let q = &compare(&side(&before), &side(&after), &plan).quantiles[0];
        if q.interval
            .is_some_and(|(lo, hi)| lo <= s.effect && s.effect <= hi)
        {
            t.covered += 1;
        }
        match q.verdict {
            Some(Verdict::Worse) => t.worse += 1,
            Some(Verdict::Better) => t.better += 1,
            Some(Verdict::Equivalent { .. }) => {
                t.equivalent += 1;
                if q.tost_interval
                    .is_some_and(|(lo, hi)| lo.is_infinite() || hi.is_infinite())
                {
                    t.equivalent_open += 1;
                }
            }
            _ => {}
        }
    }
    t
}

fn side(runs: &[netmeasure_core::stats::series::Series]) -> Side<'_> {
    Side {
        nodes: vec![
            runs.iter()
                .map(|series| RunInput {
                    series,
                    floor_samples: 1.0,
                })
                .collect(),
        ],
    }
}

fn rate(n: usize) -> f64 {
    n as f64 / R as f64
}

#[test]
#[ignore = "release build: cargo xtask sim"]
fn smoke_grid() {
    let scenarios = synth::grid::smoke();
    let tallies: Vec<Tally> = std::thread::scope(|s| {
        let handles: Vec<_> = scenarios
            .iter()
            .enumerate()
            .map(|(i, sc)| s.spawn(move || tally(sc, 1000 + i as u64)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut failures = Vec::new();
    println!(
        "{:<34} {:>8} {:>6} {:>6} {:>6}",
        "scenario", "coverage", "worse", "better", "equiv"
    );
    for (sc, t) in scenarios.iter().zip(&tallies) {
        let (cov, worse, better, eq) = (
            rate(t.covered),
            rate(t.worse),
            rate(t.better),
            rate(t.equivalent),
        );
        println!(
            "{:<34} {cov:>8.3} {worse:>6.3} {better:>6.3} {eq:>6.3}",
            sc.name
        );
        let censored = sc.model.timeouts > 0.0;
        let mut check = |ok: bool, what: &str| {
            if !ok {
                failures.push(format!("{}: {what}", sc.name));
            }
        };
        if sc.block.is_some() {
            check(
                cov < 0.89,
                "negative control: the naive bootstrap must under-cover",
            );
            continue;
        }
        if !censored {
            check(cov >= 0.89, "coverage below 89 %");
        }
        if (sc.effect - 1.0).abs() < 1e-9 {
            check(worse + better <= 0.095, "false better/worse above 9.5 %");
        }
        if sc.effect > 1.1 && sc.effect < 1.2 {
            check(eq <= 0.095, "false equivalent above 9.5 %");
        }
        if sc.effect >= 1.3 {
            check(worse >= 0.9, "a clear regression found in fewer than 90 %");
        }
        check(
            t.equivalent_open == 0,
            "equivalent with an infinite TOST bound",
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
