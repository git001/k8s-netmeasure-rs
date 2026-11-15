//! Synthetic runs with a known effect for the simulation suite (design §11).
//!
//! Latencies are log-normal around 1.5 ms with σ = 0.8 (p99/p50 ≈ 6.4, close to the
//! first cluster baselines), optionally AR(1) on the log scale, with a random
//! effect per run, bursts (regime switching) and timeouts. The effect multiplies
//! every latency, so every quantile of the after-side is exactly `effect` times the
//! reference quantile: the true ratio is known without a closed form.

pub mod grid;

use netmeasure_core::stats::segment::{CENSORED, Segment};
use netmeasure_core::stats::series::Series;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;

#[derive(Debug, Clone, Copy)]
pub struct Model {
    /// AR(1) coefficient of the log latency (0 = independent).
    pub phi: f64,
    /// Standard deviation of a per-run log offset (between-run variance).
    pub run_sd: f64,
    /// Bursts: about one per 2000 samples, ~200 samples long, +1.5 on the log scale.
    pub bursts: bool,
    /// Probability of a timeout per connect (censored, independent of the effect).
    pub timeouts: f64,
    pub runs: usize,
    pub samples: usize,
}

const MEDIAN_NS: f64 = 1.5e6;
const SIGMA: f64 = 0.8;

fn normal(rng: &mut ChaCha8Rng) -> f64 {
    // Box–Muller; one value per call is enough here.
    let (u, v): (f64, f64) = (rng.random(), rng.random());
    (-2.0 * (1.0 - u).ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// One side: `m.runs` runs with the multiplicative `effect`.
pub fn side(m: &Model, effect: f64, rng: &mut ChaCha8Rng) -> Vec<Series> {
    (0..m.runs).map(|r| run(m, effect, r, rng)).collect()
}

fn run(m: &Model, effect: f64, index: usize, rng: &mut ChaCha8Rng) -> Series {
    let offset = m.run_sd * normal(rng);
    let innovation = (1.0 - m.phi * m.phi).sqrt();
    let mut e = normal(rng);
    let mut burst = 0usize;
    let mut values = Vec::with_capacity(m.samples);
    for _ in 0..m.samples {
        e = m.phi * e + innovation * normal(rng);
        if m.bursts && burst == 0 && rng.random::<f64>() < 1.0 / 2000.0 {
            burst = 200;
        }
        let lift = if burst > 0 {
            burst -= 1;
            1.5
        } else {
            0.0
        };
        let v = if rng.random::<f64>() < m.timeouts {
            CENSORED
        } else {
            (effect * MEDIAN_NS * (offset + SIGMA * e + lift).exp()) as u64
        };
        values.push(v);
    }
    let marks = vec![0; values.len()];
    Series {
        run_id: format!("run-{index}"),
        target: 1,
        name: "sim".into(),
        timeout_ns: 5_000_000_000,
        segments: vec![Segment {
            cycles: vec![0],
            attempts: values.len() as u64,
            values,
            marks,
            ..Segment::default()
        }],
    }
}
