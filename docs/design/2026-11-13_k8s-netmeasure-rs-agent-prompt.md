# k8s-netmeasure-rs — TCP connect latency measurement for OpenShift

Repository: `k8s-netmeasure-rs` · binary `k8s-netmeasure` · crates `netmeasure-core`, `k8s-netmeasure`
Metric prefix: `netmeasure_` · environment variable prefix: `NETMEASURE_`

(The metric prefix is deliberately shorter than the repository name. `k8s_netmeasure_connect_duration_seconds` is unwieldy in PromQL and in dashboards. Say so if you disagree.)

Language is decided: **Rust, edition 2024, toolchain 1.100, stable only.** No nightly features.

---

## Step 0, before any design or code

A local clone of https://github.com/cloud-bulldozer/k8s-netperf is available; its path is in `local/CONTEXT.local.md`. It is a reference, not a base — we are not extending a Go project. Read it and report, in writing, with file and function references. A pre-check of these questions exists in `docs/design/2026-11-13_k8s-netperf-step0-evaluation.md`; verify it against the code rather than trusting it:

1. What does it measure, exactly, in code? Not what the README says. Several tools in this space document one thing and measure another; see "Known traps" below.
2. What is its process lifecycle: one-shot job, long-running service, or operator?
3. How does it place and discover test endpoints across nodes and zones?
4. How does `--all` / `--tcp-tolerance` work, and what exactly does it compare? It is the only automated better/worse verdict found anywhere in this tool space, so the mechanism matters.
5. How do `--label` and `--annotation` flow through to the created objects and to the results?
6. Where do results go, and in what format?
7. Which design decisions are worth adopting, which are worth avoiding, and why.

Deliver this evaluation and wait before writing production code.

---

## Why this exists

A latency-sensitive application on OpenShift 4.22 with OVN-Kubernetes, running in a public cloud, has a p99 latency regression. Support cases are open with the OS vendor and the cloud provider. The vendors propose changes such as a test kernel with NIC driver fixes or different NIC settings.

### Confidential context

The concrete environment — application, cloud, NIC hardware, case numbers, hand measurements, local paths — is in **`local/CONTEXT.local.md`**. Read it; you need it to make sensible decisions. The `local/` directory is git-ignored and the repository is **public**. Therefore:

- **Never copy anything from `local/CONTEXT.local.md` into a committed file**: not into code, comments, README, docs, test fixtures, examples, default configuration, log examples or commit messages. This includes case numbers, application and cloud names, NIC models and driver names in an environment-identifying context, and the measured values.
- Committed examples use neutral placeholders (`ticket=CASE-0001`, `app`, `nic0`, invented round numbers).
- Environment-specific configuration (e.g. a driver-specific counter allowlist) lives as an example file under `local/`, not as a committed default.
- Before proposing a commit, check the diff for anything from the context file and say explicitly that you did.

**Every proposed change must be verified: better, worse, or no measurable change, with evidence good enough to submit to a support case.** "No measurable change" must be demonstrated as equivalence within a declared margin, not merely inferred from a non-significant difference (see Statistics). That is the whole purpose of this tool. Continuous observation is secondary.

Hand measurements with bash and `/dev/tcp` (values in the context file) show connect latencies from a few tens of µs (pod to pod, same node) to tens of ms (across nodes, both through the overlay and node to node bypassing it), with the same distribution on two different NIC generations.

ICMP between pods is blocked by NetworkPolicy, so ping-based measurement is unavailable.

---

## Known traps — these are why we are building rather than adopting

All verified against source code, not documentation:

- **NetObserv** advertises "TCP handshake round-trip time" but reads `srtt_us >> 3` at `fentry/tcp_rcv_established`, a hook unreachable during the handshake. It measures established-connection RTT, keeps the per-flow maximum, and substitutes a synthetic 10 µs when no value is available.
- **kubenurse**'s httptrace histograms all measure elapsed-since-request-start rather than per-phase duration, because the trace closures capture a `start` variable reassigned just before `RoundTrip`. `connect_done` therefore includes DNS.
- **goldpinger**'s TCP probe (`doTCPProbe`, `net.DialTimeout`) applies only to external `--tcp-targets`, never to peers. Its only Prometheus output is an error counter (`goldpinger_tcp_errors_total`). It is timed only for the `/check_all` JSON response, as `time.Since(start).Milliseconds()` — integer milliseconds, so a sub-millisecond connect reads as 0, and the dial includes DNS resolution for hostname targets. Peer latency is an HTTP round trip (`goldpinger_peers_response_time_s`).
- **goldpinger** also has an opt-in UDP peer probe (`--udp-enabled`, `goldpinger_peers_udp_rtt_s`). It observes the **mean** RTT of a burst of packets (default 10) once per ping, not per-packet values, so its histogram cannot show a tail.
- **NetObserv and kubenurse**, and goldpinger's HTTP histograms, default to buckets starting at 5 ms. goldpinger's UDP RTT histogram starts at 100 µs. Our range starts at a few tens of µs, so same-node samples would land in the lowest bucket of every one of them.
- **k8s-netperf** reports the arithmetic mean of per-process p99 values, averaged again across samples, truncated to integer µs; its only verdict is a percent threshold on mean throughput between two paths of the same run. See the Step 0 evaluation.
- An earlier measurement of ours was silently invalidated because the measuring container had a 50m CPU limit and was CFS-throttled. Its ~100 ms peaks were the measuring tool, not the network.

**Document in the README precisely what this tool measures, in terms of the TCP state machine.**

