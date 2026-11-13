# Phase 1 design — 02 Measurement core

Part of the [Phase 1 design](README.md) · covers §3.0–§3.5 · previous: [Build, dependencies and code structure](01-build-and-structure.md) · next: [Validity, CPU checks, selftest and probe](03-validity-selftest-probe.md)

---

## 3. Measurement core

### 3.0 What the measured quantity covers, and what it does not (review round 2)

Measuring connection establishment rather than latency on an existing connection is a
**decision**, not a simplification. It has a strength and a limit, and both belong
in the README and in the header of every report.

- **Strength: every sample is a new flow.** Each connect has a new 4-tuple (new
  ephemeral source port), so it is hashed afresh onto the NIC's TX/RX queues and onto
  any other per-flow path state (RSS, conntrack, OVS flow tables). Thousands of
  connects sample that hash space. A long-lived connection would either hit a bad
  queue or not, for its whole life. For a problem that affects only some flows, the
  connect sample is the better instrument.
- **Limit: small exposure per sample.** One sample exposes only the SYN, SYN-ACK and
  ACK of the handshake, a few packets over about one RTT. A stall or delay on the path
  is visible only if it hits exactly those packets. A lost handshake packet shows up
  as ≈ 1 s (SYN retransmission); a lost packet on an established connection is
  outside the measured quantity altogether. Latency spikes on **existing** connections
  (application RPC traffic) are therefore only partly observable through this
  instrument.
- **Consequence for the verdict**, printed in the report header and stated in the
  README: *`better` / `worse` / `equivalent` refer to TCP connection establishment
  latency on the measured paths. `equivalent` does not show that latency on
  established connections is unchanged.*
- **Later extension, not Phase 1:** an echo mode (fixed payload request/response on
  established connections, measured and reported separately from connect latency).
  The result-file schema keeps room for a second sample kind.

### 3.1 Per-sample sequence

Outside the timed section: `socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC)`,
`setsockopt(SO_LINGER, {1, 0})`. Timed section (revised in review round 1):

```
t0 = Instant::now()
deadline = t0 + timeout                       // absolute, monotonic
r  = connect(fd, addr)                        // 0, or -1/EINPROGRESS, or -1/other errno
if r == -1 && errno == EINPROGRESS:
    loop:
        n = ppoll([fd: POLLOUT], max(0, deadline - now()))   // never negative (EINVAL)
        if n == -1 && errno == EINTR: continue    // re-wait for the REMAINING time only
        if n == 0: outcome = timeout; break       // deadline reached
        break                                     // any revents (POLLOUT, POLLERR, POLLHUP)
                                                  // = the kernel finished the attempt
t1 = Instant::now()
```

Then outside: `getsockopt(SO_ERROR)` → outcome (0 → candidate `ok`, else the errno
class of §3.2); `getsockopt(TCP_INFO)` → SYN retransmissions **and `tcpi_state`**;
`close()` (RST, no TIME_WAIT). **Plausibility check (review round 2):** a sample is
`ok` only if `SO_ERROR == 0` **and** `tcpi_state` is `TCP_ESTABLISHED` or
`TCP_CLOSE_WAIT`. `SO_ERROR == 0` in any other state, e.g. still `SYN_SENT` after an
unexpected wake-up, is an `anomaly` (§3.2) and never counted as `ok`. The check costs
nothing, because `TCP_INFO` is read anyway.

**`CLOSE_WAIT` (decided after the first cluster `selftest`).** A listener that accepts
and closes at once sends its FIN right after the handshake. If the measuring thread is
preempted between the end of `ppoll` and `getsockopt(TCP_INFO)`, the FIN is already
there and the socket is in `CLOSE_WAIT`. Seen in a pod with a 1-CPU quota (1 in 1000
loopback connects) and reproduced with the thread pinned to a busy CPU (40–317 in
1000, all `CLOSE_WAIT`). `CLOSE_WAIT` is reachable only from `ESTABLISHED`, so the
handshake is proven complete and the sample is `ok`; it is counted as `peer_closed`
(sample flag, `probe` field, `selftest` detail), so a target that closes early stays
visible. Our own listeners avoid it in the first place (§4.2).
An immediate `connect` error other than `EINPROGRESS` (e.g. `ENETUNREACH`) ends the
timed section at once and is classified directly.

