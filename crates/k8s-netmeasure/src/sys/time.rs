//! Monotonic clock in nanoseconds, absolute sleeps and timer slack (design §3.4).

use nix::errno::Errno;
use nix::sys::time::TimeSpec;
use nix::time::{ClockId, ClockNanosleepFlags, clock_gettime, clock_nanosleep};

const NS_PER_S: u64 = 1_000_000_000;

/// `CLOCK_MONOTONIC` in nanoseconds (vDSO, no syscall).
pub fn monotonic_ns() -> u64 {
    // CLOCK_MONOTONIC always exists on Linux; the call cannot fail.
    let ts = clock_gettime(ClockId::CLOCK_MONOTONIC).expect("CLOCK_MONOTONIC");
    (ts.tv_sec() as u64) * NS_PER_S + ts.tv_nsec() as u64
}

/// Nanoseconds as a `timespec`, for `ppoll` and `clock_nanosleep`.
pub fn timespec(ns: u64) -> TimeSpec {
    TimeSpec::new((ns / NS_PER_S) as _, (ns % NS_PER_S) as _)
}

/// Sleeps until the absolute monotonic time `deadline_ns` (`TIMER_ABSTIME`); an
/// interruption re-sleeps to the same deadline.
pub fn sleep_until(deadline_ns: u64) {
    let deadline = timespec(deadline_ns);
    while clock_nanosleep(
        ClockId::CLOCK_MONOTONIC,
        ClockNanosleepFlags::TIMER_ABSTIME,
        &deadline,
    ) == Err(Errno::EINTR)
    {}
}

/// Sets the calling thread's timer slack (default 50 µs) to `ns`.
pub fn set_timer_slack(ns: u64) -> Result<(), i32> {
    nix::sys::prctl::set_timerslack(ns as _).map_err(|e| e as i32)
}