---

## What is measured

The quantity is **client-observed TCP connection establishment latency**: the time from entering `connect()` until the measuring thread has observed that the client socket reached `ESTABLISHED`. On the wire that is SYN sent → SYN-ACK received → ACK sent. With a non-blocking connect and `poll()` (see Measurement requirements) the measured time additionally contains the wake-up and scheduling of the measuring thread after the kernel has completed the handshake. That share can differ between kernel versions; the README must name it.

It includes the local and remote kernel network paths (routing, netfilter/OVS/OVN, conntrack, driver, NIC) and the remote kernel's SYN handling. It does **not** include the server application's `accept()`: the kernel completes the handshake before `accept()` is called. It is not a pure network RTT, and the README must say so.

**Local control measurement (loopback).** Every cycle includes `127.0.0.1:<listener port>` as an additional target, measured round-robin with the others. It is a **local control measurement, not a baseline of the measurement overhead**. On loopback the handshake most likely completes within the `connect()` syscall itself, so the thread never sleeps: loopback bounds the syscall and local stack cost, not the scheduler wake-up cost, and it never touches a NIC or driver path. A shift in loopback between the sides indicates that local behaviour changed; an unchanged loopback does not prove that all local kernel effects are unchanged. `report` flags a significant loopback shift; the README states both limits, so that nobody over-interprets a small loopback change at the tens-of-µs scale.

**SYN retransmissions.** An initial SYN lost on the wire shows up as a connect of ≈1 s (initial RTO), a second loss as ≈3 s. **Never infer a retransmission from the duration.** After `connect()` returns — outside the timed section, before `close()` — read `TCP_INFO` and record the retransmission count of the handshake per sample. Verify on the target kernel which field counts SYN retransmissions (`tcpi_total_retrans` is the candidate) and document the finding. If it cannot be established reliably, record the value as `unknown`, not as 0. A SYN retransmission is a network finding, not a measurement error.

**Measurement versus diagnosis.** The verdict is based on the connect latency distribution only (plus failure and timeout counts). Everything else — SYN retransmissions, NIC ring sizes and channels, driver and TCP counters, listener counters — is diagnostic context that may explain a change but never decides one. Keep this separation in the module structure, in the result file and in the report output.

---

## Phases

Build in three phases. Each phase ends in something usable. Do not start a phase before the previous one is agreed.

**Phase 1 — measurement quality.** `run`, `listen` and `report` on result files. Listener, measurement core, loopback control, warm-up, round-robin, self-validation, `pair` and `sampled` topologies with explicit targets, optional background load, pacing, result file, statistical report for both comparison designs (sequential and concurrent node groups). No Kubernetes API, no Prometheus. Deployable ad hoc as a static binary via `oc cp` and `oc exec` into any pod (overlay) or a `hostNetwork` debug pod (underlay). This phase alone must be able to answer the vendor cases.

**Phase 2 — Kubernetes/OpenShift.** Node and zone discovery via the API, `mesh` topology, automatic collection of node, kernel and NIC context, Helm chart with Job and DaemonSet, SCC, RBAC, PVC for result files.

**Phase 3 — observability.** `serve`, Prometheus classic and native histograms, Loki logging, Grafana dashboard, underlay NIC and TCP counters as metrics, `report` reading Prometheus windows via Thanos Querier.

---

## Subcommands

- **`k8s-netmeasure listen`** — runs only the listener. Used on the target side in Phase 1.
- **`k8s-netmeasure run`** — one-shot. Fixed sample count, writes a result file, exits. The primary mode for vendor-change verification.
- **`k8s-netmeasure report`** — compares two sets of result files (Phase 1) or two Prometheus windows (Phase 3).
- **`k8s-netmeasure serve`** — long-lived (Phase 3). Listener plus own cycle timer, `/metrics` endpoint, DaemonSet. For continuous observation and for seeing whether outliers correlate with anything else.

`run` must not rely on Prometheus. A Job shorter than the scrape interval (UWM default 30 s) may never be scraped, its series disappear when it exits, and UWM has no Pushgateway. If `run` exposes metrics at all (Phase 3), they are best-effort; the result file is the record.

Two deployment variants from the same binary: `path=overlay` on the pod network, and `path=underlay` with `hostNetwork: true`, which additionally reads host NIC and TCP counters.

**Run and cycle.** A *run* is one invocation of `run`: a fixed number of samples per target (`--samples`, see Measurement requirements), with fixed configuration. A run is divided into *cycles* of configurable length (propose a default in the range of tens of seconds). The cycle is the unit of self-validation (cgroup, listener and counter reads before and after), of the per-cycle log line and of the `cycle_id`; an invalid cycle loses only its own samples, not the run. Short cycles also keep shutdown cheap. `serve` uses the same cycle concept with its own timer, but has no run end.

### Process behaviour and shutdown

The binary runs directly as the container process of a Kubernetes Job or DaemonSet — no supervisor, no init wrapper. Kubernetes provides restarts, probes and log collection. Therefore:

- No self-daemonising, no fork, logs to stdout only (JSON lines, see Logging), nothing written to stderr except a last-resort message before a crash.
- **Readiness**: `listen` and `serve` become ready only once the listener is bound and accepting and the self-checks (CPU quota floor) have passed. Expose readiness on the control port (Phase 1) and as `/ready` next to `/metrics` (Phase 3). Kubernetes probes must **never** target the measured listener port, since every probe connection would show up in its counters.
- **Meaningful, documented exit codes**: success; configuration error; refused to start (e.g. CPU quota below the floor); completed but with invalid cycles; interrupted by signal; internal error. Propose the numbering.

