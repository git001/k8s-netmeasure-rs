# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `report` (first step of milestone 5): compares `--before` and `--after` result
  files in the sequential design with the factor `kernel`. Per path one verdict on
  the declared primary quantile (default p99) with an equivalence margin and its
  rationale, Bonferroni over the paths, source nodes as the top bootstrap level;
  descriptive quantiles, failure and SYN-retransmission rates, the loopback median,
  the between-run spread, the evidence label and the context of both sides. Refuses
  (exit 5) overlapping sides and a changed NIC family or CPU model on a node unless
  `--allow-context-mismatch`. User guide page `07-report.md`.

- `deploy/examples/mesh/mesh.sh`: every node measures every other node without
  hand-edited manifests. It deploys the listeners (with optional node labels and
  tolerations from the environment), finds the listener addresses, runs one Job per
  source node in parallel, overlay and underlay alternating in rounds, copies the
  result files out of an `emptyDir` and deletes the Jobs. `inventory` prints one
  line per node (kernel, NIC drivers with ring sizes); a Job pod still `Pending`
  after `PENDING_S` is given up with the scheduler's reason; run pods request
  `RUN_CPU_REQUEST` (250m) with a limit of 1 CPU.
- README: acknowledgement line.
- `cargo xtask semisynthetic <dir>` (design §11): the semi-synthetic check on real
  result files. Runs of every node are split at random into two pseudo sides, a known
  factor (×1.0, ×1.1, ×1.3) is imposed on one, and the coverage and verdicts of the
  full comparison are printed; below 90 % coverage it fails. The data is passed by
  path and never copied into the repository.
- `report`: the same run given twice on one side (one file in two archives) is a
  usage error (exit 2) instead of counting twice.
- `report`: a context field equal on both sides prints as `both:` instead of a lone
  `before:` line.
- Simulation smoke grid (design §11): 8 synthetic scenarios with a known effect,
  R = 200, checked against the smoke-band criteria, including a negative control
  that proves the naive bootstrap under-covers. New gate `cargo xtask sim`, part of
  `ci`. The `report` header now names it under "method validation".

- Statistics, milestone 4a (`netmeasure-core::stats`): per-target sample series from
  result files (valid cycles only, without warm-up, consecutive valid cycles as one
  segment; timeouts
  censored, failures and SYN retransmissions counted), the type-1 quantile with
  censoring (`≥ timeout`), the distribution-free order-statistic interval with its
  exact binomial coverage, and the tail minimum of 40 expected exceedances with the
  required sample count below it.
- Statistics, milestone 4b: the stationary block bootstrap within a run's segments
  (seeded `ChaCha8`, blocks never cross an excluded cycle), the Politis–White block
  length with the Patton–Politis–White correction on the exceedance indicator, the
  5-s floor and the `n/10` cap with their conflict flag, and the autocorrelation for
  transparency. New dependencies: `rand` 0.10.3 (already in the graph), `rand_chacha`
  0.10.0.
- Statistics, milestone 4c: sides with hierarchical replication (nodes → runs →
  blocks; one node in the sequential design), between-run spread, the comparison of
  two sides (difference or ratio), verdicts `better` / `worse` / `equivalent` (TOST)
  / `inconclusive` with the censoring rules, one primary quantile with a verdict and
  the others descriptive, Bonferroni levels over `k`, failure rates with their own
  verdict family and SYN retransmissions as diagnostics (both bootstrapped, verdict
  or interval only from 10 events, Clopper–Pearson descriptive below), and the
  cross-group correlation of the concurrent design as a pure computation.

### Changed

- `run --output` accepts an existing directory and then names the file
  `<path>-<node>-<UTC start>-<run_id>.json`; an existing file is never overwritten
  (exit 3). On a cluster a reused file name had silently replaced an earlier
  baseline. The example Jobs write to `/results/`.

- `listener_drops` invalidates a cycle only if the listener's `SK_MEMINFO_DROPS`
  **and** its namespace's `ListenOverflows`/`ListenDrops` rose. On a cluster every
  cycle was invalid although no SYN was lost: the client's RST reached the target's
  request socket before its ACK (an embryonic reset), which raises `sk_drops` only.
  Such drops are now the annotation `listener_drops_not_syn`. `/v1/snapshot` reports
  `netns_embryonic_rsts`, and the result file's target diagnostics its delta.
