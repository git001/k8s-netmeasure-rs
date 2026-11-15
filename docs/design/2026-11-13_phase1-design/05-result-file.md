# Phase 1 design — 05 Result file

Part of the [Phase 1 design](README.md) · covers §5 · previous: [Listener and control port](04-listener-and-control-port.md) · next: [Load, process behaviour and configuration](06-runtime.md)

---

## 5. Result file schema (`schema_version: 1`)

One JSON document. The header stays human-readable (pretty-printed objects); numeric
sample columns are written as one compact array per line by a custom `serde_json`
formatter. Atomic write: `<path>.tmp-<run_id>` → `fsync` → `rename` → `fsync(dir)`.

**Never overwritten** (added 2026-11-15 after the first cluster runs, where a reused
file name silently replaced an earlier baseline): `--output` is either an existing
directory, in which `run` names the file `<path>-<node>-<UTC start
YYYYMMDDTHHMMSSZ>-<run_id>.json` (node name sanitised to `[A-Za-z0-9._-]`,
`unknown-node` if undeclared), or a file name that must not exist yet; an existing
file refuses the start (exit 3). The check runs at start, before anything is
measured.

```jsonc
{
  "schema": "k8s-netmeasure/result", "schema_version": 1,
  "run": {
    "run_id": "01J…", "status": "complete" | "interrupted" | "failed",
    "started_at": "2026-…Z", "ended_at": "…Z",          // UTC, ns precision
    "clock": { "realtime_at_origin": "…Z", "boot_id": "…" },   // maps monotonic offsets to wall clock
    "labels": { "phase": "baseline", "ticket": "CASE-0001" },
    "build_info": { "version": "…", "git_sha": "…", "rustc": "1.100.0", "target": "x86_64-unknown-linux-gnu", "glibc": "2.39", "static": false },
    "config": { "samples": 5000, "warmup": 50, "interval_ns": 10000000, "loopback_every": 10,
                "cycles": 10, "cycle_slots": [611, 555, 556, 555, 556, 555, 556, 555, 556, 556],
                "timeout_ns": 5000000000, "backlog": 4096, "listener": "127.0.0.1:41873", "control": null, "path": "overlay",
                "path_source": "declared" | "detected", "topology": "pair",
                "load": { "threads": 0, "rate_per_s": 0 }, "shutdown_deadline_ns": …, "no_cgroup_check": false },
    "config_source": { "samples": "default", "interval_ns": "env", "path": "cli", … },   // one entry per parameter (§9.1)
  },
  "source":  { "context_before": { … }, "context_after": { … } },
  "targets": [
    { "index": 0, "name": "loopback", "addr": "127.0.0.1:41873", "kind": "loopback" },   // run's own ephemeral loopback listener
    { "index": 1, "name": "target-a", "addr": "10.0.0.5:17700", "kind": "listener",
      "control": "10.0.0.5:17701", "context_before": { … }, "context_after": { … } }
  ],
  "measurement": {
    "cycles": [ { "cycle_id": "01J…", "index": 0, "first_sample": 0, "end_sample": 611,
                  "started_at": "…Z", "ended_at": "…Z",
                  "validity": { "valid": true, "reasons": [], "annotations": [] },
                  "quality": { "wake_lateness_ns": { "p50": …, "p90": …, "p99": …, "max": … },
                               "overruns": 0, "cpu": { "throttled_usec_delta": 0, "nr_throttled_delta": 0 },
                               "listener_drops": { "source": 0, "targets": { "1": 0 } } } } ],
    "samples": {
      "count": 5611,
      "result_codes": ["ok", "timeout", "refused", "net_error", "local_error", "anomaly"],
      "target":         [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, …],
      "late_ns":        [812, 1033, …],     // actual start minus scheduled start
      "duration_ns":    [31211, 4120334, …],
      "result":         [0, 0, …],
      "syn_retrans":    [0, 0, …],          // -1 = unknown
      "flags":          [1, 0, …],          // bit0 sync_connect, bit1 warmup, bit2 after_overrun, bit3 eintr, bit4 peer_closed
      "reanchor":       { "index": [], "sched_offset_ns": [] },   // sparse: only re-anchored slots
      "errno":          { "index": [], "value": [] }               // sparse
    }
  },
  "diagnostics": {
    "cycles": [ { "cycle_id": "01J…",
                  "source":  { "netstat": { "TcpExt.ListenDrops": 0, … }, "ethtool": { "nic0": { … } } },
                  "targets": { "1": { "netns_listen": { … }, "netstat": { … }, "ethtool": { … } } },
                  "load":    { "attempts": 0, "by_result": { "ok": 0, … }, "syn_retrans": 0 } } ]
  }
}
```