**SIGTERM and SIGINT must be handled cleanly**, because a run is long: with the default of 5000 samples per target at a 10 ms interval, a run takes minutes, and even a single cycle can approach the grace period, while Kubernetes sends SIGKILL after `terminationGracePeriodSeconds` (default 30 s). Rules:

- On the first SIGTERM/SIGINT, stop scheduling new samples and stop background load threads. No sample is cut off mid-`connect()`; an in-flight connect completes or times out.
- If the running cycle can finish within `--shutdown-deadline` (configurable, default well below the default grace period; propose a value), finish it. Otherwise end it immediately and record it as an incomplete cycle with the invalidity reason `interrupted`, keeping all samples taken so far.
- Always write what exists: in `run`, the result file is written with run status `interrupted`, valid and complete cycles stay usable; the final INFO/WARN log lines are emitted; files are written atomically (write to a temporary name, then rename), so a SIGKILL can never leave a truncated result file that looks complete.
- `listen` stops accepting, closes the listening socket, and exits; a source measuring against it sees `refused`, which `report` must be able to distinguish from a network failure via the target context (listener gone vs. node reachable).
- A second SIGTERM/SIGINT during shutdown exits immediately, still without leaving a truncated file.
- Exit with the "interrupted by signal" code.
- The chart sets `terminationGracePeriodSeconds` comfortably above `--shutdown-deadline`, and documents the relation; `serve` uses shorter cycles than `run` (propose defaults) so that a rolling update loses little data.
- Signal handling must not put an async runtime or any extra thread into the measuring thread's path; the measuring thread checks a shutdown flag between samples.

### Target context and control port

