//! SYN retransmission field, listener drop indicator and SYN backoff (design §3.3):
//! fill a backlog-1 accept queue until a connect stalls, then measure one connect
//! whose SYN is dropped and accept one queued connection after ~0.5 s, i.e. before
//! the first SYN retransmission at ~1 s. Expected: duration ≈ 1 s, own `sk_drops` +1,
//! `tcpi_total_retrans` = 1.

use std::os::fd::AsFd;
use std::time::Duration;

use netmeasure_core::measure::outcome::Outcome;
use netmeasure_core::measure::sockinfo::{self, Field};
use serde_json::json;

use super::lo;
use super::report::{Check, Status};
use crate::diag::node;
use crate::measure::{connect, thread};
use crate::sys::sockopt;

const STALL_NS: u64 = 200_000_000;
const MEASURED_TIMEOUT_NS: u64 = 5_000_000_000;
const ACCEPT_AFTER: Duration = Duration::from_millis(500);
const MAX_FILL: usize = 64;

pub fn run() -> Vec<Check> {
    match measure() {
        Ok(m) => evaluate(&m),
        Err(e) => vec![inconclusive(
            "syn-retrans-field",
            format!("setup failed: {e}"),
        )],
    }
}

/// Raw observations of the procedure.
pub struct Observed {
    pub stalled: bool,
    pub filled: usize,
    pub attempt: connect::Attempt,
    pub sk_drops_delta: Option<u32>,
    pub listen_drops_delta: Option<u64>,
}

fn measure() -> std::io::Result<Observed> {
    let (listener, addr) = lo::bounded(1)?;
    let mut held = Vec::new();
    let mut stalled = false;
    while held.len() < MAX_FILL {
        match std::net::TcpStream::connect_timeout(&addr.into(), Duration::from_nanos(STALL_NS)) {
            Ok(s) => held.push(s),
            Err(_) => {
                stalled = true;
                break;
            }
        }
    }
    let drops_before = drops(&listener);
    let ld_before = node::netstat().get("TcpExt.ListenDrops").copied();
    let acceptor = std::thread::spawn(move || {
        std::thread::sleep(ACCEPT_AFTER);
        let _ = listener.accept();
        listener
    });
    let attempt = thread::run(move |_| connect::attempt(addr, MEASURED_TIMEOUT_NS));
    let listener = acceptor
        .join()
        .map_err(|_| std::io::Error::other("acceptor panicked"))?;
    let drops_after = drops(&listener);
    let ld_after = node::netstat().get("TcpExt.ListenDrops").copied();
    Ok(Observed {
        stalled,
        filled: held.len(),
        attempt,
        sk_drops_delta: drops_before
            .zip(drops_after)
            .map(|(b, a)| a.wrapping_sub(b)),
        listen_drops_delta: ld_before.zip(ld_after).map(|(b, a)| a.saturating_sub(b)),
    })
}

fn drops(listener: &socket2::Socket) -> Option<u32> {
    let (buf, len) = sockopt::meminfo(listener.as_fd()).ok()?;
    sockinfo::decode_meminfo_drops(sockopt::SK_MEMINFO_DROPS, &buf, len).known()
}

/// The three-way evaluation of §3.3 plus the listener-drop and backoff checks.
pub fn evaluate(m: &Observed) -> Vec<Check> {
    let a = &m.attempt;
    let details = json!({
        "stalled": m.stalled, "filled": m.filled, "outcome": a.outcome,
        "duration_ns": a.duration_ns, "syn_retrans": a.tcp_info.total_retrans,
        "total_rto": a.tcp_info.total_rto,
        "sk_drops_delta": m.sk_drops_delta, "listen_drops_delta": m.listen_drops_delta,
    });
    let dropped = m.sk_drops_delta.is_some_and(|d| d >= 1);
    let delayed = a.outcome == Outcome::Ok && a.duration_ns >= 900_000_000;
    let field = if !m.stalled {
        inconclusive(
            "syn-retrans-field",
            "no connect stalled: the accept queue never filled".into(),
        )
    } else if !dropped || !delayed {
        inconclusive(
            "syn-retrans-field",
            "no provable SYN drop with a delayed connect".into(),
        )
    } else {
        match a.tcp_info.total_retrans {
            Field::Known(n) if n >= 1 => check(
                "syn-retrans-field",
                Status::Confirmed,
                format!("tcpi_total_retrans = {n} after a dropped SYN"),
            ),
            Field::Known(_) => check(
                "syn-retrans-field",
                Status::Unreliable,
                "SYN dropped but tcpi_total_retrans = 0: run records unknown".into(),
            ),
            Field::Unknown => check(
                "syn-retrans-field",
                Status::Unreliable,
                "TCP_INFO too short for tcpi_total_retrans".into(),
            ),
        }
    };
    let indicator = match m.sk_drops_delta {
        _ if !m.stalled => inconclusive("listener-drop-indicator", "no SYN drop provoked".into()),
        Some(d) if d >= 1 => check(
            "listener-drop-indicator",
            Status::Confirmed,
            format!("own sk_drops +{d} (SO_MEMINFO usable)"),
        ),
        Some(_) => check(
            "listener-drop-indicator",
            Status::Unreliable,
            "sk_drops unchanged: run uses the netns fallback".into(),
        ),
        None => check(
            "listener-drop-indicator",
            Status::Unreliable,
            "SO_MEMINFO unavailable: run uses the netns fallback".into(),
        ),
    };
    let backoff = if delayed {
        check(
            "syn-backoff",
            Status::Info,
            format!(
                "first SYN retransmission after {:.3} s",
                a.duration_ns as f64 / 1e9
            ),
        )
    } else {
        inconclusive("syn-backoff", "no retransmitted connect observed".into())
    };
    [field, indicator, backoff]
        .map(|c| Check {
            details: details.clone(),
            ..c
        })
        .into()
}

fn check(name: &'static str, status: Status, summary: String) -> Check {
    Check {
        name,
        status,
        summary,
        details: serde_json::Value::Null,
    }
}

fn inconclusive(name: &'static str, summary: String) -> Check {
    check(name, Status::Inconclusive, summary)
}
