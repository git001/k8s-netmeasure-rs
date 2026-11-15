//! Bootstrap replicates on several threads (design §10.3). Replicate `i` draws from
//! its own ChaCha streams `2i` (reference) and `2i + 1` (after) of the recorded seed,
//! so the result is the same for every thread count and a report stays reproducible
//! from its seed.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::side::{Replicate, Side};

/// The two sides with their mean block lengths.
pub struct Sides<'s, 'a> {
    pub reference: &'s Side<'a>,
    pub after: &'s Side<'a>,
    pub block_reference: f64,
    pub block_after: f64,
}

fn stream(seed: u64, n: u64) -> ChaCha8Rng {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    rng.set_stream(n);
    rng
}

fn one(s: &Sides, ps: &[f64], seed: u64, i: usize) -> (Replicate, Replicate) {
    let i = i as u64;
    (
        s.reference
            .replicate(ps, s.block_reference, &mut stream(seed, 2 * i)),
        s.after
            .replicate(ps, s.block_after, &mut stream(seed, 2 * i + 1)),
    )
}

/// `n` replicates of both sides, in replicate order, on up to `threads` threads.
pub fn replicates(
    s: &Sides,
    ps: &[f64],
    n: usize,
    seed: u64,
    threads: usize,
) -> Vec<(Replicate, Replicate)> {
    let threads = threads.clamp(1, n.max(1));
    if threads == 1 {
        return (0..n).map(|i| one(s, ps, seed, i)).collect();
    }
    let chunk = n.div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let range = (t * chunk).min(n)..((t + 1) * chunk).min(n);
                scope.spawn(move || range.map(|i| one(s, ps, seed, i)).collect::<Vec<_>>())
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("bootstrap thread panicked"))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::segment::Segment;
    use crate::stats::series::Series;
    use crate::stats::side::RunInput;

    fn series(offset: u64) -> Series {
        let values: Vec<u64> = (0..500).map(|i| 1_000 + (i * 37 + offset) % 997).collect();
        Series {
            run_id: format!("r{offset}"),
            target: 1,
            name: "t".into(),
            timeout_ns: 5_000_000_000,
            segments: vec![Segment {
                cycles: vec![0],
                attempts: values.len() as u64,
                marks: vec![0; values.len()],
                values,
                ..Segment::default()
            }],
        }
    }

    #[test]
    fn the_thread_count_does_not_change_the_replicates() {
        let (a, b) = (series(1), series(2));
        let side = |s| Side {
            nodes: vec![vec![RunInput {
                series: s,
                floor_samples: 10.0,
            }]],
        };
        let (ra, rb) = (side(&a), side(&b));
        let s = Sides {
            reference: &ra,
            after: &rb,
            block_reference: 10.0,
            block_after: 10.0,
        };
        let one = replicates(&s, &[0.5, 0.99], 37, 9, 1);
        assert_eq!(one.len(), 37);
        for threads in [2, 5, 64] {
            assert_eq!(replicates(&s, &[0.5, 0.99], 37, 9, threads), one);
        }
        assert_ne!(replicates(&s, &[0.5, 0.99], 37, 10, 4), one);
        assert!(replicates(&s, &[0.5], 0, 9, 4).is_empty());
    }
}
