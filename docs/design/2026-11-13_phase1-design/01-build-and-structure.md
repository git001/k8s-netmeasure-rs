# Phase 1 design — 01 Build, dependencies and code structure

Part of the [Phase 1 design](README.md) · covers §1–§2 · next: [Measurement core](02-measurement-core.md)

---

## 1. Toolchain and dependencies

`rust-toolchain.toml` pins `1.100` with target `x86_64-unknown-linux-gnu`; edition
2024, stable only. **[deviation]** The prompt assumes a fully static
`x86_64-unknown-linux-musl` build. Decision: **the glibc target**, because the container
image will be UBI 10 minimal.

Consequences:

- **Build against the oldest glibc we must run on.** A binary linked on a newer host
  (for example a development machine with glibc 2.44) can require symbol versions that
  UBI 10 (glibc 2.39) does not provide. Release builds therefore run in a UBI 10
  builder container, not on the host.
- **Decided 2026-11-13: the image carries one build, dynamically linked against UBI
  10's glibc (2.39).**
- **Emergency artifact (review round 1):** CI additionally builds a static
  `x86_64-unknown-linux-musl` binary and publishes it as a release artifact, **not**
  as a distribution format and not in the image. It exists for one situation: the
  image cannot be pulled or mirrored in time, and the only way to measure is `oc cp`
  into an already running pod of any base image. It is built on the CI runner with
  `musl-tools` (mimalloc's C part needs `musl-gcc`) and runs the same test suite.
  Its `build_info` says `static: true`, so result files show which build produced
  them.
- **[deviation]** The prompt's Phase 1 path "copy a static binary with `oc cp` into any
  pod" becomes the emergency path above. The glibc binary from the image runs only in
  pods whose glibc is ≥ 2.39. **The primary ad hoc path is our own image**:
  - **Overlay, preferred:** our own pod on the same node as the application. It has
    its own resource limits, so the CPU self-validation works fully. The README names
    this as the default choice.
  - **Underlay:** a `hostNetwork` pod running the image (§13).
  - **Measuring from inside an existing application pod's netns** (same network path
    as the application): an ephemeral container with our image (`oc debug` / `kubectl
    debug --image=<image> --target=<container> <pod>`). An ephemeral container shares the
    pod's network namespace, so no `oc cp` is needed. It requires RBAC on
    `pods/ephemeralcontainers`.
  - **Caveat:** ephemeral containers cannot have resource limits. The tool's own
    `cpu.max` then reads `max`, and the CPU quota floor passes trivially, while
    throttling of the pod-level cgroup stays invisible (§3.7). The README says so, and
    the result file records `container_kind: ephemeral` when detected. **`report`
    caps the evidence of such runs at `preliminary`** and states why (§10.5).

Release profile:
`lto = "thin"`, `codegen-units = 1`, `panic = "unwind"` (the measuring thread's
panic must be caught so that the result file is still written).

Phase 1 dependencies — deliberately small. **No async runtime in the measurement
path.** tokio exists only on the control-port thread (§7) and in the main thread's
`block_on` calls between cycles in `run`.

**Version rule:** always the newest release of every crate, provided the whole graph
resolves without conflicts. Duplicate versions are forbidden for direct dependencies.
For transitive dependencies a duplicate is allowed only as an explicit, reasoned
exception (§12). Default features are switched off wherever they pull in something
unused.

The table was resolved on 2026-11-13 with `cargo add` (newest versions) in a scratch
project containing every crate below. Results:

- `cargo tree -d` shows **no duplicate versions**.
- The project builds with toolchain 1.100.0.
- `rand` 0.10 is shared by `rand_chacha`, `ulid` and `statrs`.
- `statrs` without default features still provides `Binomial`/`Beta`/`Normal` CDFs and
  quantiles; spot check: `Normal::inverse_cdf(0.975) = 1.959964`.
- `signal-hook` 0.4.6 still has `flag::register_conditional_shutdown`.

