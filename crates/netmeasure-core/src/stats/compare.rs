//! Comparison of two sides (design §10.3, §10.4): hierarchical bootstrap replicates
//! of both sides (independent RNG streams from one recorded seed), the primary
//! quantile with a verdict, the other quantiles descriptive, failure rates with
//! their own verdict family, SYN retransmissions as diagnostics.

use super::blocklen::BlockChoice;
use super::describe::{Estimate, describe};
use super::parallel::{Sides, replicates};
use super::rate::{self, RateComparison};
use super::segment::{CENSORED, mark};
use super::side::{Replicate, Side};
use super::verdict::{Levels, Margin, Verdict, decide, effect, interval};

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Quantiles to report; must contain `primary`.
    pub quantiles: Vec<f64>,
    /// The only latency quantile with a verdict, declared before the measurement.
    pub primary: f64,
    pub margin: Option<Margin>,
    /// Independent verdicts of this family (Bonferroni).
    pub k: u32,
    pub replicates: usize,
    pub seed: u64,
    pub block_override: Option<f64>,
    /// Threads for the replicates; the result does not depend on it.
    pub threads: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuantileComparison {
    pub p: f64,
    pub reference: Option<Estimate>,
    pub after: Option<Estimate>,
    /// `after − reference` (ns), or `after / reference` with a relative margin.
    pub effect: Option<f64>,
    /// Relative change `after / reference − 1`, when both are measured.
    pub relative: Option<f64>,
    /// At the Bonferroni level for the primary quantile, 95 % for the others.
    pub interval: Option<(f64, f64)>,
    pub tost_interval: Option<(f64, f64)>,
    /// `None` for descriptive quantiles.
    pub verdict: Option<Verdict>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SideSummary {
    pub runs: usize,
    pub nodes: usize,
    pub n: usize,
    /// Applied mean block length and the per-run choices (transparency, §10.2).
    pub block_length: f64,
    pub blocks: Vec<BlockChoice>,
    /// Per run: `run_id` and its primary quantile (between-run spread).
    pub per_run_primary: Vec<(String, Option<u64>)>,
    pub timeouts: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub seed: u64,
    pub levels: Levels,
    pub reference: SideSummary,
    pub after: SideSummary,
    pub quantiles: Vec<QuantileComparison>,
    pub failures: RateComparison,
    pub syn_retransmissions: RateComparison,
}

pub fn compare(reference: &Side, after: &Side, plan: &Plan) -> Comparison {
    let levels = Levels::bonferroni(plan.k);
    let ratio = matches!(plan.margin, Some(Margin::Relative(_)));
    let (len_r, blocks_r) = reference.block_lengths(plan.primary, plan.block_override);
    let (len_a, blocks_a) = after.block_lengths(plan.primary, plan.block_override);
    let sides = Sides {
        reference,
        after,
        block_reference: len_r,
        block_after: len_a,
    };
    let reps: Vec<(Replicate, Replicate)> = replicates(
        &sides,
        &plan.quantiles,
        plan.replicates,
        plan.seed,
        plan.threads,
    );
    let (pooled_r, pooled_a) = (reference.pooled_sorted(), after.pooled_sorted());
    let timeout = timeout_of(reference).max(timeout_of(after));
    let quantiles = plan
        .quantiles
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let primary = (p - plan.primary).abs() < 1e-12;
            let est = |v: &[u64]| describe(v, p, timeout, 0.95).estimate;
            let point = |v: &[u64]| super::quantile::type1(v, p).unwrap_or(CENSORED);
            let effects: Vec<f64> = reps
                .iter()
                .map(|(r, a)| effect(r.quantiles[i], a.quantiles[i], ratio))
                .collect();
            let level = if primary { levels.two_sided } else { 0.95 };
            let iv = interval(&effects, level);
            let tost = primary.then(|| interval(&effects, levels.tost)).flatten();
            let (qr, qa) = (point(&pooled_r), point(&pooled_a));
            let e = effect(qr, qa, ratio);
            QuantileComparison {
                p,
                reference: est(&pooled_r),
                after: est(&pooled_a),
                effect: e.is_finite().then_some(e),
                relative: (qr != CENSORED && qa != CENSORED && qr > 0)
                    .then(|| qa as f64 / qr as f64 - 1.0),
                interval: iv,
                tost_interval: tost,
                verdict: primary.then(|| decide(iv, tost, plan.margin, ratio)),
            }
        })
        .collect();
    let rates = |f: fn(&Replicate) -> (u64, u64)| -> Vec<_> {
        reps.iter().map(|(r, a)| (f(r), f(a))).collect()
    };
    Comparison {
        seed: plan.seed,
        levels,
        reference: summary(reference, &pooled_r, plan.primary, len_r, blocks_r),
        after: summary(after, &pooled_a, plan.primary, len_a, blocks_a),
        quantiles,
        failures: rate::compare(
            reference.observed_rate(mark::FAILED, 0),
            after.observed_rate(mark::FAILED, 0),
            &rates(|r| r.failures),
            levels,
            true,
        ),
        syn_retransmissions: rate::compare(
            reference.observed_rate(mark::SYN_RETRANSMITTED, mark::SYN_UNKNOWN),
            after.observed_rate(mark::SYN_RETRANSMITTED, mark::SYN_UNKNOWN),
            &rates(|r| r.syn),
            levels,
            false,
        ),
    }
}