- Example manifests: `run` Jobs request 500m CPU with a limit of 1 (a full CPU is
  often not free as a request on busy nodes), `listen` requests 100m; notes on the
  storage class (zonal disks need `WaitForFirstConsumer`), on querying overlay
  listeners past their NetworkPolicy, on taking the current target pod IPs before
  each run (a one-liner in the user guide), and the results reader runs on workers
  only.

- A connect whose socket is already in `CLOSE_WAIT` when `TCP_INFO` is read (the
  target accepted and closed before the check) is `ok`, no longer `anomaly`: that
  state is reachable only after a completed handshake. It is counted as
  `peer_closed` (`probe` field, `selftest` detail). Seen as 1 anomaly in 1000 in a
  pod with a 1-CPU quota.
- The `selftest` loopback listener keeps the last 64 accepted connections open
  instead of closing each at once, so its FIN cannot reach the client before the
  check; `listen` will do the same (design §4.2).
- The binary uses the system allocator; mimalloc is removed. It brought no measurable
  benefit here and was the only C dependency, so the static musl build now needs no
  `musl-gcc` (CI no longer installs `musl-tools`).
- Design: the per-cycle NIC path check also compares RX/TX ring sizes; a change
  invalidates the cycle as `nic_changed`.
- Design: the NIC family is treated as a property of the host, not of the instance
  type. The NIC path is checked around every cycle (`nic_changed`), a family change
  between crossover periods is a context mismatch, the node context gains the CPU
  model, and the report states when one instance type occurs with several families.

### Added

- Example manifests in `deploy/examples/` (milestone 3e2): `listen` as underlay
  (hostNetwork, OpenShift SCC `hostnetwork-v2`) and overlay DaemonSet with
  `--node-context-from $(HOST_IP):17701`, a NetworkPolicy for the overlay `listen`,
  overlay and underlay `run` Jobs writing to a volume, and a pod to fetch the result
  files; no Service, Route or Ingress. User guide section and diagram
  `deployment.dot` (overlay above underlay, measured and control paths).
- The container image contains `tar`, so `oc cp` / `kubectl cp` can copy result
  files out of a pod.
- `run --shutdown-deadline` (default 10s, design §8): after the first SIGTERM/SIGINT
  no new cycle starts; the running cycle is finished if its remaining slots fit into
  the deadline, else it ends at once as `interrupted`. Recorded as
  `config.shutdown_deadline_ns`.
- `run --control-port` / `--control-bind` (off by default): the same probes and `/v1/*`
  as `listen`, with the `run_id` as `listener_instance` and liveness bounds for the
  measuring and main threads; recorded as `config.control`. A set port that cannot
  be bound is exit 3.
- `--node-context-from <ip:port>` for `run`, `listen` and `selftest` (node listener,
  design §5): the NIC path of an underlay `listen` on the same node, proven by equal
  `boot_id`, stands in for the pod's own (`observed_via_node_listener`). Refused
  start (exit 3) if it is unreachable, on another node or sees no NIC. `run` reads
  it with every snapshot; an overlay `listen` refreshes it every second. New
  annotations `node_context_unavailable` and `node_listener_mismatch`; new
  `selftest` check `node-listener`.
- README section "Why this tool exists": the problem, the two questions the tool
  answers, how it differs from existing tools, and what it is not.
- User guide in `docs/user/`: concepts, `selftest` and `probe`, `listen`, `run` with
  the result file and exit codes, running in Kubernetes and OpenShift (pod types,
  permissions, security), troubleshooting. Seven Graphviz diagrams (`*.dot` sources
  with rendered SVG) for the overview, the workflow, the `run` lifecycle, the slot
  schedule, cycle validity, the `listen` lifecycle and the threads.
- `run` against `listen` targets: the control port is the target's IP and
  `--target-control-port` (default 17701); `/v1/info` and `/v1/context` at start
  (no answer: exit 3), `/v1/snapshot` before and after every cycle over one
  keep-alive connection per target (2 s timeout, one reconnect). New validity reason
  `target_set_changed` (other `listener_instance` or `boot_id`, or unreachable);
  target listener drops and target NIC path changes use the same rules as the
  source. `--no-target-control` measures plain TCP targets and annotates every cycle
  `target_unchecked`. Result file: per target control address, `listener_instance`
  and context before/after; `quality.listener_drops` per side; target counter deltas
  in `diagnostics`.