The target node's context matters as much as the source's: a kernel change applies to both ends. The listener must make its own context (node, kernel, NIC driver and version, ring sizes, channels, `somaxconn`, its own socket's drop counter, namespace listen counters) available to `run`, which fetches it **before and after** each cycle, outside the timed section, and stores it in the result file.

This goes through a **separate control port** (required); it also serves readiness. Propose the protocol (e.g. one JSON line per request). It must not share the measured port and must not be HTTP probing in disguise.

---

## Topologies

- **`pair`** — one source, one target. The first mode for the vendor cases: controlled, cheap, easy to repeat.
- **`sampled`** — a chosen set of source/target pairs, spread across zones. Needed because a p99 regression may be node-specific, which `pair` can miss.
- **`mesh`** (Phase 2) — every node to every node. Cost grows as n²: 100 nodes × 99 targets × 5000 samples is ~50 M connects per run, and 495,000 paced connects per source — ~82 min at a 10 ms interval. A full mesh is therefore an observation tool, not the path to a verdict. State the cost in the README, and make sure concurrent sources cannot overload one listener (see Listener).

In Phase 1 targets are given explicitly (`--target host:port`, repeatable, or a file).

To finish faster, use more sources (pods on different nodes), never more measuring threads in one process.

---

## Background load (Phase 1, `run` only)

There is **no `--parallel`**. Concurrent measuring threads would change the quantity being measured — CPU and softirq contention on the measuring node, a higher conntrack/OVS flow-setup rate, different RSS queue distribution — and would mix "finish faster" with "measure under load".

The application's regression may only appear under concurrency, so load must still be available, as an **explicit experimental variable**:

- Exactly one measuring thread, unchanged. Its samples are the latency distribution.
- Optional background load: `--load-threads N` and/or `--load-connects <rate per second>` (propose the interface) open and reset connections to the same targets (not the loopback control) on separate threads. This is **TCP connection churn load**: it stresses SYN handling, conntrack/OVS flow setup and the driver's per-connection path. It does **not** simulate many concurrently established connections (e.g. 100 open application client connections), which is a different load profile; name that as a possible later extension in the README, do not build it now. Their durations are **not** part of the latency distribution; their results (ok, timeout, refused, error) and SYN retransmissions are recorded separately per cycle.
- The load parameters are part of the effective configuration in the result file and of every log line. Load is a property of a run, not of a cycle: it is constant for the whole run.
- The CPU quota floor scales with the number of threads (measuring + listener + load); propose the formula. Refuse to start below it.
- The README must state that results under load depend much more on the node's CPU situation, and that comparable load results need Guaranteed QoS, ideally with exclusive CPUs (static CPU manager policy).
- Intended use: idle and loaded runs, each before and after a change. This shows whether a fix acts only under load.

`serve` does not generate load.

---

## Measurement requirements

- **Round-robin across targets**, not target by target. A two-second network event must spread across all targets rather than corrupting one completely.
- **Warm-up**: discard the first N samples per target per run (default 50).
- **Fixed sample count per run** (`--samples`), configurable. **Default: 5000 per target**, sized for a p99 verdict: ~50 observations above the p99, consistent with the tail minimum (see Statistics). 1000 would leave only ~10 and the tool would warn in its default configuration. At a 10 ms interval that is ~50 s per target per run — in `pair` mode with the loopback control ~100 s. p99.9 needs roughly ten times more and must be requested explicitly. State in the README what sample count each quantile needs and what it costs in wall time, which scales with the number of targets.
- **Measurement on a dedicated OS thread**, no async executor. Rationale: an executor's wake-up jitter is of the same order as the quantity being measured. State honestly that no published measurement of this effect was found — it is structural reasoning, not a cited result.
- **Connect timeout.** A plain blocking `connect()` waits ~127 s with default `tcp_syn_retries`. Use a non-blocking connect with `poll()` on the same dedicated thread (`socket2::Socket::connect_timeout` does this), and document the extra wake-up in the timed section. Default timeout **above 3 s** (propose a value) so that the first and second SYN retransmissions are observed as durations rather than collapsed into timeouts.
- **Paced, not back-to-back.** `--interval` sets the start-to-start spacing between connects of the measuring thread (propose a default; the wall-time figures in this document assume 10 ms). Without pacing, the connect rate depends on the latency itself, so a slower kernel would automatically generate less load and the two sides would not run under the same conditions. If a connect overruns its slot, start the next one immediately, do **not** catch up with a burst, and record the overrun (scheduled vs actual start) per sample. The wake-up lateness of the pacing sleep is also the measure for scheduling jitter on the measuring thread (see Self-validation).
- Time only the connect with a monotonic clock. No DNS resolution, no allocation, no logging inside the timed section.
- **Fixed per-sample order:** create the socket and set `SO_LINGER = 0` (outside the timed section) → timed `connect()` → read `TCP_INFO` → `close()`. `SO_LINGER` takes effect only at close, so setting it beforehand keeps any syscall out of the gap between measurement and `TCP_INFO`; `close()` before `TCP_INFO` would lose the retransmission count. `SO_LINGER = 0` resets sockets rather than accumulating them in TIME_WAIT.
- **Timeouts and errors are results, not gaps.** Record each with `result = timeout | refused | error`. A timeout is a censored observation (duration ≥ timeout) and must enter quantile estimation as such; dropping it biases the tail downwards.

### Listener

- **Own listener on a dedicated port by default**, so no foreign application sees thousands of aborted connections. Probing a foreign port must be available as an option, since that is the path real traffic takes.
- The accept queue **can** become the bottleneck. When it is full, Linux has two paths, both counted by the same counters: it drops the incoming **SYN** (`tcp_conn_request`), so the client retransmits after ~1 s and the tool manufactures its own outlier; or it drops the handshake's final **ACK** (`syn_recv_sock` overflow, with `tcp_abort_on_overflow = 0`), in which case the client is already `ESTABLISHED` and the measured `connect()` is unaffected — since we reset immediately, nothing further happens. Run `accept()` continuously on a dedicated thread and close immediately; make the backlog configurable and record the effective value (bounded by `net.core.somaxconn`).
- **Per-socket drop indicator.** The kernel's `tcp_listendrop()` increments the listening socket's own `sk_drops` in addition to the namespace counter. Read it for the tool's own listening socket via `getsockopt(SO_MEMINFO)`, field `SK_MEMINFO_DROPS`, before and after every cycle. It is a per-socket drop indicator, **not** an exclusive accept-queue-overflow counter: other paths may increment it too, and it cannot distinguish the SYN path (affects measurement) from the ACK path (does not). Verify on the target kernel which paths increment it and document the finding. Any increase marks the cycle invalid (`listener_drops`). This is **deliberately conservative**: with a continuously accepting listener, drops should never occur, so any drop means the tool was not in its intended state; a cycle wrongly invalidated is cheaper in a support case than a self-made outlier. The README states this.
- `TcpExt.ListenOverflows` and `TcpExt.ListenDrops` from `/proc/net/netstat` are recorded as context only. They are per network namespace, not per socket: in the overlay variant they include every container of the pod (sidecars too), and with `hostNetwork` every listener on the node. An increase there without an increase of the tool's own `sk_drops` is annotated (`netns_listen_drops`), not treated as invalidity. If `SO_MEMINFO` turns out to be unusable, fall back to the namespace counters and label the reason `listener_drops_suspected`.

---

## Self-validation — non-negotiable

Validity is a set of reasons, not a single boolean. A cycle is invalid if any of these occurred:

- `cpu_throttled` — read the process's own cgroup v2 `/sys/fs/cgroup/cpu.max` and `/sys/fs/cgroup/cpu.stat` before and after every cycle; `throttled_usec` increased.
- `listener_drops` — the tool's own listening socket's drop counter increased, on the source's or the target's listener (target values via the target context). `listener_drops_suspected` only in the fallback described under Listener.
- `target_set_changed` — targets appeared or disappeared during the cycle.
- `sample_count_short` — fewer valid samples than configured.
- `interrupted` — the cycle was ended early by a shutdown signal (see Process behaviour and shutdown).
- Scheduling jitter: the wake-up lateness of the pacing sleep is recorded per cycle (distribution, not just max). Propose a threshold and whether exceeding it invalidates (`scheduling_jitter`) or only annotates.
- Pacing overruns are recorded per cycle; propose whether a high overrun fraction invalidates.

Timeouts and SYN retransmissions do **not** invalidate a cycle; they may be the finding. `report` evaluates connection failures separately from the quality of the measurement.

Invalid cycles are still recorded and published, labelled with their reasons, so they can be excluded or inspected.

Refuse to start if the CPU quota is below a configurable floor (default 500m).

Parse the cgroup and `/proc` files directly. The available crates are either container-runtime-oriented or unmaintained since early 2025.

---

## Result file

`run` writes one result file per run. Requirements:

- Lossless: every sample, in measurement order, with its target, scheduled and actual monotonic start offset, duration in ns, result and SYN retransmission count (or `unknown`). Required for a block bootstrap.
- Self-describing: `schema_version`, `build_info`, the full effective configuration (sample count, warm-up, interval, cycle length, timeout, backlog, port, path, topology, load parameters), run labels, wall-clock start and end. Wall-clock timestamps must allow `report` to establish that runs of the two node groups in a concurrent comparison actually overlapped.
- Diagnostics are kept in a separate part of the file from the measured samples.
- Per cycle, when load is active: load connection counts by result and their SYN retransmissions, kept separate from the measured samples.
- Context for both ends: source and every target — node, zone, instance type, uptime, kernel, NIC driver, driver version, firmware, ring sizes, channels, MTU, CPU quota, `somaxconn` — before and after.
- Per cycle: `cycle_id` (ULID), start/end, validity and reasons, cgroup deltas, listener counter deltas, and in the underlay variant the netstat and ethtool counter deltas (the allowlists are defined under Prometheus metrics, but apply from Phase 1).
- Size: 100,000 samples should stay in the single-digit MB range. Propose a representation (e.g. columnar arrays inside JSON, optionally compressed) that keeps the header human-readable.
- Output: `--output <path>`. No stdout option: stdout carries the JSON log lines. A finished Job's container cannot be read with `oc cp`, and multi-MB output through Loki is unreliable, so the chart's Job template writes to a PVC (Phase 2). In Phase 1 the binary runs via `oc exec` in a running pod, from which the file is copied.

---

## Run metadata

Repeatable `--label key=value` on the command line, plus `NETMEASURE_LABELS` for DaemonSet use. These identify a run so that before and after can be selected by meaning rather than by timestamp:

    k8s-netmeasure run --label phase=baseline --label ticket=CASE-0001 --label kernel-change=none

They go into the result file header and into every log line's fields. In Phase 3 they must **not** become labels on the latency histogram — that multiplies the series count. They go on a separate info metric:

    netmeasure_run_info{phase="baseline", ticket="CASE-0001", kernel_change="none"} 1

Document the PromQL join:

    netmeasure_connect_duration_seconds_bucket
      * on(instance) group_left(phase) netmeasure_run_info

Separately, pass `podLabels` and `podAnnotations` through the Helm values onto the tool's own pods, so admission policies, cost tagging and NetworkPolicy selectors can target it.

---

## `report` subcommand

    k8s-netmeasure report --before 'results/baseline/*.json' --after 'results/testkernel/*.json' \
        --expect-change kernel --equivalence-margin 10%

Each side accepts one or more result files (several runs, several sources). Results are grouped by source/target pair and path.

### Comparison designs

`report` supports two designs and states in its output which one was used.

**Sequential** — the same nodes before and after a change, i.e. two time windows. Exposed to reboot, placement, time-of-day and load differences; needs several runs per side (see Statistics).

**Concurrent node groups** — the stronger design for our case, preferred whenever possible. A test kernel is rolled out through a MachineConfigPool to a *subset* of nodes, so both kernels exist at the same time:

- test-kernel node A → baseline node X, and baseline node B → the same baseline node X, measured in the **same time window**. This is a **matched comparison**: time of day, cluster load and the target are shared. It does **not** by itself isolate the kernel — A and B can still differ in CPU load, cloud host placement, NUMA layout and network path. A causal claim about the kernel needs the crossover described below.
- **Reboot both groups.** The test-kernel nodes have just rebooted; untouched baseline nodes may have run for weeks (different cache state, conntrack tables, neighbour entries). Reboot the baseline group as well, with an unchanged kernel, so that the groups differ in kernel and not in uptime. Uptime is part of the node context; `report` warns on a large uptime difference between the groups.
- The reverse direction — baseline node B → test-kernel node A and baseline node B → baseline node Y — isolates the receiving side. Offer it as its own scenario, since a kernel change affects both ends.

The two sides are then two **node groups within one window**, not two windows. This design brings its own confound, **node-to-node differences**: the cluster may mix NIC generations (see the context file), and cloud host placement differs. Therefore:

- `report` checks the node context and refuses the comparison (or requires an explicit override that is printed) unless both groups match in instance type, NIC model, NIC driver family and firmware.
- Several nodes per group, not one, and the run files must show the two groups' runs overlapping in time.
- Where possible, a **crossover**: afterwards the test kernel moves to the other group and the measurement is repeated, which separates the kernel effect from the node effect. `report` should accept both halves and say whether a crossover is present.
- Sequential A-B-A runs remain useful as a control, but are no longer the foundation.

In Phase 3 each side may instead be a Prometheus window, selected by matching `netmeasure_run_info` and resolving the label match to the time ranges where that series existed. If a phase label existed in two disjoint intervals, either union them or report the ambiguity — never silently take the first. Explicit time ranges must remain available as an alternative.

### Statistics — get this right, it is the point of the tool

- Mann-Whitney U and Kolmogorov-Smirnov are both wrong for a p99 claim: KS is least sensitive in the tails, Mann-Whitney tests the bulk. The correct quantity is a confidence interval on the **difference** of the quantile between the two sides.
- **Samples are not independent.** A network event affects consecutive samples. An ordinary bootstrap that resamples individual samples produces intervals that are too narrow — the worst possible error in front of a support case. Use a **block bootstrap** over measurement order, with blocks of contiguous samples of configurable length (justify the default; a whole cycle may be too long when a run has few cycles).
- **The run is the unit of replication.** A kernel change implies a reboot, new pod placement and different neighbours; within-run resampling cannot see run-to-run variance. With several runs, resample hierarchically (runs, then blocks within runs) and show between-run spread. In the concurrent design, the node is a further level of replication. Three runs per side are still very few for estimating between-run variance, so the output labels the **strength of evidence**:

  | Data | Label |
  |---|---|
  | 1 run per side | `exploratory` — run-to-run uncertainty not estimated, interval optimistic |
  | 2–4 runs per side | `preliminary` |
  | concurrent node groups without crossover, any number of runs | at most `preliminary` — matched, not causal |
  | ≥ 5 runs per side, interleaved/alternating, or concurrent node groups with several nodes per group, both groups rebooted, and crossover | `strong` |

  Propose and justify the exact thresholds.
- **Four verdicts, not three.** A confidence interval that covers zero does not mean "no change"; it means no change was demonstrated. Per compared quantity the verdict is one of:
  - `better` / `worse` — the **95 % two-sided** interval for the difference excludes zero;
  - `equivalent` — equivalence shown against a margin declared **before** the measurement, using two one-sided tests (TOST) at α = 0.05 each, i.e. the **90 %** interval lying within ±margin;
  - `inconclusive` — neither.
  Precedence: if the difference is significant **and** the 90 % interval lies within the margin, the verdict is `equivalent`, annotated "statistically significant, practically irrelevant". Without a declared margin, `equivalent` is never reported. "No measurable change" in a support case requires `equivalent`, not `inconclusive`. The report prints both intervals with their levels.
- **Equivalence margin.** `--equivalence-margin` (absolute or relative) requires `--equivalence-rationale "<text>"`: why a change of this size is irrelevant **for the application** (its request latency), not for statistical convenience. Both are printed in the report and stored in its JSON. The reference value is the before side in the sequential design and the reference (baseline) group in the concurrent design.
- **Effect size always.** Every comparison prints the absolute and relative change of each quantile, regardless of the verdict, so that a statistically significant but tiny change is visible as such.
- **Per side**, also give the exact distribution-free confidence interval for each quantile from order statistics (binomial). It needs no resampling.
- **Tail size.** Bootstrap intervals for extreme quantiles are unreliable with few observations beyond the quantile. Require a configurable minimum count above the quantile (propose and justify a default, expected in the range of 30–50); below it, print a warning and the required sample count instead of an interval. The default sample count must satisfy this minimum for p99; if your proposed minimum needs more than 5000, say so.
- **Censoring.** Timeouts enter quantiles as ≥ timeout. Report timeout counts and SYN-retransmission counts separately per side.
- **From Prometheus buckets** (Phase 3) there are no individual observations, so bootstrap is impossible. Use the published asymptotic method for quantile confidence intervals from grouped data, which extends to two-group differences (arXiv 1712.02476); from native histograms the same applies with finer bins. The independence caveat above applies here too and must be stated. Report the fraction of observations in the `+Inf` bucket; if the quantile falls into it, say so instead of printing a number.
- For counter rates such as retransmissions per million segments or SYN retransmissions per connect, use an exact conditional Poisson rate-ratio interval for the comparison and a Wilson interval per side. Note in the output that burst overdispersion makes Poisson intervals anti-conservative.
- In the Prometheus path the sample size is the `_count` delta, **not** the number of points returned by a range query. A two-hour range at 15 s step is 480 points but may represent millions of observations or a handful.
- Apply a multiple-comparison correction when the report checks many metrics or pairs, and state in the output which correction was used.

Output as a text table in the style of `benchstat` — medians, absolute and relative change, 95 % and (with a margin) 90 % interval, the verdict (`better` / `worse` / `equivalent` / `inconclusive`) and the evidence label — and as JSON. State which method produced each interval and which comparison design was used. Diagnostics (SYN retransmissions, NIC and TCP counters, listener counters) follow in a separate section, clearly marked as context, not as verdict.

### Comparability

**Print the run metadata and the node context of both sides in the report.** A verdict that does not state which kernel and which ring size each side ran on is not usable in a support case.

A difference between the sides is either **expected** — declared with `--expect-change` (e.g. `kernel`, `nic-ring`, `nic-channels`, `nic-driver`, `load`) — or **unexpected**. `--expect-change` only suppresses the **warning**, never the display: every difference in context between the sides is listed, expected ones without a warning marker. If the kernel is declared as changing and a z-stream update came along with it, that must remain visible. Warn on unexpected differences: instance type, a large uptime difference, CPU quota, target set, sample count, warm-up, interval, timeout, backlog, load parameters, tool version, MTU, NIC settings not declared as changed. Never pool runs with different load parameters on one side. Also warn on any invalid cycle, on a window or run shorter than the configured minimum, and when the loopback control differs significantly between the sides.

---

## Prometheus metrics (Phase 3)

Target is OpenShift User Workload Monitoring: `/metrics`, a Service, and a ServiceMonitor in the chart.

    netmeasure_connect_duration_seconds          histogram  labels: target_node, path
    netmeasure_connect_total                     counter    labels: target_node, path, result
                                                            result = ok|timeout|refused|error
    netmeasure_connect_syn_retrans_total         counter    labels: target_node, path
    netmeasure_cycle_valid                       gauge      labels: path
    netmeasure_cycle_invalid_total               counter    labels: path, reason
    netmeasure_listener_drops_total              counter    (own listening socket, SK_MEMINFO_DROPS)
    netmeasure_self_cpu_throttled_seconds_total  counter
    netmeasure_self_cpu_quota_cores              gauge
    netmeasure_run_info                          gauge = 1  labels: free-form, see above
    netmeasure_build_info                        gauge = 1  labels: version, toolchain, git_sha
    netmeasure_node_info                         gauge = 1  labels: node, zone, kernel,
                                                            nic_driver, nic_driver_version

The loopback control appears as `target_node="loopback"`.

Underlay mode additionally:

    netmeasure_nic_ring_size                     gauge      labels: dev, dir
    netmeasure_nic_channels                      gauge      labels: dev, kind
    netmeasure_ethtool_stat                      counter    labels: dev, stat
    netmeasure_netstat                           counter    labels: proto, key

`netmeasure_ethtool_stat` takes a configurable allowlist. The committed default is driver-neutral (e.g. every `*_dropped` and `*_err*` stat); the driver-specific allowlist for our cluster is an example file under `local/` (see the context file). `netmeasure_netstat` defaults to `TCPTimeouts`, `TCPLostRetransmit`, `TCPSynRetrans`, `TCPFastRetrans`, `ListenOverflows`, `ListenDrops`, plus `Tcp.RetransSegs` and `Tcp.OutSegs` from `/proc/net/snmp`.

Note for `prometheus-client` 0.26.0: registering a `Counter` whose name ends in `_total` panics under debug assertions, because the suffix is appended automatically.

### Histogram design, and this one has a trap

If the p99 falls in the highest bucket, `histogram_quantile()` returns the upper bound of the **second**-highest bucket — a constant. Two windows then look identical purely from bucket saturation. The top boundary must sit safely above the worst value of interest: the worst value observed so far is in the tens of ms (see the context file), and SYN retransmissions produce ≈1 s and ≈3 s, so size for the configured connect timeout and say so in the README.

Range to cover: 25 µs to the connect timeout. For 25 µs to ~1.6 s (a ratio of 64,000), classic histograms need roughly 28 buckets at factor 1.5 (25 µs · 1.5²⁷ ≈ 1.4 s; 18 buckets would end at ~25 ms, which is exactly the saturation trap above); extend accordingly for the chosen timeout. Native histograms cover it in one series at schema 4 (bucket growth factor 2^(1/16) ≈ 1.044, so about 4.4 % bucket width), at the cost of up to ~256 sparse buckets.

**Emit both.** Native histograms are stable since Prometheus 3.8.0, but from 3.9 the old feature flag is a no-op and `scrape_native_histograms` must be set explicitly, and protobuf must win content negotiation. The OCP 4.22 documentation does **not** state the shipped Prometheus version (its version matrix ends at 4.20 / Prometheus 3.5.0); read it from the cluster (`oc -n openshift-user-workload-monitoring exec prometheus-user-workload-0 -c prometheus -- prometheus --version`) and record it in the README. `ScrapeConfig` is not a supported OpenShift API, but it is not needed: the 4.22 `ServiceMonitor` API reference documents `scrapeNativeHistograms` (Prometheus ≥ 3.8), `scrapeProtocols`, `scrapeClassicHistograms` and `convertClassicHistogramsToNHCB`, so the per-job switch lives in the chart's ServiceMonitor. Whether UWM's Prometheus honours these fields must be verified on the cluster. The 4.22 release notes do not mention native histograms. Treat classic as the guaranteed path. The difference is material: a 100-node full mesh (9,900 ordered pairs, one path) is roughly 9,900 × (28 buckets + `+Inf` + `_sum` + `_count`) ≈ 307,000 active series classic against 9,900 native.

UWM can enforce `enforcedSampleLimit` and `enforcedLabelLimit` (`user-workload-monitoring-config`); exceeding them fails the **entire** scrape. Document the expected per-target sample count so administrators can size those limits.

**Cardinality**: node-pair labels grow as n². Provide a config switch to aggregate by zone-pair instead, and put the series-count formula in the README.

Assume Prometheus exemplars are unavailable by default. The only documented way to enable exemplar storage in UWM is `prometheusUserWorkload.remoteWrite[].sendExemplars: true` (4.22 config map reference, `RemoteWriteSpec`, "only applies to user-defined monitoring"), which requires a remote-write target. Do not depend on it; correlate logs to metrics by matching label values instead.

### Endpoint handling for `report`

Both a plain Prometheus and the OpenShift Thanos Querier. Thanos uses bearer-token authentication and enforces tenancy **by port** (verified against the 4.22 documentation, `modules/monitoring-resources-reference-for-the-cluster-monitoring-operator.adoc`):

- **9091** — all Thanos Querier endpoints, cluster-wide. Requires `cluster-monitoring-view` or `cluster-monitoring-metrics-api`. This is the port behind the `thanos-querier` Route (`oc -n openshift-monitoring get route thanos-querier`), which exposes `/api` only.
- **9092** — only `/api/v1/query`, `/api/v1/query_range`, `/api/v1/labels`, `/api/v1/label/*/values` and `/api/v1/series`, restricted to one project. Requires the `view` role in that project and a mandatory `namespace=` query parameter. `report` must work within this endpoint set, since a namespace-scoped ServiceAccount can only use this port.
- **9093** — `/api/v1/alerts` and `/api/v1/rules`, project-restricted. Not needed by `report`.

In-cluster, default the token to `/var/run/secrets/kubernetes.io/serviceaccount/token`; otherwise `--token-file`. Support `--ca-file` and `--insecure`, since the Route carries the ingress certificate.

**Retention**: UWM Prometheus retains data for **24 h by default** (`prometheusUserWorkload.retention`). This is one reason the vendor verification runs on result files. In the Prometheus path, `report` must detect when a requested window lies outside the available data and say so explicitly rather than reporting on a partial window.

---

## Logging for Loki

JSON lines on stdout, flat field names so LogQL's `json` parser works. Applies from Phase 1; Loki itself is Phase 3.

- **Do not log individual probes.** At these rates that is thousands of lines per minute. Individual samples belong in the result file.
- One INFO line per cycle per path: sample count, p50, p90, p99, p99.9, max, error and timeout counts, SYN retransmissions, validity and reasons, plus all run metadata.
- One WARN line per sample above a configurable outlier threshold (default 20 ms), with full context.
- WARN on an invalid cycle with its reasons, ERROR on a target unreachable for an entire cycle.
- A `cycle_id` (ULID) on every line of a cycle, so a summary and its outliers group together.

**Label names and values in the log fields must be identical to the Prometheus label names and values.** Put a mapping table in the README so a spike seen in a graph can be turned into a LogQL query by copying label values across.

Use `tracing` with `tracing-subscriber`'s JSON formatter, `flatten_event(true)` and `with_span_list(false)` — that is the flat-key recipe.

---

## Deliverables

Phase 1:

- Clear module separation: measurement and statistics free of I/O, so they can be unit-tested; measurement and diagnostics in separate modules
- `listen`, `run` (including background load), `report` (result files), result file schema documented
- Shutdown behaviour tested: SIGTERM mid-cycle within and beyond the deadline, a second signal, and SIGKILL during file writing (no truncated file that parses as complete)
- Unit tests for the statistics against known inputs, including autocorrelated input (block vs. naive bootstrap), censored timeouts, a tail below the minimum count, all four verdicts (in particular `inconclusive` vs `equivalent`), node-group mismatch rejection, and bucket saturation for the later Prometheus path
- `README.md` with: what is measured in terms of the TCP state machine, the loopback control, SYN-retransmission classification, sample counts per quantile and their wall-time cost, the statistical methods and their assumptions, the four verdicts and how to choose an equivalence margin, the two comparison designs (with a worked MachineConfigPool example for the concurrent one), pacing, why there is no `--parallel` and how to use background load, and how to run the static binary ad hoc inside an arbitrary pod via `oc cp` and `oc exec`

Phase 2:

- `chart/` Helm chart: Job template for `run` with a PVC for result files, DaemonSet in both variants for `listen`/`serve`, Service, RBAC, `podLabels` and `podAnnotations` passthrough, readiness probes on the control port (never the measured port), `terminationGracePeriodSeconds` derived from `--shutdown-deadline`
- Security contexts shaped for OpenShift `restricted-v2` by default — drop ALL, `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `runAsNonRoot`, seccomp `RuntimeDefault`. goldpinger's chart (`charts/goldpinger/values.yaml`) is the closest model in this space, but it is **not** admissible under `restricted-v2` as shipped: it hard-codes `runAsUser: 1000` and `fsGroup: 2000`, while `restricted-v2` uses `MustRunAsRange` / `MustRunAs` with the namespace's allocated range. Copy its container security context, and leave `runAsUser`, `runAsGroup` and `fsGroup` unset by default so the SCC assigns them; the image must run as an arbitrary UID.
- The underlay variant needs `hostNetwork` and therefore the **`hostnetwork-v2`** SCC (like `hostnetwork`, but drops ALL capabilities, defaults seccomp to `runtime/default` and forbids privilege escalation, so the same security context works unchanged). Grant it in the chart via a Role with verb `use` on `securitycontextconstraints/hostnetwork-v2`, and document the manual equivalent: `oc adm policy add-scc-to-user hostnetwork-v2 -z <serviceaccount> -n <namespace>`. Never deploy into an `openshift-*` or `kube-*` namespace: ServiceMonitors there are explicitly unsupported.
- RBAC: `discovery.k8s.io/endpointslices` and core `nodes`, get/list/watch
- README additions: mesh cost, chart usage

Phase 3:

- `serve`, `/metrics`, ServiceMonitor, `report` on Prometheus windows
- A Grafana dashboard JSON in the chart. The 4.22 documentation describes no supported mechanism for custom OpenShift console dashboards, so do not depend on one; if you propose a console dashboard anyway, label it unsupported
- README additions: label-to-log-field mapping table, ready-made PromQL, series-count formula, bucket rationale, retention and UWM limits

---

## Non-goals for the first version

- No HTTP or TLS probing. TCP connect only. The application-level token endpoint comes later and needs a load generator.
- No OTLP, no tracing backend, no alerting rules.
- No privileged container. `hostNetwork` for the underlay variant is acceptable; `CAP_NET_RAW` is not needed and must not be requested.

---

## Verified dependency versions (as of 2026-11-13)

`prometheus-client` 0.26.0 · `kube` 5.1.0 with `k8s-openapi` 0.29.0 (Kubernetes v1.37, MSRV 1.95) · `socket2` 0.6.5 · `tracing` 0.1.44 · `tracing-subscriber` 0.3.23

`kube` defaults to rustls, so a fully static `x86_64-unknown-linux-musl` build is on the supported path; `clux/muslrust` CI-tests that combination daily. `std::time::Instant` uses `CLOCK_MONOTONIC` on Linux and is sufficient — `quanta` buys mockability, not accuracy.

---

## Before you write code

Deliver the Step 0 evaluation first and wait. Then propose the dependency list and the module layout for Phase 1, including the result file schema, the control port protocol, the background-load interface, the pacing default and cycle length, the exit codes and shutdown deadline, the statistical methods with their defaults (block length, tail minimum, evidence thresholds) and the report model for both comparison designs, and wait again. Ask about anything underspecified rather than guessing.
