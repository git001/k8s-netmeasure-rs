# Phase 1 design — 03 Validity, CPU checks, selftest and probe

Part of the [Phase 1 design](README.md) · covers §3.6–§3.9 · previous: [Measurement core](02-measurement-core.md) · next: [Listener and control port](04-listener-and-control-port.md)

---

### 3.6 Validity: proposed thresholds

| Reason | Rule | Invalidates? |
|---|---|---|
| `cpu_throttled` | `throttled_usec` delta > 0 | yes |
| `listener_drops` | own or target listener `SK_MEMINFO_DROPS` delta > 0 | yes |
| `listener_drops_suspected` | fallback, netns counters only | yes |
| `target_set_changed` | a target's control port reports a different `listener_instance` or `boot_id`, or a target became unreachable on the control port | yes |
| `sample_count_short` | any `local_error` or `anomaly` (§3.2), or a missing slot | yes |
| `nic_changed` | the **NIC path** of the source or of a target differs between before- and after-snapshot, or from the run's first cycle: the VF disappeared, appeared, is bound to another driver, or its ring sizes changed (added in milestone 2 planning, see below) | yes |
| `interrupted` | shutdown ended the cycle early | yes |
| `scheduling_jitter` | p99 of pacing wake-up lateness > **1 ms**, or max > **10 ms** | **annotate only** |
| `pacing_overrun` | overrun fraction > **5 %** | **annotate only** |
| `netns_listen_drops` | netns `ListenOverflows/ListenDrops` delta > 0 without own `sk_drops` | annotate only |
| `nic_path_unknown` | the NIC path before or after the cycle is `unknown` (pod network namespace without a node listener, §5); added in milestone 3c2 so that an unobserved NIC path is never read as unchanged | annotate only |
| `cpu_throttle_unchecked` | `--no-cgroup-check` was given (§3.7) | annotate only |
| `target_unchecked` | `--no-target-control`: the targets have no control port, so `target_set_changed`, target listener drops and the targets' NIC path are not checked (milestone 3d) | annotate only |

