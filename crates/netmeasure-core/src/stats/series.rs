//! Sample series for the statistics (design §10.1, §10.2): per target of one run, the
//! counted samples of every **valid** cycle, in measurement order, one segment per
//! cycle. Warm-up samples and invalid cycles are left out; a block never crosses a
//! segment boundary (§10.2).

use crate::measure::outcome::Outcome;
use crate::measure::record::{RESULT_CODES, flag};
use crate::schema::result::{ResultFile, SampleColumns};

/// A timeout in the latency distribution: larger than every measured value, meaning
/// "at least the timeout" (§10.1).
pub const CENSORED: u64 = u64::MAX;

/// The counted samples of one target in one valid cycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Segment {
    pub cycle: u64,
    /// Latencies in measurement order: `ok` as ns, `timeout` as [`CENSORED`].
    pub values: Vec<u64>,
    /// Connects counted for the failure rate (every outcome except local errors).
    pub attempts: u64,
    /// `timeout` + `refused` + `net_error` (§10.4).
    pub failures: u64,
    /// Connects with at least one SYN retransmission, and connects where the kernel
    /// did not report it.
    pub syn_retransmitted: u64,
    pub syn_unknown: u64,
}

/// One target of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Series {
    pub run_id: String,
    /// 0 = loopback control.
    pub target: u16,
    pub name: String,
    pub timeout_ns: u64,
    pub segments: Vec<Segment>,
}

impl Series {
    /// All latencies of the valid segments, sorted (censored last).
    pub fn sorted_values(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .segments
            .iter()
            .flat_map(|s| s.values.clone())
            .collect();
        v.sort_unstable();
        v
    }

    pub fn excluded_cycles(&self, cycles: u64) -> u64 {
        cycles.saturating_sub(self.segments.len() as u64)
    }
}

/// Every target of `file` (index 0 = loopback control first). `Err` if the sample
/// columns do not match the file's own description.
pub fn series(file: &ResultFile) -> Result<Vec<Series>, String> {
    let targets: Vec<(u16, String)> = file
        .targets
        .iter()
        .map(|t| (t.index, t.name.clone()))
        .collect();
    let cycles: Vec<CycleRange> = file
        .measurement
        .cycles
        .iter()
        .map(|c| CycleRange {
            index: c.index,
            first: c.first_sample,
            end: c.end_sample,
            valid: c.validity.valid,
        })
        .collect();
    from_parts(&Parts {
        run_id: &file.run.run_id,
        timeout_ns: file.run.config.timeout_ns,
        targets: &targets,
        cycles: &cycles,
        samples: &file.measurement.samples,
    })
}

/// A cycle's sample range `[first, end)` and its validity.
#[derive(Debug, Clone, Copy)]
pub struct CycleRange {
    pub index: u64,
    pub first: u64,
    pub end: u64,
    pub valid: bool,
}

/// The parts of a result file the series need.
pub struct Parts<'a> {
    pub run_id: &'a str,
    pub timeout_ns: u64,
    pub targets: &'a [(u16, String)],
    pub cycles: &'a [CycleRange],
    pub samples: &'a SampleColumns,
}

pub fn from_parts(p: &Parts) -> Result<Vec<Series>, String> {
    let s = p.samples;
    let n = s.count as usize;
    let columns = [
        s.target.len(),
        s.duration_ns.len(),
        s.result.len(),
        s.flags.len(),
    ];
    if columns.iter().any(|&len| len != n) || s.syn_retrans.len() != n {
        return Err("sample columns differ in length from `count`".into());
    }
    if s.result_codes != RESULT_CODES {
        return Err(format!("unknown result codes {:?}", s.result_codes));
    }
    let mut out: Vec<Series> = p
        .targets
        .iter()
        .map(|(index, name)| Series {
            run_id: p.run_id.to_owned(),
            target: *index,
            name: name.clone(),
            timeout_ns: p.timeout_ns,
            segments: Vec::new(),
        })
        .collect();
    for cycle in p.cycles.iter().filter(|c| c.valid) {
        let (first, end) = (cycle.first as usize, cycle.end as usize);
        if first > end || end > n {
            return Err(format!("cycle {} has an invalid sample range", cycle.index));
        }
        let mut segs: Vec<Segment> = out
            .iter()
            .map(|_| Segment {
                cycle: cycle.index,
                ..Segment::default()
            })
            .collect();
        for i in first..end {
            if s.flags[i] & flag::WARMUP != 0 {
                continue;
            }
            let seg = segs
                .get_mut(usize::from(s.target[i]))
                .ok_or_else(|| format!("sample {i} names an unknown target"))?;
            add(
                seg,
                outcome(s.result[i])?,
                s.duration_ns[i],
                s.syn_retrans[i],
            );
        }
        for (series, seg) in out.iter_mut().zip(segs) {
            if seg.attempts > 0 {
                series.segments.push(seg);
            }
        }
    }
    Ok(out)
}

