//! Outcome classification of one connect (design §3.1, §3.2). Pure: the binary
//! collects the raw facts, this module decides.

use serde::Serialize;

/// Linux `TCP_ESTABLISHED` (`include/net/tcp_states.h`).
pub const TCP_ESTABLISHED: u8 = 1;

/// How `connect()` returned and how the wait for completion ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// `connect()` returned 0: the handshake completed inside the call (§3.1).
    Immediate,
    /// `EINPROGRESS`, then `ppoll` reported an event (POLLOUT/POLLERR/POLLHUP).
    Event,
    /// `EINPROGRESS`, then the deadline passed without an event.
    TimedOut,
    /// `connect()` failed at once with this errno (not `EINPROGRESS`).
    ConnectFailed(i32),
    /// A local syscall around the connect failed (socket creation, `ppoll`).
    LocalFailure(i32),
}

/// The raw facts of one attempt, read outside the timed section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub completion: Completion,
    /// `getsockopt(SO_ERROR)`; `None` if it could not be read.
    pub so_error: Option<i32>,
    /// `tcpi_state` from `TCP_INFO`; `None` if unavailable.
    pub tcp_state: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Timeout,
    Refused,
    NetError,
    LocalError,
    Anomaly,
}

impl Outcome {
    /// Part of the latency distribution (`timeout` censored), design §3.2.
    pub fn in_latency_distribution(self) -> bool {
        matches!(self, Outcome::Ok | Outcome::Timeout)
    }

    /// A network failure for the failure-rate family (§10.4).
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Outcome::Timeout | Outcome::Refused | Outcome::NetError
        )
    }

    /// Invalidates the cycle via `sample_count_short` (§3.6).
    pub fn invalidates_cycle(self) -> bool {
        matches!(self, Outcome::LocalError | Outcome::Anomaly)
    }
}

/// The errno that explains the outcome, if any.
pub fn errno(facts: &Facts) -> Option<i32> {
    match facts.completion {
        Completion::ConnectFailed(e) | Completion::LocalFailure(e) => Some(e),
        _ => facts.so_error.filter(|&e| e != 0),
    }
}

/// Decides the outcome. `ok` requires `SO_ERROR == 0` **and** `ESTABLISHED`.
pub fn classify(facts: &Facts) -> Outcome {
    match facts.completion {
        Completion::LocalFailure(_) => Outcome::LocalError,
        Completion::TimedOut => Outcome::Timeout,
        Completion::ConnectFailed(e) => errno_class(e),
        Completion::Immediate | Completion::Event => match facts.so_error {
            None => Outcome::LocalError,
            Some(0) if facts.tcp_state == Some(TCP_ESTABLISHED) => Outcome::Ok,
            Some(0) => Outcome::Anomaly,
            Some(e) => errno_class(e),
        },
    }
}

/// `refused`, `local_error` (resources of this host, not the network) or `net_error`.
pub fn errno_class(errno: i32) -> Outcome {
    match errno {
        libc::ECONNREFUSED => Outcome::Refused,
        libc::EMFILE | libc::ENFILE | libc::EADDRNOTAVAIL | libc::ENOBUFS | libc::ENOMEM => {
            Outcome::LocalError
        }
        _ => Outcome::NetError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(completion: Completion, so_error: Option<i32>, tcp_state: Option<u8>) -> Facts {
        Facts {
            completion,
            so_error,
            tcp_state,
        }
    }

    #[test]
    fn ok_needs_so_error_zero_and_established() {
        for c in [Completion::Immediate, Completion::Event] {
            assert_eq!(
                classify(&facts(c, Some(0), Some(TCP_ESTABLISHED))),
                Outcome::Ok
            );
        }
    }

    #[test]
    fn so_error_zero_in_any_other_state_is_an_anomaly_never_ok() {
        for state in (0u8..=12).filter(|&s| s != TCP_ESTABLISHED) {
            assert_eq!(
                classify(&facts(Completion::Event, Some(0), Some(state))),
                Outcome::Anomaly
            );
        }
        assert_eq!(
            classify(&facts(Completion::Event, Some(0), None)),
            Outcome::Anomaly
        );
    }

    #[test]
    fn errors_by_class() {
        let refused = facts(Completion::Event, Some(libc::ECONNREFUSED), Some(7));
        assert_eq!(classify(&refused), Outcome::Refused);
        assert_eq!(errno(&refused), Some(libc::ECONNREFUSED));
        let unreach = facts(Completion::ConnectFailed(libc::ENETUNREACH), None, None);
        assert_eq!(classify(&unreach), Outcome::NetError);
        assert_eq!(errno(&unreach), Some(libc::ENETUNREACH));
        let ports = facts(Completion::ConnectFailed(libc::EADDRNOTAVAIL), None, None);
        assert_eq!(classify(&ports), Outcome::LocalError);
        assert_eq!(
            classify(&facts(Completion::LocalFailure(libc::EMFILE), None, None)),
            Outcome::LocalError
        );
        assert_eq!(
            classify(&facts(Completion::Event, None, None)),
            Outcome::LocalError
        );
        for e in [libc::EHOSTUNREACH, libc::ECONNRESET, libc::ETIMEDOUT] {
            assert_eq!(errno_class(e), Outcome::NetError);
        }
    }

    #[test]
    fn timeout_is_censored_failure() {
        let t = classify(&facts(Completion::TimedOut, Some(0), Some(2)));
        assert_eq!(t, Outcome::Timeout);
        assert!(t.in_latency_distribution() && t.is_failure() && !t.invalidates_cycle());
        assert_eq!(errno(&facts(Completion::TimedOut, Some(0), Some(2))), None);
    }

    #[test]
    fn families() {
        assert!(Outcome::Ok.in_latency_distribution() && !Outcome::Ok.is_failure());
        assert!(Outcome::Refused.is_failure() && !Outcome::Refused.in_latency_distribution());
        assert!(Outcome::Anomaly.invalidates_cycle() && Outcome::LocalError.invalidates_cycle());
        assert!(!Outcome::NetError.invalidates_cycle());
    }
}