| Crate | Version | Features | Use | Note |
|---|---|---|---|---|
| `socket2` | 0.6.5 | — | socket creation, `SO_LINGER`, non-blocking connect | |
| `libc` | 0.2.191 | — | `getsockopt(TCP_INFO / SO_MEMINFO / SO_ERROR)`, `ppoll`, `clock_nanosleep`, `prctl(PR_SET_TIMERSLACK)`, `ioctl(SIOCETHTOOL)` | |
| `clap` | 4.6.7 | `derive`, `env` | CLI; every parameter also as `K8S_NETMEASURE_*` (§9.1) | |
| `serde` / `serde_json` | 1.0.229 / 1.0.154 | `derive` | result file, report JSON, control-port bodies | custom formatter, §5 |
| `tokio` | 1.54.1 | `rt`, `net`, `time` (no `rt-multi-thread`, no `signal`) | `current_thread` runtime for the control port | needed by `kube` in Phase 2 anyway |
| `hyper` / `hyper-util` / `http-body-util` | 1.12.0 / 0.1.21 / 0.1.5 | `server`, `client`, `http1` | HTTP/1.1 control-port server and client | `/metrics` joins in Phase 3 |
| `tracing` | 0.1.44 | `default-features = false`, `std` | logging facade | without defaults: no `tracing-attributes`, which would pull in a second `syn` major version |
| `tracing-subscriber` | 0.3.23 | `json` | JSON lines, `flatten_event(true)`, `with_span_list(false)` | |
| `signal-hook` | 0.4.6 | — | SIGTERM/SIGINT → `AtomicBool`; second signal → immediate exit | no extra thread |
| `ulid` | 3.0.0 | — | `run_id`, `cycle_id`, `listener_instance` | |
| `rand` / `rand_chacha` | 0.10.3 / 0.10.0 | — | seeded bootstrap RNG (seed in report) | core only |
| `statrs` | 0.19.1 | `default-features = false` | binomial, beta, normal quantiles | without defaults: no `nalgebra` (about 30 crates fewer) |
| `glob` | 0.3.4 | — | `report --before 'results/x/*.json'` | |
| `jiff` | 0.2.43 | `default-features = false`, `std`, `serde` | UTC `Timestamp` (ns, RFC 3339), `SignedDuration`, CLI durations | only via `netmeasure-util::time`; no tz database; never in the timed section |
| `thiserror` / `anyhow` | 2.0.21 / 1.0.104 | — | errors (core / binary) | |
| `mimalloc` | 0.1.52 | — | `#[global_allocator]`, binary only | C build: builder needs `gcc` |
| dev: `tempfile` / `assert_cmd` | 3.27.0 / 2.2.2 | — | integration and shutdown tests | |
| xtask: `syn` / `proc-macro2` | 3.0.10 / 1.0.107 | `full` / `span-locations` | line-limit check (§12) | not shipped |

Versions are pinned by `Cargo.lock`. Keeping them current is part of CI (§12):
- a weekly job runs `cargo update` and the full test suite;
- `cargo deny` treats a duplicate version of a direct dependency as an **error**, and a
  transitive duplicate as an error unless it is listed in `deny.toml` `skip` with a
  reason and a review date (decided in review round 1, 2026-11-13);
- GitHub Dependabot opens PRs for new major versions.

Everything else (cgroup, `/proc`, ethtool) is parsed/called directly, as the prompt
requires.

---

## 2. Crate and module layout

The two crates named in the prompt, plus **[addition]** a shared utility crate, a CLI
definition crate and an `xtask` tooling crate:

- **`netmeasure-core`** does **no I/O at all** (no sockets, no files, no clock reads, no
  logging) and is therefore fully unit-testable.
- **`k8s-netmeasure`** does all I/O and calls into core.
- **`netmeasure-util`** holds every helper that more than one module needs (rule below).
- **`netmeasure-cli`** holds the clap command definitions and the config-surface
  check (§9.1). Parsing argv and the environment happens in the binary. *Added in
  milestone 1:* clap's derive output allows the whole clippy `complexity` group, which
  is a hard compile error (E0453) under `#![forbid(clippy::too_many_arguments)]`. The
  derive types therefore live in this one crate, whose root carries `#![deny(…)]`
  instead of `forbid`; everything else keeps `forbid`.
- **`xtask`** holds repository checks (§12). It is not shipped.

Measurement and diagnostics are separate modules in both main crates.

**Shared-code rule.** As soon as a second module needs the same kind of function —
parsing or formatting durations, min/max bounds checks, `KEY=VALUE` parsing,
absolute-or-relative values such as `10%` / `50us` — it moves to `netmeasure-util`,
not into a copy. Like core, `netmeasure-util` is I/O-free and has `#![forbid(unsafe_code)]`.

**File-size rule** (revised in review round 1). Lines inside `#[cfg(test)]` items do
not count; all other physical lines do, including comments and blank lines.

- **Target: 200 lines.**
- **Hard maximum: 400 lines**, allowed only where splitting would tear apart one
  coherent idea, e.g. the three-level bootstrap. A file between 201 and 400 lines must
  begin with a justification line `//! line-limit: <reason>`.
- `cargo xtask line-limit` (§12) fails on any file over 400, and on any file over 200
  without that line. It also lists all justified files, so their number stays visible
  in review.
- Otherwise a module that grows past the target becomes a directory module
  (`stats/bootstrap/{mod,stationary,block_length,levels}.rs`).

**No argument-count escape hatch.** `#[allow(clippy::too_many_arguments)]` is
forbidden. Every crate root carries `#![forbid(clippy::too_many_arguments)]`; because
it is `forbid`, an inner `allow` is a compile error, not just a lint. The single
exception is `netmeasure-cli` (above), which has `deny`; there, and everywhere else,
`cargo xtask line-limit` rejects any hand-written `allow`, `expect` or `warn` of the
lint or of the groups containing it (`clippy::complexity`, `clippy::all`). A function that
would exceed clippy's limit (7) takes a context struct instead (e.g. `ProbeCtx`,
`CycleCtx`, `ReportCtx`), named after what it groups rather than `Params`.

