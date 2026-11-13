# Phase 1 design — 05 Result file

Part of the [Phase 1 design](README.md) · covers §5 · previous: [Listener and control port](04-listener-and-control-port.md) · next: [Load, process behaviour and configuration](06-runtime.md)

---

## 5. Result file schema (`schema_version: 1`)

One JSON document. The header stays human-readable (pretty-printed objects); numeric
sample columns are written as one compact array per line by a custom `serde_json`
formatter. Atomic write: `<path>.tmp-<run_id>` → `fsync` → `rename` → `fsync(dir)`.

```jsonc
{
  "schema": "k8s-netmeasure/result", "schema_version": 1,
  "run": {
    "run_id": "01J…", "status": "complete" | "interrupted" | "failed",
    "started_at": "2026-…Z", "ended_at": "…Z",          // UTC, ns precision
    "clock": { "realtime_at_origin": "…Z", "boot_id": "…", "synchronized": true },   // maps monotonic offsets to wall clock; synchronized from adjtimex() at start and end
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
      "flags":          [1, 0, …],          // bit0 sync_connect, bit1 warmup, bit2 after_overrun, bit3 eintr
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
boot_id, uptime, CPU quota, `somaxconn`, `tcp_syn_retries`, `tcp_syn_linear_timeouts`,
MTU, and per physical interface (anything with `/sys/class/net/*/device`): driver,
driver version, firmware, ring sizes, channels.

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
  http://<node-ip>:17701` and fetches the node listener's `/v1/context`. `run` does so
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