fn outcome(code: u8) -> Result<Outcome, String> {
    Ok(match code {
        0 => Outcome::Ok,
        1 => Outcome::Timeout,
        2 => Outcome::Refused,
        3 => Outcome::NetError,
        4 => Outcome::LocalError,
        5 => Outcome::Anomaly,
        other => return Err(format!("unknown result code {other}")),
    })
}

fn add(seg: &mut Segment, o: Outcome, duration_ns: u64, syn_retrans: i64) {
    if o.invalidates_cycle() {
        return; // never in a valid cycle; kept out of every family regardless
    }
    seg.attempts += 1;
    seg.failures += u64::from(o.is_failure());
    match o {
        Outcome::Ok => seg.values.push(duration_ns),
        Outcome::Timeout => seg.values.push(CENSORED),
        _ => {}
    }
    match syn_retrans {
        r if r < 0 => seg.syn_unknown += 1,
        r if r > 0 => seg.syn_retransmitted += 1,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::record::{Sample, columns};

    fn sample(target: u16, ns: u64, outcome: Outcome, flags: u8, retrans: Option<u32>) -> Sample {
        Sample {
            target,
            late_ns: 0,
            duration_ns: ns,
            outcome,
            syn_retrans: retrans,
            flags,
            errno: None,
            reanchored_at_ns: None,
        }
    }

    #[test]
    fn valid_cycles_without_warmup_one_segment_each() {
        let s = columns(&[
            sample(1, 900, Outcome::Ok, flag::WARMUP, Some(0)),
            sample(0, 50, Outcome::Ok, 0, Some(0)),
            sample(1, 1000, Outcome::Ok, 0, Some(0)),
            sample(1, 5_000_000_000, Outcome::Timeout, 0, Some(2)),
            // cycle 1, invalid: left out entirely
            sample(1, 7, Outcome::Ok, 0, Some(0)),
            // cycle 2
            sample(1, 2000, Outcome::Refused, 0, None),
            sample(1, 3000, Outcome::Ok, 0, Some(1)),
        ]);
        let cycles = [
            CycleRange {
                index: 0,
                first: 0,
                end: 4,
                valid: true,
            },
            CycleRange {
                index: 1,
                first: 4,
                end: 5,
                valid: false,
            },
            CycleRange {
                index: 2,
                first: 5,
                end: 7,
                valid: true,
            },
        ];
        let targets = [(0, "loopback".to_owned()), (1, "node-b".to_owned())];
        let out = from_parts(&Parts {
            run_id: "R",
            timeout_ns: 5_000_000_000,
            targets: &targets,
            cycles: &cycles,
            samples: &s,
        })
        .unwrap();
        assert_eq!(out[0].segments.len(), 1);
        let b = &out[1];
        assert_eq!((b.name.as_str(), b.segments.len()), ("node-b", 2));
        assert_eq!(b.segments[0].values, vec![1000, CENSORED]);
        assert_eq!((b.segments[0].attempts, b.segments[0].failures), (2, 1));
        assert_eq!(b.segments[1].cycle, 2);
        assert_eq!(b.segments[1].values, vec![3000]);
        assert_eq!((b.segments[1].failures, b.segments[1].syn_unknown), (1, 1));
        assert_eq!(b.segments[1].syn_retransmitted, 1);
        assert_eq!(b.sorted_values(), vec![1000, 3000, CENSORED]);
        assert_eq!(b.excluded_cycles(3), 1);
    }

    #[test]
    fn inconsistent_files_are_errors() {
        let mut s = columns(&[sample(1, 1, Outcome::Ok, 0, Some(0))]);
        let targets = [(0, "lo".to_owned())];
        let cycles = [CycleRange {
            index: 0,
            first: 0,
            end: 1,
            valid: true,
        }];
        let run = |s: &SampleColumns| {
            from_parts(&Parts {
                run_id: "R",
                timeout_ns: 1,
                targets: &targets,
                cycles: &cycles,
                samples: s,
            })
            .unwrap_err()
        };
        assert!(run(&s).contains("unknown target"));
        s.count = 2;
        assert!(run(&s).contains("differ in length"));
    }
}