**`unsafe` policy.** `#![forbid(unsafe_code)]` in core and util. In the binary, all
`libc` calls (`getsockopt`, `ppoll`, `clock_nanosleep`, `prctl`, `ioctl`) live in a
single module `sys/`, which is the only place with `#[allow(unsafe_code)]`; the rest of
the crate is `#![deny(unsafe_code)]`.

```
crates/netmeasure-util/src/
  time.rs            the only jiff user: duration parsing ("10ms", "20s"), RFC 3339 timestamps,
                     timestamp arithmetic (overlap, boot time = now − uptime), ns conversion
  bounds.rs          range-checked values (min/max with error messages naming the flag)
  kv.rs              KEY=VALUE and comma-list parsing (--label, K8S_NETMEASURE_LABEL, list-valued env vars)
  amount.rs          absolute-or-relative amounts ("10%", "50us") for margins and thresholds

crates/netmeasure-core/src/
  config.rs          effective configuration types + validation (RunConfig, ListenConfig, ReportConfig)
  labels.rs          --label / K8S_NETMEASURE_LABEL parsing and normalisation (§9)
  measure/
    sample.rs        Sample, Outcome {Ok, Timeout, Refused, NetError(errno), LocalError(errno)}, SynRetrans {Known(u32), Unknown}
    schedule.rs      pacing arithmetic: slot deadlines, overrun handling, round-robin order (pure functions)
    cycle.rs         cycle slot plan, validity evaluation from before/after snapshots → reasons
  diag/
    snapshot.rs      snapshot types (cgroup cpu, listener, netstat, ethtool, node context) + delta logic
  schema/
    result.rs        result-file serde types, schema_version, columnar encode/decode
    control.rs       control-port response body types (/v1/info, /v1/context, /v1/snapshot)
  stats/
    quantile.rs      quantile estimator with censoring
    order_ci.rs      distribution-free quantile CI (binomial)
    bootstrap.rs     stationary block bootstrap, two-/three-level resampling, block-length selection
    verdict.rs       better / worse / equivalent / inconclusive, TOST, margins, censored bounds
    rates.rs         exact rate intervals, descriptive only (< 10 failures), never a verdict
    multiplicity.rs  primary-quantity family, k (§10.4), Bonferroni levels, intersection-union roles
  report/
    model.rs         grouping, declared-design validation, crossover estimand, evidence label
    compare.rs       context comparison, expected/unexpected differences, refusal rules
    render_text.rs   benchstat-style table → String
    render_json.rs

crates/k8s-netmeasure/src/
  main.rs, cli.rs, exit.rs     (main.rs sets the mimalloc #[global_allocator])
  sys/               the only unsafe module: thin wrappers over libc syscalls
  shutdown.rs        signal flags
  logging.rs         tracing setup, field names (§9)
  measure/
    probe.rs         the per-sample syscall sequence (§3.1)
    thread.rs        dedicated measuring thread: runs one cycle per command, preallocated buffers
    load.rs          background load threads (§6)
  listener.rs        measured listener + accept thread
  control/
    server.rs        hyper server on the control thread (listen); routes, strict 404/405/400
    client.rs        hyper client via block_on (run), keep-alive per target
  diag/
    cgroup.rs        /sys/fs/cgroup/cpu.max, cpu.stat
    sockinfo.rs      TCP_INFO, SO_MEMINFO decoding (syscalls via sys/)
    netstat.rs       /proc/net/netstat, /proc/net/snmp
    ethtool.rs       SIOCETHTOOL: drvinfo, ring params, channels, stats (underlay)
    node.rs          uname, /proc/uptime, boot_id, somaxconn, tcp sysctls, MTU, interfaces
  cmd/
    listen.rs, run.rs, report.rs, selftest.rs, probe.rs
  output.rs          atomic file write

crates/netmeasure-cli/src/
  lib.rs             clap derive types, env-name rule, config-surface check (§9.1)

xtask/src/           cargo xtask fmt | clippy | line-limit | test | deny | audit | coverage | ci | musl (§12)
```

The allocator does not change the measurement: the measuring thread does not allocate
in the timed section or between samples (preallocated buffers). mimalloc mainly helps
`report`, where the bootstrap allocates heavily, and the load threads.

Threads in `run`: **main** (orchestration, snapshots, control-port client, file
writing, shutdown flag), **measuring** (one, dedicated), **accept** (own listener,
needed for the loopback control), **control** (HTTP server, only when `--control-port`
is set, §4.1), **load** × N (optional). `listen` has main, accept and control. The measuring thread never
logs, allocates or touches the network outside its probe sequence; it receives
"run cycle k" over a channel and hands back a filled, preallocated buffer.
