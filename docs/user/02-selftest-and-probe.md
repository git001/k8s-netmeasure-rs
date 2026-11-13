# `selftest` and `probe`

[User guide](README.md) · previous: [Concepts](01-concepts.md) · next: [`listen`](03-listen.md)

## `selftest`: is this kernel suitable?

```sh
k8s-netmeasure selftest [--output selftest.json] [--node-context-from <node-ip>:17701] [--log-level debug]
```

Runs about 10 s, needs no privileges and touches only its own loopback listeners.
Run it once per node type before the first measurement, in the same kind of pod as
the measurement (overlay and/or `hostNetwork`), and keep the report with the results.

| Check | What it tells you |
|---|---|
| `syn-retrans-field` | the kernel counts SYN retransmissions in `TCP_INFO`; otherwise `run` records them as unknown |
| `listener-drop-indicator` | a listener's dropped SYNs are visible (`SO_MEMINFO`); otherwise `run` falls back to namespace counters |
| `syn-backoff` | when the first SYN retransmission happens (about 1 s) |
| `tcpi-total-rto` | whether the kernel provides this newer field (absent on older kernels; not a failure) |
| `loopback-connect` | 1000 local connects: latency, `anomaly` count (must be 0), `peer_closed` |
| `pacing` | how late the paced loop wakes up (p99 should be far below 1 ms) |
| `signal-isolation` | a signal to the process never interrupts a measurement |
| `cgroup-v2` | the CPU quota and throttling counters are readable |
| `node-listener` | only with `--node-context-from`: the node listener is reachable, on this node (same boot id) and sees the NIC path |

The output is a table on stdout, one JSON log line per check and, with `--output`, a
JSON report including the node context (kernel, CPU, boot id, physical interfaces
with driver and ring sizes when visible).

Exit 0: every check established its result (a missing kernel feature is a finding,
not a failure). Exit 6: at least one check was inconclusive.

`--output` needs a writable directory; in a pod with a random UID and a read-only
root file system that is typically `/tmp` or a mounted volume.

## `probe`: a quick cross-check

```sh
k8s-netmeasure probe --target 10.0.0.5:17700 [--count 5] [--timeout 5s]
```

A few connects (1–100) with exactly the measurement code path, one JSON line each on
stdout:

```json
{"duration_ns":231204,"eintr":0,"errno":null,"error":null,"outcome":"ok","peer_closed":false,"seq":0,"syn_retrans":0,"sync_connect":false,"target":"10.0.0.5:17700","total_rto":"unknown"}
```

Use it to check that a target is reachable and behaves before a `run`. It is not a
measurement: no pacing, no warm-up, no cycles, no validity checks, and `report` does
not accept its output. Exit 0 if every connect was `ok`, 7 otherwise.

`probe` also works against any TCP listener, e.g. a node's kubelet on 10250.

Without the tool, the shell equivalent (no timing) is
`exec 3<>/dev/tcp/10.0.0.5/17700 && exec 3<&-`.