fn timeout_of(side: &Side) -> u64 {
    side.runs().map(|r| r.series.timeout_ns).max().unwrap_or(0)
}

fn summary(
    side: &Side,
    pooled: &[u64],
    primary: f64,
    len: f64,
    blocks: Vec<BlockChoice>,
) -> SideSummary {
    SideSummary {
        runs: side.runs().count(),
        nodes: side.nodes.len(),
        n: pooled.len(),
        block_length: len,
        blocks,
        per_run_primary: side.per_run(primary),
        timeouts: pooled.iter().filter(|&&v| v == CENSORED).count() as u64,
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::*;
    use crate::stats::series::Series;
    use crate::stats::side::RunInput;
    use rand::RngExt;

    /// Log-normal-ish latencies around `median_us`, i.i.d., one segment.
    fn run(id: &str, median_us: f64, seed: u64) -> Series {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let values = (0..5000)
            .map(|_| {
                let z: f64 = (0..12).map(|_| rng.random::<f64>()).sum::<f64>() - 6.0;
                (median_us * 1000.0 * (0.5 * z).exp()) as u64
            })
            .collect();
        crate::stats::side::tests::series(id, values, 0)
    }

    fn side(runs: &[Series]) -> Side<'_> {
        Side {
            nodes: vec![
                runs.iter()
                    .map(|s| RunInput {
                        series: s,
                        floor_samples: 20.0,
                    })
                    .collect(),
            ],
        }
    }

    fn plan(margin: Option<Margin>) -> Plan {
        Plan {
            quantiles: vec![0.5, 0.99],
            primary: 0.99,
            margin,
            k: 1,
            replicates: 400,
            seed: 7,
            block_override: None,
            threads: 2,
        }
    }

    #[test]
    fn same_distribution_is_equivalent_or_inconclusive() {
        let a: Vec<Series> = (0..3).map(|i| run("a", 1500.0, i)).collect();
        let b: Vec<Series> = (10..13).map(|i| run("b", 1500.0, i)).collect();
        let c = compare(&side(&a), &side(&b), &plan(Some(Margin::Relative(0.15))));
        let primary = c.quantiles.iter().find(|q| q.verdict.is_some()).unwrap();
        assert!(
            matches!(
                primary.verdict,
                Some(Verdict::Equivalent { .. } | Verdict::Inconclusive { .. })
            ),
            "{primary:?}"
        );
        assert_eq!(c.quantiles[0].verdict, None, "p50 is descriptive");
        assert_eq!((c.reference.runs, c.after.per_run_primary.len()), (3, 3));
        assert_eq!(c.failures.verdict, None, "no failures: descriptive");
    }

    #[test]
    fn a_clear_regression_is_worse() {
        let a: Vec<Series> = (0..3).map(|i| run("a", 1500.0, i)).collect();
        let b: Vec<Series> = (10..13).map(|i| run("b", 2500.0, i)).collect();
        let c = compare(&side(&a), &side(&b), &plan(Some(Margin::Relative(0.1))));
        assert_eq!(c.quantiles[1].verdict, Some(Verdict::Worse));
        assert!(c.quantiles[1].relative.unwrap() > 0.4);
        let again = compare(&side(&a), &side(&b), &plan(Some(Margin::Relative(0.1))));
        assert_eq!(c, again, "same seed, same result");
    }
}