- `run`, second part: snapshots before and after every cycle (own and visible
  ancestor cgroup throttling, loopback listener `SK_MEMINFO_DROPS`, netns
  `ListenDrops`/`ListenOverflows`, NIC path) with the validity reasons
  `cpu_throttled`, `listener_drops`, `listener_drops_suspected`, `nic_changed` and the
  annotations `netns_listen_drops`, `nic_path_unknown`, `cpu_throttle_unchecked`.
  CPU checks at start: exit 3 for a quota below 500m or an unreadable cgroup v2
  unless `--no-cgroup-check`; `run.cgroup` records quota, floor, visible ancestors
  and `ancestor_cpu_throttling` (`observed`, `inferred_impossible`, `unknown`). Per
  cycle `quality.cpu`, `quality.listener_drops` and `diagnostics` with the netstat
  counters that increased.
  The design's `clock.synchronized` field is dropped (it would have needed an extra
  `unsafe` `adjtimex` call for a diagnostic hint only).
- `run` subcommand, first part: `--target` (repeatable), `--samples`, `--warmup`,
  `--interval`, `--loopback-every`, `--cycles` / `--cycle` (effective cycle length
  2–60 s), `--timeout`, `--outlier-threshold`, `--path` (else detected), `--label`,
  `--backlog`, `--output`, declared context. Paced measuring thread with one job per
  cycle, own loopback control listener, per-cycle validity (`sample_count_short`,
  `interrupted`, annotations `scheduling_jitter`, `pacing_overrun`), one INFO log
  line per cycle and target, WARN per outlier and invalid cycle. Result file
  (`k8s-netmeasure/result`, schema 1) with header, node context before and after,
  targets, cycles and compact sample columns, typed in
  `netmeasure-core::schema::result`; written atomically via
  `<output>.tmp-<run_id>`. Exit 0 / 4 / 143 / 130 (`interrupted` file).
- Security warning for `listen` (minimal protection only, never expose it to the
  internet) in the README, in `listen --help` and as a WARN log line at every start.
- `listen` control port: `GET /v1/context` (node context with provenance per field:
  `observed`, `declared`, `unknown`; kernel, boot id, uptime, CPU model and
  microcode, own `cpu.max`, TCP sysctls, MTU, NIC path) and `GET /v1/snapshot`
  (monotonic time, boot id, `listener_instance`, accept counters, listener
  `SK_MEMINFO_DROPS`, netns `ListenOverflows`/`ListenDrops`, own `cpu.stat`, NIC
  path). The bodies are typed in `netmeasure-core::schema::control` and read back
  with `deny_unknown_fields`. New declared-context flags `--node-name` (default
  `$NODE_NAME`), `--zone`, `--instance-type`.
- `listen` control port (HTTP/1.1 + JSON on its own thread, tokio `current_thread` +
  hyper): `GET /startup`, `/live` (watchdog over the accept and main threads, 5 s),
  `/ready` (503 from the first shutdown signal on) and `/v1/info` (build info,
  `listener_instance` ULID, measured address, backlog). Unknown path 404, other
  methods 405, any query 400. `--control-port` (default 17701, must differ from
  `--port`, exit 2) and `--control-bind` (default `auto`: the IPv4 address of the
  default-route interface; exit 3 if there is none). `/v1/context` and
  `/v1/snapshot` follow.
- `listen` subcommand, measured port only (control port follows): `--port` (default
  17700), `--bind` (default `0.0.0.0`), `--backlog` (default 4096, effective
  `min(backlog, somaxconn)`, logged). A dedicated accept thread keeps the last 64
  connections open. SIGTERM / SIGINT stop it with exit 143 / 130, a second signal
  exits at once; a port that cannot be bound is exit 3. On shutdown it logs accepted,
  aborted and failed accepts and the listener's `SK_MEMINFO_DROPS`.
- Node facts list every physical interface with its driver and RX/TX ring sizes
  (current and maximum, `ethtool -g` equivalent, no capability needed); a size the
  driver does not report is `null`. `selftest --output` includes them.
- `selftest`: the loopback check lists the TCP state of every `anomaly`
  (`anomaly_states`).
- Node facts include the CPU model and microcode revision of the first processor
  (`/proc/cpuinfo`); `selftest --output` includes them.
- `selftest --output`: `node` is now the node context of `/v1/context` (every field
  with `value` and `source`).