**Fields added in milestone 3c** (implementation detail, same schema version):
- every cycle carries `start_offset_ns`, the monotonic offset of its first scheduled
  slot from the run origin; slot `k` of the cycle is scheduled at
  `start_offset_ns + k × interval`, and `reanchor.sched_offset_ns` is relative to the
  cycle's start. Each cycle has its own pacing origin because the snapshots between
  cycles take time;
- `config.outlier_threshold_ns` (`--outlier-threshold`, §9.3);
- `config.path_source` is `declared` with `--path`, otherwise `detected`: `underlay`
  if a physical interface is visible in the network namespace, else `overlay`.
- milestone 3c2: `run.cgroup` (own path, quota and floor in millicores, visible
  ancestors, `ancestor_cpu_throttling` and its note, §3.7), `config.no_cgroup_check`,
  per cycle `quality.cpu` (`throttled_usec_delta`, `ancestors_throttled_usec_delta`)
  and `quality.listener_drops` (own loopback listener), and
  `diagnostics.cycles[].source.netstat` with only the counters that increased.
- **`clock.synchronized` dropped** (decided in milestone 3c2): it would have needed
  `adjtimex()`, for which `nix` has no safe wrapper, i.e. a further `unsafe` call,
  for a diagnostic hint only. Durations never depend on the wall clock (monotonic
  only); `report` names a possible clock error without knowing the NTP state (§10.6).
- milestone 3d: per target `control`, `listener_instance`, `context_before` and
  `context_after`; `config.target_control_port`; per cycle
  `quality.listener_drops` as `{ "source": …, "targets": { "1": … } }` as in the
  example above; `diagnostics.cycles[].targets` with the deltas of `accepted`,
  `aborted`, `netns_listen_drops`, `netns_listen_overflows` and (added 2026-11-15)
  `netns_embryonic_rsts` per target. A target's
  `name` is its declared node name if it has one, else its address.
