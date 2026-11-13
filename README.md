# k8s-netmeasure-rs

Reproducible TCP connect-latency measurement in Kubernetes and OpenShift.

**Status: milestone 3 of the [Phase 1 implementation plan](docs/design/2026-11-13_phase1-design/11-implementation-plan.md)
in progress:** `probe`, `selftest`, `listen` and `run` (`--node-context-from` and the
shutdown deadline follow). `report` follows. The [user guide](docs/user/README.md) explains how to use the tool, with diagrams of
the flows. The design is in
[docs/design/2026-11-13_phase1-design/](docs/design/2026-11-13_phase1-design/README.md).

## Why this tool exists

Many cluster problems show up first as slow or failing **new TCP connections**
between nodes: timeouts in short-lived client connections, retries, a p99 that
drifts after a node-pool change. They are hard to pin down, because the effect sits
in the tail of the distribution, is noisy, and depends on things that are invisible
from Kubernetes: the NIC hardware of the host, its driver and ring sizes, the
kernel, CPU throttling of the measuring pod. A single `curl` loop or a dashboard
average cannot tell a real difference from noise or from a disturbed measurement.

The tool is built to answer two questions reproducibly, with evidence that holds up
in a support case:

1. **Do nodes with different NIC hardware behave differently?** The same instance
   type can run on hosts with different NIC families, and a virtual function can
   disappear and come back during host maintenance. The tool observes the NIC path
   on every node and around every measurement cycle instead of assuming it.
2. **Does a change make it better, worse or equivalent?** For example a test
   kernel, a driver update or a new node image, measured before and after with the
   same parameters and compared with confidence intervals (`report`, follows).

Existing tools answer different questions. k8s-netperf runs throughput and
request/response benchmarks and averages percentiles; goldpinger times HTTP round
trips in whole milliseconds and probes on the port it measures. Neither measures the
TCP handshake itself at the precision needed for a p99 comparison. The evaluation is
in [docs/design](docs/design/2026-11-13_k8s-netperf-step0-evaluation.md).

What `k8s-netmeasure` does differently:

- **measures `connect()` directly**, in nanoseconds with a monotonic clock, on a
  dedicated thread that no signal and no log call can disturb;
- **paces the connects** at a fixed interval, independent of the answers, with a
  **loopback control** in every run that separates local effects from network
  effects;
- **checks every cycle** for what would falsify it (CPU throttling, SYN drops at
  either listener, a restarted target, a changed NIC path) and marks it invalid
  instead of silently averaging it in;
- **records the context** (kernel, CPU, NIC driver and ring sizes, cgroup limits) of
  both sides with every result, so two result files can be checked for comparability;
- **verifies its own assumptions on the target kernel** (`selftest`) before the first
  measurement;
- needs **no API access and no privileges** for the measurement itself, and ships as
  a signed image and as a static binary for ad hoc use.

It is not a monitoring system (metrics follow in a later phase), not a throughput
benchmark, and it does not measure latency on established connections.

## Subcommands so far

> [!WARNING]
> **`listen` has only minimal protection. Never expose it to the internet.**
> Neither port has authentication, TLS, rate limiting or an allowlist. The measured
> port accepts every TCP connection, so anyone who can reach it can fill its accept
> queue and falsify measurements. The control port discloses node details (kernel,
> boot id, CPU, NIC drivers and ring sizes, cgroup limits) to anyone who can reach
> it. With `hostNetwork`, NetworkPolicy does not apply. Run it only inside the
> cluster network, only for the duration of a test series, and restrict access with
> NetworkPolicy (overlay) or node firewall / cloud security groups (underlay). Never
> create a `LoadBalancer` or `NodePort` Service, Route or Ingress for it.

```sh
# Is this kernel suitable? Run once per node type before measuring (about 10 s).
k8s-netmeasure selftest [--output selftest.json]

# Measurement target: accepts on the measured port until SIGTERM/SIGINT (exit 143/130).
k8s-netmeasure listen [--port 17700] [--bind 0.0.0.0] [--backlog 4096] \
                      [--control-port 17701] [--control-bind auto] \
                      [--node-name N] [--zone Z] [--instance-type T]

# The measurement: paced connects in cycles, one result file (JSON) at the end.
k8s-netmeasure run --target 10.0.0.5:17700 --output result.json \
                   [--samples 5000] [--warmup 50] [--interval 10ms] [--loopback-every 10] \
                   [--cycles 10 | --cycle 20s] [--timeout 5s] [--outlier-threshold 20ms] \
                   [--path overlay|underlay] [--label k=v ...] [--backlog 4096] \
                   [--no-cgroup-check] [--target-control-port 17701 | --no-target-control] \
                   [--node-name N] [--zone Z] [--instance-type T]

# Manual cross-check of one path, same code path as the measurement (not a measurement).
k8s-netmeasure probe --target 10.0.0.5:17700 [--count 5] [--timeout 5s]
```

Every parameter can also be set as `K8S_NETMEASURE_<FLAG>`, e.g.
`K8S_NETMEASURE_TARGET=10.0.0.5:17700`. Setting the same parameter both ways, an
unknown `K8S_NETMEASURE_*` variable, or an empty one is an error (exit 2).

Logs are JSON lines on stderr; results go to stdout. Every subcommand takes
`--log-level error|warn|info|debug|trace` (`K8S_NETMEASURE_LOG_LEVEL`, default `info`);
`debug` adds details such as the source of every parameter and the full data of each
`selftest` check.