- `--log-level` (`K8S_NETMEASURE_LOG_LEVEL`) on every subcommand: `error`, `warn`,
  `info` (default), `debug`, `trace`; `debug` logs parameter sources and full
  `selftest` check data.
- Atomic result write: temporary file `<name>.tmp-<tag>`, `fsync`, `rename`, then
  `fsync` of the directory (design §5), so a completed file also survives a node crash.
- `probe` subcommand: a few connects (`--count`, 1–100) to `--target` with the exact
  measurement code path; one JSON line per connect on stdout with outcome, duration,
  errno, `sync_connect`, SYN retransmissions. Exit 0 if every connect is `ok`, 7
  otherwise.
- `selftest` subcommand: verifies on the running kernel the SYN retransmission field,
  the listener drop indicator (`SO_MEMINFO`), SYN backoff, `tcpi_total_rto`, loopback
  connect behaviour and anomaly rate, pacing lateness, signal isolation of the
  measuring thread, and cgroup v2 visibility. Summary table on stdout, one JSON log
  line per check, `--output` for a JSON report. Exit 0, or 6 if a check was
  inconclusive. The node-listener check follows with `listen`.
- Measurement core: non-blocking connect with `ppoll` on an absolute deadline
  (`EINTR` never extends the timeout), `SO_ERROR` and `TCP_INFO` read outside the
  timed section, `ok` only with `ESTABLISHED`, outcome classes `ok`, `timeout`,
  `refused`, `net_error`, `local_error`, `anomaly`; a measuring thread with all
  asynchronous signals blocked and 1 ns timer slack; the pacer with the four overrun
  rules; slot and cycle plan. `TCP_INFO` and `SO_MEMINFO` offsets come from `libc`
  (`offset_of!`); a field beyond the length the kernel filled is `unknown`, never 0.
  Signal mask, `sigaction`, `kill`, `ppoll`, clocks and timer slack go through the
  safe `nix` API; `unsafe` remains only for installing the signal handler and for
  `getsockopt` of partially filled buffers.
- Configuration rules: every parameter also as `K8S_NETMEASURE_<FLAG>`; unknown or
  empty `K8S_NETMEASURE_*` variables and the same parameter on the command line and
  in the environment are errors (exit 2); variables of other subcommands are ignored
  and logged.

- Phase 1 design in `docs/design/2026-11-13_phase1-design/`, after three external
  review rounds.
- Cargo workspace (Rust 1.100, edition 2024) with the crates `netmeasure-util`,
  `netmeasure-core`, `netmeasure-cli`, `k8s-netmeasure` and `xtask`.
- `k8s-netmeasure` CLI frame: `--help`, and `--version` with build information (git
  revision, rustc, target, static or dynamic linking). Usage errors exit with code 2.
  No measurement subcommands yet.
- `netmeasure-cli`: clap command definitions and the config-surface check (every
  parameter has a `K8S_NETMEASURE_<FLAG>` env var).
- Quality gates as `cargo xtask <gate>`: `fmt`, `clippy`, `line-limit` (file-size and
  argument-count rules), `test`, `deny` (including review dates of duplicate-version
  exceptions), `audit`, `coverage` (per-crate thresholds), `ci` (all of them) and
  `musl`.
- `deny.toml`: license allowlist, crates.io only, no duplicate versions.
- `Containerfile`: built in UBI 10 (glibc 2.39), runtime UBI 10 minimal, both pinned by
  digest; non-root user 1001, works with a read-only root filesystem.
- GitHub Actions, pinned by commit SHA: quality gates and the musl build on every
  push and pull request; image build, Trivy scan, push to
  `ghcr.io/git001/k8s-netmeasure-rs` and build provenance attestation; a daily audit;
  a weekly dependency freshness job; the static musl binary as a release asset on
  version tags. Dependabot for major crate versions and GitHub Actions.
- Licensed under MIT OR Apache-2.0.
- Release workflow: the GitHub release notes are the `CHANGELOG.md` section of the
  tagged version (`cargo xtask changelog <version>`); a tag without a changelog
  section fails the release.
- Image supply-chain evidence: keyless cosign signature, SPDX SBOM (syft) as cosign
  attestation, SLSA build provenance; the musl release binary gets an SPDX SBOM,
  an SBOM attestation and build provenance. Release notes include the verification
  commands.
- Binaries are built with `cargo auditable`, so SBOM generation and the image scan
  see the Rust dependencies.