**NIC path** (added in milestone 2 planning). The same instance type can run on hosts
with different NIC hardware, and an accelerated-networking VF can be removed and
re-added during host maintenance; traffic then falls back to the slower synthetic
interface. Neither is visible from the instance type, and both change the measured
path without any change to the software under test. So the NIC path is checked
around **every cycle**, not only in the run's context:
- per physical interface: present or not, the bound driver (`readlink
  /sys/class/net/<if>/device/driver`) and the RX/TX ring sizes (`ETHTOOL_GRINGPARAM`,
  added in the milestone 2 review: a ring resize, e.g. by a tuning daemon, changes the
  path's queueing), plus which interface carries the default route; a few
  `readlink`/`stat` calls and one `ioctl` per interface, cheap enough for every cycle;
- observed directly in the underlay; in the overlay through the node listener
  (`--node-context-from`, §5), whose `/v1/snapshot` carries the same fields;
- if the node listener cannot be reached for a cycle, the NIC path of that cycle is
  `unknown` and annotated, as for the context (§5); it never counts as unchanged.

Rationale for "annotate only" on jitter and overrun: lateness affects the *start* of
a sample, not the timed connect itself, so it does not invalidate the measured
duration. It does, however, indicate CPU contention on the measuring node, which can
also slow the connect path. `report` warns whenever the fraction of annotated cycles
differs between the sides by more than **10 percentage points**. A 5 % overrun fraction typically means the interval
is too short for that path's tail; the warning names `--interval`.

### 3.7 CPU quota floor

`floor = 500m + 500m × load_threads` (default 500m without load). The main, measuring
and accept threads sleep almost all the time; 500m covers them with headroom. A load
thread is close to busy at high rates, so each one adds a full half-core.
`cpu.max = max` passes. **Unreadable cgroup v2 → refuse to start** (exit 3) unless
`--no-cgroup-check` is given; that flag is recorded, and every cycle then carries the
annotation `cpu_throttle_unchecked`.

README limits to name: throttling of an *ancestor* cgroup (the pod-level cgroup) is not
visible in the container's own `cpu.stat`. And when the binary is `oc exec`'d into an
application container, it **shares that container's CPU quota with the application**.
The throttle check catches the consequence, not the cause.

**Ancestor cgroups (review round 1).** At startup the tool resolves its cgroup path
from `/proc/self/cgroup` and tries to read `cpu.max` and `cpu.stat` of every ancestor
directory visible under `/sys/fs/cgroup`. Every visible level with a quota is checked
for throttling like the own cgroup. With the usual private cgroup namespace the
ancestors are not visible; then the result file says so explicitly, never as an
implied "no throttling". `report` lists the value per side.

**What can be inferred without seeing the ancestors** (review round 2). The kubelet
gives the pod-level cgroup a CPU quota only if **every** container of the pod has a CPU
limit, and then the quota is the sum of those limits over the same period. Two cases
follow:
- If every container stays within its own limit, the sum cannot be exceeded. Pod-level
  throttling without container-level throttling is then impossible.
- If any container lacks a limit, the pod cgroup has no quota at all.

This inference fails in two cases:
1. ephemeral containers, which have no limit of their own but run inside the
   pod-level quota (already capped at `preliminary`, §1);
2. pod-level resources, a newer Kubernetes feature where the pod limit can be smaller
   than the sum of the container limits.

The field therefore takes one of three values:

| `ancestor_cpu_throttling` | When |
|---|---|
| `observed: <delta>` | an ancestor with a quota is visible and was checked |
| `inferred_impossible` | not visible; own container has a limit; not ephemeral. Carries the note `unverified: pod-level resources not checked` in Phase 1 |
| `unknown` | not visible and the inference does not apply (ephemeral, no own limit) |

The inference is verified once on the target cluster in implementation step 2 (§14),
by reading the pod cgroup's `cpu.stat` from the node via a debug shell. Phase 2 checks
the pod spec for pod-level resources via the API and can then drop the `unverified`
note.

### 3.8 `selftest` subcommand (decided 2026-11-13)

**[addition]** A fifth subcommand, `k8s-netmeasure selftest`, is a regular,
fully documented subcommand like the others. It is listed in `--help` and the README
and follows every rule of §9.1: env vars, hard failures on unknown parameters,
`K8S_NETMEASURE_*`. Purpose: the prompt repeatedly demands "verify on the target
kernel"; `selftest` turns each of those verifications into a reproducible command whose
output can be attached to a support case.

Checks (all unprivileged; the SYN checks need only loopback in the pod's own netns):

| Check | Method | Result |
|---|---|---|
| SYN retransmission field | §3.3: fill the accept queue until a connect stalls, accept after ~0.5 s (before the first retransmission) | confirmed / unreliable (`run` then writes `unknown`) / **inconclusive** if no SYN drop could be provoked — per the three-way table in §3.3 |
| `tcpi_total_rto` present | `optlen` returned by `TCP_INFO` | present / absent (kernel feature, not a failure) |
| Listener drop indicator | same test: `SK_MEMINFO_DROPS` and `TcpExt.ListenDrops` before/after | own `sk_drops` +1 → `SO_MEMINFO` usable; otherwise `run` uses the `listener_drops_suspected` fallback |
| SYN backoff timing | duration of the retransmitted connect; `tcp_syn_retries`, `tcp_syn_linear_timeouts` sysctls | reported (≈1 s first retransmit; linear vs exponential where observable), informational |
| Loopback `sync_connect` | 1000 loopback connects | fraction completing inside `connect()`; evidence for the loopback-control statement in §3.1. Also the **`anomaly` rate** (§3.2): every anomaly would invalidate a whole cycle, so a non-zero rate here is known before the first `run` (review round 3); the TCP state of each anomaly and the `peer_closed` count (§3.1) are reported |
| Timer slack / pacing | the same 1000 loopback connects, paced at the default interval with the real pacer (§3.4); so the loopback, pacing and signal checks share one ~10 s loop | wake-up lateness p50/p99/max, overruns |
| cgroup v2 | read `cpu.max`, `cpu.stat`, visible ancestors (§3.7) | readable / not, quota value, ancestors visible or `unknown` |
| Signal isolation | send **SIGUSR2** (no-op handler installed for this check) to the process during a paced connect loop. Not SIGTERM: that would trigger the shutdown path of §8 and end `selftest` with exit 143 | no EINTR seen in the measuring thread |
| Node listener (with `--node-context-from`) | fetch `/v1/context`, compare `boot_id` | same node confirmed, or mismatch (§5). Arrives with `listen` in milestone 3; until then `selftest` has no such flag |

- Output: one JSON log line per check (as everywhere), plus a summary table on
  stdout at the end, and `--output <file>` for a JSON report with build info and node
  context.
- Exit codes: `0` all checks ran (feature absence such as a missing `tcpi_total_rto` is
  a finding, not a failure); `6` at least one check could not establish what it tests,
  e.g. no SYN drop could be provoked. Codes 2 and 3 as in §8.
- `selftest` takes about 10 s. It must not run against a production listener; it uses
  only its own loopback listeners.
- The same checks run as integration tests in CI, so a kernel or library change that
  breaks an assumption fails the build, not only the field run.
- A failed or inconclusive `selftest` does **not** block `run`. It tells the operator
  which fields `run` will record as `unknown` or via fallback.

### 3.9 `probe` subcommand (decided in review round 1)

**[addition]** A sixth subcommand, `k8s-netmeasure probe`, is documented like all
the others. It replaces the full libcurl in the image as the manual cross-check (§13).

```
k8s-netmeasure probe --target 10.0.0.5:17700 [--count 1] [--timeout 5s]
```

- Each connect uses exactly the §3.1 sequence (same code path as `run`). The output is
  one JSON line per connect: duration in ns, outcome, errno, `sync_connect`,
  `peer_closed` (§3.1), SYN retransmissions.
- `--count` default 1, maximum 100.
- No pacing, no cycles, no self-validation, no result file. It is a cross-check, not a
  measurement. The README says so, and `report` does not accept its output.
- Exit codes: `0` all connects `ok`; `7` at least one connect not `ok`; 2 and 3 as in
  §8.
- Without the tool, the README documents the shell equivalent (no timing):
  `exec 3<>/dev/tcp/<ip>/<port> && exec 3<&-`. `bash` is part of the image.