Details:

- **EINTR.** The measuring thread blocks SIGTERM, SIGINT and all other asynchronous
  signals with `pthread_sigmask` when it starts, so the kernel delivers them to
  another thread. EINTR is therefore not expected in the timed section. If it does
  occur, the loop continues waiting until the same absolute deadline; it never
  restarts the full timeout. Each EINTR is counted per sample (flag bit). The
  remaining time is clamped to 0: after an EINTR right at the deadline a negative
  `timespec` would be `EINVAL` and turn a timeout into a `local_error`; `ppoll` with 0
  checks readiness once more and otherwise reports the timeout correctly (review
  round 3).
- **POLLERR / POLLHUP** (e.g. RST from the target → refused) end the wait exactly like
  POLLOUT. The *time* of the event is what is measured; *which* event it was is
  determined from `SO_ERROR`.
- **Why `SO_ERROR` stays outside the timed section** (a review suggested moving it
  in). `t1` is taken immediately after the kernel has signalled the end of the attempt.
  Reading `SO_ERROR` afterwards does not move that moment; it only reads the result.
  Inside the window it would add one syscall to every sample without making anything
  more correct. The outcome is never inferred from `ppoll` alone, only from
  `SO_ERROR`. The README states this.
- `ppoll` with a `timespec` gives nanosecond deadline resolution; `poll`'s
  milliseconds would not.

**[deviation]** The prompt names `socket2::Socket::connect_timeout`. That function
switches the socket to non-blocking and back *inside* the call, i.e. two extra
`fcntl` syscalls in the timed section. Creating the socket non-blocking up front and
doing `connect` + `ppoll` ourselves keeps them out. `socket2` is still used for socket
creation and options.

**[addition]** Per sample we record `sync_connect = (r == 0)`. On loopback the kernel
can complete the handshake inside `connect()` itself (the softirq runs before the
call returns), so a non-blocking connect then returns 0 instead of `EINPROGRESS`. If
that happens, it is direct evidence for the prompt's statement that loopback bounds
the syscall cost but not the wake-up cost. It costs 1 bit per sample.

Also inside `connect()`: ephemeral port selection. Since `SO_LINGER=0` avoids TIME_WAIT,
this stays cheap, but the README will name it.

### 3.2 Outcome classification

| Outcome | Condition | In latency distribution? |
|---|---|---|
| `ok` | `SO_ERROR == 0` and `tcpi_state` is `ESTABLISHED` or `CLOSE_WAIT` (§3.1) | yes |
| `anomaly` | `SO_ERROR == 0` but any other `tcpi_state` | no; counted and logged at WARN, counts towards `sample_count_short` like a local error. Expected never to occur; `selftest` measures its rate before the first `run` (§3.8) and step 2 checks it on both NIC families (§16); the README tells the operator to report it |
| `timeout` | `ppoll` timed out | yes, censored at ≥ timeout |
| `refused` | `ECONNREFUSED` | no; counted, evaluated as failure |
| `net_error` | `EHOSTUNREACH`, `ENETUNREACH`, `ECONNRESET`, `ETIMEDOUT`, … | no; counted, evaluated as failure |
| `local_error` | `EMFILE`, `ENFILE`, `EADDRNOTAVAIL`, `ENOBUFS`, `ENOMEM`, socket creation failure | no; **not a network result** — the sample is invalid and counts towards `sample_count_short` |

**[addition]** The split of the prompt's `error` into `net_error` and `local_error` is new.
A local resource failure such as port exhaustion says nothing about the network and
must not appear as a network failure in a support case. The errno is always recorded.