- milestone 6 (2026-11-15), NIC statistics: `diagnostics.cycles[].source.nic` and
  `targets[i].nic` hold the selected `ethtool -S` counters that increased during
  the cycle, per physical interface (`{ "<if>": { "stop_queue": 3 } }`). Selected
  are counters whose name contains `drop`, `discard`, `err`, `miss`, `over`,
  `timeout`, `full`, `busy`, `stop` or `wake` (driver-neutral, no vendor list);
  `u64::MAX` (a driver's "not available") is left out. `{}` means read, nothing
  increased; an absent field means not read (an older version, a pod network
  namespace without node listener, or no physical interface). Overlay runs read them
  from the node listener. `/v1/snapshot` carries the current values as `nic_stats`.
  They never invalidate a cycle; the NIC *path* (§3.6) is unchanged.
Fields of later steps (`control`, `load`, `shutdown_deadline_ns`) are added with them
(3e, milestone 6).

The file has three parts: **measurement** (samples plus the inputs to the validity
decision), **diagnostics** (context that can explain a change but never decides one),
and header/context. This is the separation the prompt requires.

**Scheduled start times are implicit** (review round 2). Without overruns, slot `k`
of a cycle is scheduled at `cycle_start + k × interval`, so it is not stored. Only
the re-anchored slots (§3.4 rule 2) are listed in the sparse `reanchor` column with
their scheduled offset. Every scheduled start can be reconstructed exactly from these.
An earlier version stored a dense `sched_delta_ns` column that was almost always
constant and cost about a third of the file.

Size: each sample costs about 20 bytes of JSON (`late_ns`, `duration_ns` and the small
integer columns), so 100,000 samples ≈ 2 MB. That is within the single-digit-MB target
without compression. **Decided 2026-11-13: no compression**; result files are plain
JSON.

**Node context** fields: node name, zone, instance type, kernel (`uname -r`, `-v`),
boot_id, uptime, CPU quota, **CPU model** (`/proc/cpuinfo` `model name`, plus
`microcode`; added in milestone 2 planning: hosts behind the same instance type can
differ in CPU as well as NIC; in the node facts and `selftest --output` since the
first cluster `selftest`, which showed loopback latency differing by 4× between
nodes), `somaxconn`, `tcp_syn_retries`,
`tcp_syn_linear_timeouts`, MTU, and per physical interface (anything with
`/sys/class/net/*/device`): driver, driver version, firmware, ring sizes, channels.
**The NIC family is always observed, never derived from the instance type** (§10.7).
**Ring sizes** (current and maximum, RX and TX, via `SIOCETHTOOL`/`ETHTOOL_GRINGPARAM`,
no capability needed) are in the node facts since the milestone 2 review: hosts
behind the same instance type were seen with different TX ring sizes per NIC family.
A direction whose maximum the driver reports as 0 is `null` (`ethtool -g`: `n/a`). In a
pod network namespace no physical interface is visible; the underlay view comes from
a `hostNetwork` process (§5, node listener).

Every field carries a provenance:
- `observed` — read by this process;
- `observed_via_node_listener` — read by an underlay `listen` on the same node, see
  below;
- `declared` — from a CLI flag;
- `unknown`.

In Phase 1 there is no API access, so zone and instance type are only `declared`
(`--zone`, `--instance-type`, `--node-name`, default `$NODE_NAME`) or `unknown`; Phase
2 reads them from the Node object.

**Node context in the overlay variant (decided 2026-11-13).** A pod's netns contains no
physical NIC, so an overlay process cannot observe any NIC field. Without them, `report`
must refuse the concurrent design (§10.6). The solution is a **node listener**: an
underlay `listen` (`hostNetwork`) on the same node, which observes the NIC.

- **Fetching:** an overlay `run` or `listen` takes `--node-context-from
  <node-ip>:17701` (an `IPv4:port` like every other address of the tool; the URL
  form of earlier drafts was dropped in milestone 3d) and fetches the node listener's
  `/v1/context`. At start the node listener must also see a NIC path, i.e. run with
  `hostNetwork`; otherwise exit 3. An overlay `listen` refreshes the node's NIC path
  every second on its main thread and reports the cached value, so its control
  thread never waits for the network. `run` does so
  before and after every cycle, like every other snapshot. An overlay `listen` includes
  the fetched NIC fields in its own `/v1/context`, so a target's NIC context reaches the
  source the same way.
- **Proof that it is the same node:** `/proc/sys/kernel/random/boot_id` belongs to the
  kernel, not to a network namespace, so a pod and a hostNetwork pod on the same node
  read the same value. At startup the node listener's `boot_id` must equal the
  process's own; otherwise the process refuses to start (exit 3). A later mismatch
  stops the fetching, and from then on the NIC fields are `unknown`, annotated
  `node_listener_mismatch`. That boot_id is shared across network namespaces must be
  confirmed once on the target cluster; it is part of `selftest` (§3.8).
- **Unreachable node listener for one cycle:** the NIC fields of that cycle are
  `unknown`, annotated `node_context_unavailable`. Context never invalidates a cycle;
  it only explains.
- **There is no hand-written context file.** NIC data is either observed (directly or
  via the node listener) or `unknown`; it is never typed in.
- Running the node listener needs the `hostnetwork-v2` SCC. In Phase 1 that is the
  example manifest in `deploy/examples/` (§13); in Phase 2 it is the chart's underlay
  DaemonSet, which provides a node listener on every node anyway.