`listen` accepts connections and never reads or writes on them. It keeps the last 64
accepted connections open instead of closing each at once, so its FIN cannot reach the
client before the client has checked the connection. The effective backlog is
`min(--backlog, net.core.somaxconn)` and is logged at start. A port that cannot be bound
ends the process with exit 3. On shutdown it logs the accepted, aborted and failed
accepts and the listener's SYN drops (`SK_MEMINFO_DROPS`).

The control port is plain HTTP/1.1 with JSON and is never measured:

| Path | Answer |
|---|---|
| `GET /startup` | 200 once both ports are bound, 503 before |
| `GET /live` | 200 unless the accept or main thread is stalled (> 5 s); never depends on the network |
| `GET /ready` | 200 while accepting; 503 before startup and from the first SIGTERM/SIGINT on |
| `GET /v1/info` | build info, `listener_instance` (ULID, new per start), measured address, backlog |
| `GET /v1/context` | node context; every field as `{"value": …, "source": "observed" \| "declared" \| "unknown"}`: kernel, boot id, uptime, CPU model and microcode, own `cpu.max`, TCP sysctls, MTU of the default-route interface, NIC path (physical interfaces with driver and ring sizes, default-route interface; `unknown` in a pod network namespace) |
| `GET /v1/snapshot` | monotonic time, boot id, `listener_instance`, accepted/aborted/failed accepts, listener SYN drops, netns `ListenOverflows`/`ListenDrops`, own `cpu.stat`, NIC path |

An unknown path is 404, any method other than `GET` 405, any query string 400.
`--control-bind auto` (default) binds the IPv4 address of the interface with the
default route: the pod IP in a pod, the node IP with `hostNetwork`. `127.0.0.1` is
then not served, so `oc port-forward` needs `--control-bind 0.0.0.0`. With
`hostNetwork`, NetworkPolicy does not apply and the control port is reachable for
anyone who can reach the node; it has no authentication. `--control-port` must
differ from `--port` (exit 2).

`--node-name` (default: the `NODE_NAME` environment variable, e.g. from the Downward
API), `--zone` and `--instance-type` are declared, not observed; without them the
fields are `unknown`. The NIC family is never derived from the instance type.

`run` measures every `--target` round-robin at `--interval` start-to-start spacing,
with every `--loopback-every`-th slot on its own loopback listener as a local control.
The first `--warmup` samples per target are stored but flagged and excluded from
statistics; `--samples` counts the rest per target. The run is cut into cycles (default
10, each kept between 2 s and 60 s). Around every cycle `run` reads its own cgroup
throttling (and that of every visible ancestor cgroup with a quota), the SYN drops of
its loopback listener, the network-namespace TCP counters and the NIC path, and
marks the cycle invalid for `cpu_throttled`, `listener_drops`,
`listener_drops_suspected`, `sample_count_short`, `nic_changed` or `interrupted`;
pacing jitter, overruns, foreign listener drops in the namespace and an unobserved
NIC path are annotations only. Each target is normally a `listen`: `run` reads its
control port (same IP, `--target-control-port`, default 17701) at start and around
every cycle, and also invalidates a cycle for `target_set_changed` (the target
restarted, rebooted or stopped answering), for SYN drops at the target's listener
and for a change of the target's NIC path. A target without a control port refuses
the start (exit 3); `--no-target-control` measures any TCP listener and annotates
every cycle `target_unchecked`. At start it refuses (exit 3) a CPU quota below 500m
and an unreadable cgroup v2; `--no-cgroup-check` measures anyway and annotates every
cycle `cpu_throttle_unchecked`. At the defaults one
target takes about 56 s. The result file is written atomically at the end (a
temporary `<output>.tmp-<run_id>` is renamed). Exit 0: every cycle valid; 4: at least
one invalid; 143/130 after SIGTERM/SIGINT, with the file marked `interrupted`; 3 if
the output directory does not exist. Labels (`--label k=v`, env comma-separated)
are normalised to Prometheus label names; reserved names such as `path` or `result`
are rejected.

`probe` measures TCP connection establishment only: one JSON line per connect with
the duration, the outcome, the errno, the SYN retransmissions and `peer_closed` (the
target had already closed the connection when it was checked; still `ok`). Without the tool, the
shell equivalent (no timing) is `exec 3<>/dev/tcp/<ip>/<port> && exec 3<&-`.

## Build and check

Toolchain: pinned by `rust-toolchain.toml` (Rust 1.100, edition 2024).

```sh
cargo build --release
cargo xtask ci        # every quality gate, exactly as CI runs them
cargo xtask <gate>    # one gate: fmt | clippy | line-limit | test | deny | audit | coverage
cargo xtask musl      # static musl build + tests (needs cargo-auditable)
cargo xtask changelog <version>   # release notes from CHANGELOG.md
```

Changes are recorded in [CHANGELOG.md](CHANGELOG.md) (Keep a Changelog format); a
release's notes are its changelog section.

`cargo xtask ci` needs `cargo-deny`, `cargo-audit` and `cargo-llvm-cov` installed.

## Container image

```sh
podman build -f Containerfile --build-arg BUILD_GIT_SHA=$(git rev-parse --short=12 HEAD) .
```

CI publishes `ghcr.io/git001/k8s-netmeasure-rs`, signed keylessly with cosign, with an
SPDX SBOM attestation and SLSA build provenance; each release's notes list the
verification commands.
Reference it by digest; for clusters that pull only from an internal registry, mirror
it by digest first (`skopeo copy --all` or `oc image mirror`), so the attestation
stays valid.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
