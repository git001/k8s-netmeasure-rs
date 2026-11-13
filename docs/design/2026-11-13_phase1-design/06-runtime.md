# Phase 1 design — 06 Load, process behaviour and configuration

Part of the [Phase 1 design](README.md) · covers §6, §8, §9 · previous: [Result file](05-result-file.md) · next: [Statistics](07-statistics.md)

---

## 6. Background load interface

```
--load-threads N        default 0
--load-rate R           total connects/s across all load threads, required when N > 0
--load-rate max         unpaced (closed loop) — allowed, but recorded and warned as not comparable
```

Load threads use the same probe sequence as the measuring thread, round-robin over
the non-loopback targets, paced open-loop at `R / N` each. That keeps the load
independent of latency, for the same reason the measuring thread is paced. Results per
cycle: attempts, counts by outcome, SYN retransmissions; durations are discarded.
Load starts with the first warm-up sample and stops on the first shutdown signal. The
README notes that at high rates conntrack entries from RST-closed connections stay
around for a few seconds, which is part of the load profile.

**Load and listener drops** (review round 2). Load that overflows the target
listener's accept queue makes `listener_drops` invalidate cycles. This is
deliberate: a dropped SYN may be a *measuring* SYN, so the measured distribution is no
longer the network's. The drops are therefore not downgraded to an annotation.
Unpaced `--load-rate max` against the default backlog will very likely invalidate
most cycles and end with exit 4. `run` warns at startup when `--load-rate max` is set,
or when the configured rate exceeds the target's reported effective backlog per
second. The README describes how to size `--backlog` and `--load-rate` together.

---

## 8. Process behaviour, shutdown, exit codes

- `--shutdown-deadline` **default 10 s**. On the first signal, load threads stop and
  no new cycle starts. If the running cycle's remaining slots × interval ≤ the
  remaining deadline, the cycle finishes; otherwise it ends at once as `interrupted`
  with its samples kept. An in-flight connect always completes or times out (≤ 5 s).
  Worst case from signal to exit ≈ deadline + timeout + file write ≈ 16 s, below the
  default 30 s grace period. Phase 2 chart rule: `terminationGracePeriodSeconds ≥
  shutdown_deadline + timeout + 10 s`.
- **Signal handling without a handler** (decided in milestone 3a, replacing
  `signal-hook`): SIGTERM and SIGINT are blocked in the main thread before any other
  thread starts, so every thread inherits the mask; one `signals` thread takes them
  with `sigwait` (`nix` `SigSet::wait`, no `unsafe`, no extra crate) and sets a flag
  every thread reads.
- **Implemented in milestone 3e:** the measuring thread checks the flag between
  slots; at the first slot after the signal it decides once whether the remaining
  slots × interval fit into what is left of the deadline (measured from the signal).
  A finished cycle stays valid; the file status is still `interrupted`, because the
  run as a whole ended early, and no further cycle starts. `--shutdown-deadline`
  accepts 1 ms – 300 s and is recorded as `config.shutdown_deadline_ns`.
- A second signal exits immediately (the `signals` thread calls `exit(128 + signal)`). Because of
  the atomic write there is then either no new file or a complete one.
- `listen`: `/ready` switches to `503` at once; stop accepting, close the measured
  listening socket, stop the control server, exit.

