//! One side of a comparison (design §10.3): its runs, grouped by node, and the
//! hierarchical bootstrap over them. Sequential design: one node, runs drawn with
//! replacement, then stationary blocks within each drawn run. Concurrent design:
//! nodes drawn first, then runs within each drawn node, then blocks.

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::blocklen::{self, BlockChoice};
use super::bootstrap::{append, select_type1};
use super::quantile::type1;
use super::segment::mark;
use super::series::Series;

/// One target series of one run, with the floor of its block length in samples.
#[derive(Debug, Clone)]
pub struct RunInput<'a> {
    pub series: &'a Series,
    pub floor_samples: f64,
}

/// A side: `nodes[i]` holds the runs of node `i` (one node in the sequential design).
#[derive(Debug, Clone)]
pub struct Side<'a> {
    pub nodes: Vec<Vec<RunInput<'a>>>,
}

/// What one replicate of a side yields.
#[derive(Debug, Clone, PartialEq)]
pub struct Replicate {
    /// Type-1 quantiles in the order of the requested `p`.
    pub quantiles: Vec<u64>,
    /// `(count, attempts)` of failures and of SYN retransmissions (known attempts).
    pub failures: (u64, u64),
    pub syn: (u64, u64),
}

impl Side<'_> {
    pub fn runs(&self) -> impl Iterator<Item = &RunInput<'_>> {
        self.nodes.iter().flatten()
    }

    /// All latencies of the side, sorted (censored last).
    pub fn pooled_sorted(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.runs().flat_map(|r| r.series.sorted_values()).collect();
        v.sort_unstable();
        v
    }

    /// Block length per run for the quantile `p` of the pooled side; the side uses
    /// the largest applied value (conservative, §10.2).
    pub fn block_lengths(&self, p: f64, override_len: Option<f64>) -> (f64, Vec<BlockChoice>) {
        let q = type1(&self.pooled_sorted(), p).unwrap_or(u64::MAX);
        let choices: Vec<BlockChoice> = self
            .runs()
            .map(|r| {
                let segs: Vec<&[u64]> = r
                    .series
                    .segments
                    .iter()
                    .map(|s| s.values.as_slice())
                    .collect();
                blocklen::choose(
                    &blocklen::indicator(&segs, q),
                    r.floor_samples,
                    override_len,
                )
            })
            .collect();
        let applied = choices.iter().map(|c| c.applied).fold(1.0, f64::max);
        (applied, choices)
    }

    /// The type-1 quantile of every run on its own: the between-run spread (§10.3).
    pub fn per_run(&self, p: f64) -> Vec<(String, Option<u64>)> {
        self.runs()
            .map(|r| (r.series.run_id.clone(), type1(&r.series.sorted_values(), p)))
            .collect()
    }

    /// One hierarchical replicate.
    pub fn replicate(&self, ps: &[f64], mean_block: f64, rng: &mut ChaCha8Rng) -> Replicate {
        let (mut values, mut marks) = (Vec::new(), Vec::new());
        for node in draw(&self.nodes, rng) {
            for run in draw(node, rng) {
                let segs = &run.series.segments;
                let v: Vec<&[u64]> = segs.iter().map(|s| s.values.as_slice()).collect();
                let m: Vec<&[u8]> = segs.iter().map(|s| s.marks.as_slice()).collect();
                append(&v, mean_block, rng, &mut values);
                append(&m, mean_block, rng, &mut marks);
            }
        }
        Replicate {
            quantiles: ps
                .iter()
                .map(|&p| select_type1(&mut values, p).unwrap_or(u64::MAX))
                .collect(),
            failures: rate(&marks, mark::FAILED, 0),
            syn: rate(&marks, mark::SYN_RETRANSMITTED, mark::SYN_UNKNOWN),
        }
    }

    /// `(count, attempts)` over the original data, for the point estimate.
    pub fn observed_rate(&self, flag: u8, exclude: u8) -> (u64, u64) {
        let marks: Vec<u8> = self
            .runs()
            .flat_map(|r| {
                r.series
                    .segments
                    .iter()
                    .flat_map(|s| s.marks.iter().copied())
            })
            .collect();
        rate(&marks, flag, exclude)
    }
}

/// Draws `items.len()` items with replacement; a single item is used as it is.
fn draw<'a, T>(items: &'a [T], rng: &mut ChaCha8Rng) -> Vec<&'a T> {
    match items.len() {
        0 => Vec::new(),
        1 => vec![&items[0]],
        n => (0..n).map(|_| &items[rng.random_range(0..n)]).collect(),
    }
}

/// Marks with `flag` among the marks without `exclude`.
fn rate(marks: &[u8], flag: u8, exclude: u8) -> (u64, u64) {
    marks
        .iter()
        .filter(|m| *m & exclude == 0)
        .fold((0, 0), |(c, n), m| (c + u64::from(m & flag != 0), n + 1))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::stats::segment::Segment;
    use rand::SeedableRng;

    pub(crate) fn series(run: &str, values: Vec<u64>, failed_every: usize) -> Series {
        let marks = (0..values.len())
            .map(|i| {
                if failed_every > 0 && i % failed_every == 0 {
                    mark::FAILED
                } else {
                    0
                }
            })
            .collect();
        Series {
            run_id: run.into(),
            target: 1,
            name: "b".into(),
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

    #[test]
    fn single_run_side_and_spread() {
        let s = series("r1", (1..=1000).collect(), 100);
        let side = Side {
            nodes: vec![vec![RunInput {
                series: &s,
                floor_samples: 5.0,
            }]],
        };
        assert_eq!(side.per_run(0.5), vec![("r1".to_owned(), Some(500))]);
        assert_eq!(side.observed_rate(mark::FAILED, 0), (10, 1000));
        let (len, choices) = side.block_lengths(0.9, None);
        assert_eq!(choices.len(), 1);
        assert!(len >= 5.0);
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let r = side.replicate(&[0.5, 0.99], len, &mut rng);
        assert_eq!(r.failures.1, 1000);
        assert!(r.quantiles[0] < r.quantiles[1]);
    }

    #[test]
    fn runs_are_drawn_with_replacement() {
        let a = series("a", vec![10; 100], 0);
        let b = series("b", vec![1000; 100], 0);
        let run = |s| RunInput {
            series: s,
            floor_samples: 1.0,
        };
        let side = Side {
            nodes: vec![vec![run(&a), run(&b)]],
        };
        let mut rng = ChaCha8Rng::seed_from_u64(5);
        let medians: Vec<u64> = (0..200)
            .map(|_| side.replicate(&[0.5], 1.0, &mut rng).quantiles[0])
            .collect();
        // Both runs drawn twice or once each: the median is 10 or 1000, both occur.
        assert!(medians.contains(&10) && medians.contains(&1000));
    }
}
