//! `probe` (design §3.9): a few connects with the measurement code path, one JSON
//! line per connect on stdout. No pacing, cycles, self-validation or result file.

use std::io::Write;
use std::process::ExitCode;

use netmeasure_cli::ProbeArgs;
use netmeasure_core::measure::outcome::{Outcome, peer_closed};
use netmeasure_util::time::as_nanos_u64;
use serde_json::json;

use crate::exit;
use crate::measure::connect::{self, Attempt};
use crate::measure::thread;

pub fn run(args: &ProbeArgs) -> ExitCode {
    let ProbeArgs {
        target,
        count,
        timeout,
        log: _,
    } = args.clone();
    let attempts = thread::run(move |_setup| {
        (0..count)
            .map(|_| connect::attempt(target, as_nanos_u64(timeout)))
            .collect::<Vec<_>>()
    });
    let mut out = std::io::stdout().lock();
    for (seq, a) in attempts.iter().enumerate() {
        let line = line(&target.to_string(), seq as u32, a);
        if writeln!(out, "{line}").is_err() {
            return exit::code(exit::INTERNAL);
        }
    }
    if attempts.iter().all(|a| a.outcome == Outcome::Ok) {
        exit::code(exit::OK)
    } else {
        exit::code(exit::PROBE_NOT_OK)
    }
}

/// One output line. Field names match the result file (§5).
pub fn line(target: &str, seq: u32, a: &Attempt) -> serde_json::Value {
    json!({
        "target": target,
        "seq": seq,
        "outcome": a.outcome,
        "duration_ns": a.duration_ns,
        "errno": a.errno,
        "error": a.errno.map(|e| std::io::Error::from_raw_os_error(e).to_string()),
        "sync_connect": a.sync_connect,
        "peer_closed": peer_closed(a.tcp_info.state),
        "syn_retrans": a.tcp_info.total_retrans,
        "total_rto": a.tcp_info.total_rto,
        "eintr": a.eintr,
    })
}