### 3.3 SYN retransmissions

Primary field: `tcpi_total_retrans`. The kernel increments `tp->total_retrans` in
`tcp_retransmit_skb`, which the SYN_SENT retransmit timer also uses, and does not reset
it when the connection is established. `tcpi_retransmits` *is* reset on ACK, so it is
unusable. If the kernel's `struct tcp_info` is long enough (checked via the returned
`optlen`), `tcpi_total_rto` is recorded as well. Both are recorded as `unknown` if
`TCP_INFO` fails or returns less data than the field needs.

**Verification on the target kernel is executable, unprivileged, and also covers
`SO_MEMINFO`** (procedure refined in review round 1):

1. A listener with a small backlog that never calls `accept()`.
2. Connect repeatedly until one connect does **not** complete within 200 ms. At that
   point the accept queue was already full *before* that SYN arrived. Linux then drops
   the SYN in `tcp_conn_request` (`sk_acceptq_is_full`) → `tcp_listendrop()`
   increments `sk_drops` and `ListenDrops`. Filling first and detecting the stall
   avoids the racy case in which the queue only fills after the SYN; there the final
   ACK is dropped instead, the client is already `ESTABLISHED`, and nothing is
   retransmitted.
3. After ~0.5 s, accept one queued connection, so the first retransmitted SYN (at
   ~1 s) succeeds. *Corrected in milestone 2:* an earlier version said ~1.5 s, which
   is after the first retransmission; that one would be dropped as well and the
   connect would show two retransmissions, contradicting the `== 1` expectation
   below. Verified on a 7.x kernel: 1.04 s, `sk_drops` +1, `ListenDrops` +1,
   `tcpi_total_retrans` 1.

Evaluation, three possible results:

| Observation | Conclusion |
|---|---|
| connect ≥ ~1 s **and** own `sk_drops` +1 **and** `tcpi_total_retrans == 1` | field and drop counter confirmed |
| connect ≥ ~1 s **and** `sk_drops` +1, but `tcpi_total_retrans == 0` | the drop provably happened, the field did not count it → field unreliable, `run` records `unknown` |
| no stalled connect, or no `sk_drops` increase | **inconclusive**: no statement about the field (`selftest` exit 6) |

`SK_MEMINFO_DROPS` remains a per-socket drop indicator, not an exclusive overflow
counter (§4.2); in this controlled test nothing else touches the socket, so its
increase is attributable. The check runs as an integration test in CI and as the
documented `selftest` subcommand (§3.8) on the target kernel. A second, independent
confirmation comes from the field: every `run` records `tcpi_total_retrans` together
with the duration, so a cluster that shows ~1 s connects with retransmission 0 would
show the contradiction in the data.

**[addition] SYN backoff timing is kernel-dependent.** As far as I know, recent
upstream kernels have `net.ipv4.tcp_syn_linear_timeouts` (default 4), with which the
first SYN retransmissions use linear rather than exponential backoff (≈1 s, ≈2 s,
≈3 s total instead of ≈1 s, ≈3 s). This needs verifying against the target kernel. It
is one more reason never to infer retransmissions from duration. `tcp_syn_retries` and,
if it exists, `tcp_syn_linear_timeouts` go into the node context.

### 3.4 Pacing, round-robin, warm-up

- One global slot schedule for the measuring thread. `--interval` is the
  start-to-start spacing between consecutive slots. **Default 10 ms.**
- **Loopback share** (review round 2, **[deviation]** from the prompt's plain
  round-robin):
  - Every `--loopback-every` slot (default 10) goes to the loopback control; all other
    slots go round-robin to the real targets. The loopback control therefore still
    appears in every cycle, interleaved with the real targets, but takes only 1/10 of
    the slots instead of 1/(T+1).
  - Its purpose is to detect a shift in local behaviour, and the report compares it on
    the **median** (§10.6), which needs far fewer samples than a tail quantile.
  - `--loopback-every 2` restores strict alternation in `pair` mode. Each real target
    still needs its full `--samples`, so shrinking the loopback share does not cost
    tail resolution on the real targets; it saves wall time.
