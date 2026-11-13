//! One stored sample (design §5) and its column encoding: result codes, flag bits,
//! `syn_retrans` with -1 for unknown, sparse `errno` and re-anchor columns.

use crate::measure::outcome::Outcome;
use crate::schema::result::{Reanchor, SampleColumns, SparseErrno};

/// Order of the `result` column codes; written to every file as `result_codes`.
pub const RESULT_CODES: [&str; 6] = [
    "ok",
    "timeout",
    "refused",
    "net_error",
    "local_error",
    "anomaly",
];

pub fn result_code(o: Outcome) -> u8 {
    match o {
        Outcome::Ok => 0,
        Outcome::Timeout => 1,
        Outcome::Refused => 2,
        Outcome::NetError => 3,
        Outcome::LocalError => 4,
        Outcome::Anomaly => 5,
    }
}

/// Bits of the `flags` column.
pub mod flag {
    pub const SYNC_CONNECT: u8 = 1;
    pub const WARMUP: u8 = 1 << 1;
    pub const AFTER_OVERRUN: u8 = 1 << 2;
    pub const EINTR: u8 = 1 << 3;
    pub const PEER_CLOSED: u8 = 1 << 4;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// 0 = loopback control, `i + 1` = real target `i`.
    pub target: u16,
    pub late_ns: u64,
    pub duration_ns: u64,
    pub outcome: Outcome,
    /// `tcpi_total_retrans`; `None` if unknown.
    pub syn_retrans: Option<u32>,
    pub flags: u8,
    pub errno: Option<i32>,
    /// Re-anchored slot (§3.4 rule 2): its scheduled start, ns after the run origin.
    pub reanchored_at_ns: Option<u64>,
}

/// The column form of `samples`; `index` in the sparse columns is the position in it.
pub fn columns(samples: &[Sample]) -> SampleColumns {
    let mut errno = SparseErrno::default();
    let mut reanchor = Reanchor::default();
    for (i, s) in samples.iter().enumerate() {
        if let Some(e) = s.errno {
            errno.index.push(i as u64);
            errno.value.push(e);
        }
        if let Some(at) = s.reanchored_at_ns {
            reanchor.index.push(i as u64);
            reanchor.sched_offset_ns.push(at);
        }
    }
    SampleColumns {
        count: samples.len() as u64,
        result_codes: RESULT_CODES.iter().map(|c| (*c).to_owned()).collect(),
        target: samples.iter().map(|s| s.target).collect(),
        late_ns: samples.iter().map(|s| s.late_ns).collect(),
        duration_ns: samples.iter().map(|s| s.duration_ns).collect(),
        result: samples.iter().map(|s| result_code(s.outcome)).collect(),
        syn_retrans: samples
            .iter()
            .map(|s| s.syn_retrans.map_or(-1, i64::from))
            .collect(),
        flags: samples.iter().map(|s| s.flags).collect(),
        reanchor,
        errno,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(outcome: Outcome, errno: Option<i32>, reanchored_at_ns: Option<u64>) -> Sample {
        Sample {
            target: 1,
            late_ns: 5,
            duration_ns: 100,
            outcome,
            syn_retrans: None,
            flags: flag::WARMUP,
            errno,
            reanchored_at_ns,
        }
    }

    #[test]
    fn codes_follow_the_documented_order() {
        for (i, o) in [
            Outcome::Ok,
            Outcome::Timeout,
            Outcome::Refused,
            Outcome::NetError,
            Outcome::LocalError,
            Outcome::Anomaly,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(usize::from(result_code(o)), i);
            assert_eq!(serde_json::to_value(o).unwrap(), RESULT_CODES[i]);
        }
    }

    #[test]
    fn columns_with_sparse_entries() {
        let c = columns(&[
            sample(Outcome::Ok, None, None),
            sample(Outcome::Refused, Some(111), Some(42)),
        ]);
        assert_eq!(c.count, 2);
        assert_eq!(c.result, vec![0, 2]);
        assert_eq!(c.syn_retrans, vec![-1, -1]);
        assert_eq!(c.flags, vec![flag::WARMUP; 2]);
        assert_eq!((c.errno.index, c.errno.value), (vec![1], vec![111]));
        assert_eq!(
            (c.reanchor.index, c.reanchor.sched_offset_ns),
            (vec![1], vec![42])
        );
    }
}