| Exit code | Meaning | Result file |
|---|---|---|
| 0 | success, all cycles valid | `complete` |
| 1 | internal error (incl. caught panic) | written if possible, `failed` |
| 2 | configuration / usage error (clap's own code) | none |
| 3 | refused to start: self-check failed (CPU quota floor, cgroup unreadable, bind failed) | none |
| 4 | completed, at least one cycle invalid | `complete` |
| 130 / 143 | interrupted by SIGINT / SIGTERM (128 + signal number, the shell convention Kubernetes users already recognise) | `interrupted`, unless it was the second signal |

`report`: 0 report produced; 2 usage error (including a missing `--design`); 5
comparison refused (context mismatch without override, or the files contradict the
declared design, §10.6); 1 internal error. The verdict does **not** set the exit code
in Phase 1.

`selftest`: 0 all checks ran; 6 at least one check could not establish what it tests;
2, 3 and 1 as above (§3.8).

`probe`: 0 all connects `ok`; 7 at least one connect not `ok`; 2, 3 and 1 as above
(§3.9).

---

## 9. Configuration, labels and log fields

### 9.1 Configuration surface

- **Every parameter is also an environment variable**, without exception. The name is
  derived mechanically from the long flag: `K8S_NETMEASURE_` + flag in upper snake
  case. For example `--interval` → `K8S_NETMEASURE_INTERVAL`, `--shutdown-deadline` →
  `K8S_NETMEASURE_SHUTDOWN_DEADLINE`, `--label` → `K8S_NETMEASURE_LABEL`.
  - **[deviation]** The prompt uses the prefix `NETMEASURE_` and the name
    `NETMEASURE_LABELS`; both are replaced.
  - The metric prefix stays `netmeasure_`.
  - Repeatable flags (`--label`, `--target`) take a comma-separated list in the env
    form.
  - Booleans accept exactly `true` or `false`.
  - Since a process runs exactly one subcommand, a flag shared by several subcommands
    (e.g. `--port`) maps to one variable.
- **Unknown parameters are hard failures** (exit 2), on every input path:
  - Command line: an unknown flag or argument (clap's default behaviour, kept).
  - Environment (refined in review round 2): a variable with the prefix
    `K8S_NETMEASURE_` that maps to a parameter of **no** subcommand fails at startup,
    and the error lists the names. This catches typos in a DaemonSet `env:` block,
    which would otherwise be silently ignored. A variable that belongs to **another**
    subcommand (e.g. `K8S_NETMEASURE_SAMPLES` seen by `listen`) is not unknown. It is
    ignored and logged once at INFO with the subcommand it belongs to, so one shared
    ConfigMap can feed both the `listen` DaemonSet and the `run` Job.
  - Structured input: control-port responses and result files read by
    `report` are deserialised with `#[serde(deny_unknown_fields)]`. Result files must
    carry a supported `schema_version`.
- **A parameter given both on the command line and in the environment is a hard
  failure** (exit 2), not a silent override. clap would let the CLI win without
  notice; a Job spec with both `args:` and `env:` setting the same value is a
  configuration error, and in a support case it must be clear which value was used.
  - **Implementation note** (review round 3): clap's `ArgMatches::value_source()`
    reports only the *winning* source, so this case cannot be detected from it. The
    config layer reads the environment itself (`std::env::vars_os`, prefix
    `K8S_NETMEASURE_`) and fails for every argument whose `value_source()` is
    `CommandLine` while its env var is also set. The config-surface test (§12) covers
    exactly this case for every argument.
- Empty values (`K8S_NETMEASURE_INTERVAL=`) are an error, not "unset".
- **Provenance:** for every parameter the effective configuration in the result file
  records its source (`cli` / `env` / `default`) in `run.config_source` (§5).
- Enforcement: a unit test walks the complete clap command tree and asserts that every
  argument (except `--help`/`--version`) has an env var, that its name matches the
  derivation rule, and that no two arguments of one subcommand share a name.

### 9.2 Labels

- `--label k=v` (repeatable) and `K8S_NETMEASURE_LABEL="k=v,k2=v2"`. Keys are normalised
  to Prometheus label names: `-` and `.` become `_`, and the result must match
  `[a-zA-Z_][a-zA-Z0-9_]*`, must not start with `__`, and must not be a reserved field
  name (`target_node`, `path`, `result`, `cycle_id`, `run_id`, `level`, `message`,
  `instance`, `job`, …). A collision after normalisation, a duplicate key or an empty
  value is an error. Values are UTF-8, ≤ 256 bytes, with no comma in the env form.
  This matches the prompt's own example (`--label kernel-change=none` →
  `kernel_change="none"`); parsing follows the strict k8s-netperf model (validation,
  duplicate rejection, protected keys).

### 9.3 Log fields

- Log lines: flat JSON; labels as top-level fields; `target_node`, `path`, `result`,
  `cycle_id` spelled exactly as the later Prometheus labels. The loopback target is
  `target_node="loopback"`. One INFO line per cycle per path; one WARN per sample above
  `--outlier-threshold` (default 20 ms); WARN for an invalid cycle; ERROR for a target
  unreachable for a whole cycle.
- **Log level** (added in milestone 2 review): every subcommand takes `--log-level`
  (`error`, `warn`, `info`, `debug`, `trace`; `K8S_NETMEASURE_LOG_LEVEL`), default
  `info`. `debug` adds detail for development and support cases (parameter sources,
  full `selftest` check data). The measuring thread never logs inside a cycle, so no
  level can disturb the timed section; filtered-out levels cost only a level check.
