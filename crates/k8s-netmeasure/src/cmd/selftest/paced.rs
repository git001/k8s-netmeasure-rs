//! Paced loopback connects at the default interval (design §3.8), which cover three
//! checks at once: loopback `sync_connect` and `anomaly` rate, pacing lateness with
//! timer slack 1 ns, and signal isolation (SIGUSR2 sent to the process mid-loop must
//! never interrupt the measuring thread).

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use netmeasure_core::measure::outcome::{Outcome, peer_closed, tcp_state_name};
use netmeasure_core::measure::pacing::{Next, Pacer};
use netmeasure_core::stats::quantile::type1;
use serde_json::json;

use super::report::{Check, Status};
use crate::listener::Listener;
use crate::measure::connect;
use crate::measure::thread::{self, ThreadSetup};
use crate::sys::{sigaction, signals, time};

pub const SLOTS: u32 = 1000;
pub const INTERVAL_NS: u64 = 10_000_000;
const TIMEOUT_NS: u64 = 1_000_000_000;
const SIGNAL_AFTER: Duration = Duration::from_secs(5);
const LISTEN_BACKLOG: u32 = 128;

/// Raw observations of the paced loop.
pub struct Observed {
    pub setup: ThreadSetup,
    pub usr2_blocked_in_thread: bool,
    pub lateness_ns: Vec<u64>,
    pub durations_ns: Vec<u64>,
    pub overruns: u32,
    pub sync: u32,
    pub anomalies: u32,
    /// `ok`, but the peer had already closed (`CLOSE_WAIT`, §3.1).
    pub peer_closed: u32,
    /// `tcpi_state` of each anomaly, by name.
    pub anomaly_states: BTreeMap<&'static str, u32>,
    pub not_ok: u32,
    pub eintr: u32,
    pub usr2_delivered: u32,
}

pub fn run() -> Vec<Check> {
    match measure() {
        Ok(o) => evaluate(&o),
        Err(e) => vec![Check {
            name: "loopback-connect",
            status: Status::Inconclusive,
            summary: format!("setup failed: {e}"),
            details: serde_json::Value::Null,
        }],
    }
}

fn measure() -> std::io::Result<Observed> {
    let listener = Listener::start(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0), LISTEN_BACKLOG)?;
    let addr = listener.local;
    sigaction::install_usr2_counter().map_err(std::io::Error::from_raw_os_error)?;
    let raiser = std::thread::spawn(|| {
        std::thread::sleep(SIGNAL_AFTER);
        sigaction::raise_usr2_process()
    });
    let mut o = thread::run(move |setup| {
        let mut o = Observed {
            setup,
            usr2_blocked_in_thread: signals::is_blocked(nix::sys::signal::Signal::SIGUSR2),
            lateness_ns: Vec::with_capacity(SLOTS as usize),
            durations_ns: Vec::with_capacity(SLOTS as usize),
            overruns: 0,
            sync: 0,
            anomalies: 0,
            peer_closed: 0,
            anomaly_states: BTreeMap::new(),
            not_ok: 0,
            eintr: 0,
            usr2_delivered: 0,
        };
        let mut pacer = Pacer::new(time::monotonic_ns() + INTERVAL_NS, INTERVAL_NS);
        for _ in 0..SLOTS {
            if let Next::At(deadline) = pacer.next() {
                time::sleep_until(deadline);
            }
            o.lateness_ns
                .push(pacer.begin(time::monotonic_ns()).late_ns);
            let a = connect::attempt(addr, TIMEOUT_NS);
            o.durations_ns.push(a.duration_ns);
            o.sync += u32::from(a.sync_connect);
            if a.outcome == Outcome::Anomaly {
                o.anomalies += 1;
                *o.anomaly_states
                    .entry(tcp_state_name(a.tcp_info.state))
                    .or_default() += 1;
            }
            o.not_ok += u32::from(a.outcome != Outcome::Ok);
            o.peer_closed += u32::from(peer_closed(a.tcp_info.state));
            o.eintr += a.eintr;
            o.overruns += u32::from(pacer.end(time::monotonic_ns()));
        }
        o
    });
    let _ = raiser.join();
    o.usr2_delivered = sigaction::usr2_delivered();
    Ok(o)
}

pub fn evaluate(o: &Observed) -> Vec<Check> {
    let mut late = o.lateness_ns.clone();
    late.sort_unstable();
    let mut dur = o.durations_ns.clone();
    dur.sort_unstable();
    let n = o.durations_ns.len() as f64;
    let q = |v: &[u64], p| type1(v, p).unwrap_or(0);
    let connect = Check {
        name: "loopback-connect",
        status: if o.anomalies == 0 {
            Status::Confirmed
        } else {
            Status::Unreliable
        },
        summary: format!(
            "{} connects: sync_connect {:.1} %, anomaly {}, not ok {}, peer closed {}, p50 {} µs, p99 {} µs",
            o.durations_ns.len(),
            100.0 * f64::from(o.sync) / n.max(1.0),
            o.anomalies,
            o.not_ok,
            o.peer_closed,
            q(&dur, 0.5) / 1000,
            q(&dur, 0.99) / 1000
        ),
        details: json!({ "connects": o.durations_ns.len(), "sync_connect": o.sync, "anomaly": o.anomalies, "anomaly_states": o.anomaly_states,
            "not_ok": o.not_ok, "peer_closed": o.peer_closed, "duration_ns": { "p50": q(&dur, 0.5), "p99": q(&dur, 0.99), "max": dur.last() } }),
    };
    let pacing = Check {
        name: "pacing",
        status: Status::Info,
        summary: format!(
            "wake-up lateness p50 {} µs, p99 {} µs, max {} µs; {} overruns; timer slack {}",
            q(&late, 0.5) / 1000,
            q(&late, 0.99) / 1000,
            late.last().copied().unwrap_or(0) / 1000,
            o.overruns,
            o.setup
                .timer_slack_errno
                .map_or("1 ns".to_owned(), |e| format!("not set (errno {e})"))
        ),
        details: json!({ "interval_ns": INTERVAL_NS, "lateness_ns": { "p50": q(&late, 0.5), "p99": q(&late, 0.99),
            "max": late.last() }, "overruns": o.overruns, "timer_slack_errno": o.setup.timer_slack_errno }),
    };
    let isolated = o.setup.signals_blocked && o.usr2_blocked_in_thread && o.eintr == 0;
    let signal = Check {
        name: "signal-isolation",
        status: match (o.usr2_delivered, isolated) {
            (0, _) => Status::Inconclusive,
            (_, true) => Status::Confirmed,
            (_, false) => Status::Unreliable,
        },
        summary: format!(
            "SIGUSR2 delivered {}×, measuring thread blocked it: {}, EINTR in timed section: {}",
            o.usr2_delivered, o.usr2_blocked_in_thread, o.eintr
        ),
        details: json!({ "usr2_delivered": o.usr2_delivered, "signals_blocked": o.setup.signals_blocked,
            "usr2_blocked_in_thread": o.usr2_blocked_in_thread, "eintr": o.eintr }),
    };
    vec![connect, pacing, signal]
}