- Absolute deadlines via `clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)`; timer slack of
  the measuring thread set to 1 ns (`prctl(PR_SET_TIMERSLACK)`), so the default 50 µs
  slack does not add itself to the lateness.
- **Overrun rules** (made exact in review round 2; the tests in §11 check exactly
  these):
  1. A slot whose sample runs past the next slot's deadline is completed normally.
  2. The immediately following slot starts **at once**; its scheduled start is set to
     that moment (`scheduled = now`, re-anchor).
  3. From there the full interval applies again: the next deadline is
     `scheduled + interval`.
  4. Missed slots are **never** made up; there is no burst. The run takes longer
     instead.

  Per sample: `late_ns = actual_start − scheduled_start` (≥ 0), and a flag for "started
  at once after an overrun".
- Warm-up: the first `--warmup` (default 50) samples **per target per run** are
  measured and stored but marked warm-up and excluded from all statistics. `--samples`
  (default 5000) counts post-warm-up samples per real target. The loopback control
  gets what its share yields.
- Wall time without overruns = `(samples + warmup) × T_real × interval ×
  e / (e − 1)` with `e = --loopback-every`. At defaults in `pair` mode:
  5050 × 10 ms × 10/9 ≈ **56 s** (≈ 561 loopback samples).

### 3.5 Run and cycle

- **The cycle is defined by a slot count, not by wall time.** The slot count is fixed;
  the duration varies with overruns. That keeps `sample_count_short` meaningful: it
  triggers only for local errors or interruption, not for every overrun.
- **Default: 10 cycles per run, slots distributed evenly** (revised in review round
  2). An earlier version used a fixed 20 s cycle. With 5000 samples on two targets one
  cycle was then exactly the 20 % tail reserve of §10.1, and the run ended with a tiny
  remainder cycle whose statistics were noise.
  - Now the run is split into `--cycles` (default 10) cycles by **counted samples per
    real target**: each cycle holds `samples / cycles` counted samples per target
    (remainder distributed one by one from cycle 0), and **cycle 0 additionally holds
    the warm-up** samples (corrected in review round 3). The loopback slots fall where
    the global slot index puts them. Each cycle's slot count follows deterministically
    and is recorded; there is no remainder cycle.
  - At defaults in `pair` mode: 500 counted samples per cycle, cycle 0 with 550
    target samples. The slot counts are `[611, 555, 556, 555, 556, 555, 556, 555, 556,
    556]` = 5611.
  - Why not distribute the *slots* evenly (the round-2 version): warm-up then eats
    into cycle 0's counted samples only, so excluding two other cycles removed slightly
    more than 20 % of the counted data. 3990 samples leave 39 exceedances above p99,
    not 40.
  - So one invalid cycle always costs exactly 10 % of the counted data, independent of
    interval, target count, sample count or warm-up. Two excluded cycles leave exactly
    4000 samples and therefore exactly the tail minimum m = 40 for p99 at the default
    5000 samples.
  - The resulting cycle length is clamped to **[2 s, 60 s]**: more cycles for very long
    runs, fewer for very short ones; the effective count is recorded. At defaults in
    `pair` mode a cycle lasts ≈ 5.6 s, well inside the shutdown deadline (§8).
  - `--cycle <duration>` overrides the count with a fixed length (the remainder is
    again distributed evenly).
  - `serve` (Phase 3) has no run end and uses a fixed 10 s cycle.
- Around each cycle, on the main thread, outside any timed section: before-snapshot
  (own cgroup, own listener `sk_drops`, netstat; underlay ethtool; the **NIC path**
  of the own node and of every target, §3.6; every target's control-port `snapshot`) → measuring thread runs the cycle (load threads run
  continuously) → after-snapshot → validity evaluation (core).
