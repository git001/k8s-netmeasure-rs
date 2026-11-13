# k8s-netmeasure-rs — Phase 1 design

Status: **all open questions decided; external review rounds 1–3 incorporated
(2026-11-13); four points deliberately left open until implementation step 2
(§16); awaiting final approval.** No production code exists yet.

Scope: Phase 1 of [the agent prompt](../2026-11-13_k8s-netmeasure-rs-agent-prompt.md)
(the "prompt" in all parts): `listen`, `run`, `report` on result files, plus
`selftest` and `probe`, as a glibc binary in a UBI 10 minimal container image. No
Kubernetes API, no Prometheus. The Step 0 evaluation of k8s-netperf and goldpinger is
in [../2026-11-13_k8s-netperf-step0-evaluation.md](../2026-11-13_k8s-netperf-step0-evaluation.md).

## What Phase 1 has to answer

The tool exists for two questions:

1. **Does a vendor change, e.g. a test kernel, make connect latency better, worse, or
   demonstrably not different?** Comparison designs, statistics and verdicts:
   [07](07-statistics.md), [08](08-report.md).
2. **Do nodes with different NIC families behave differently**, and does a driver
   change act only where that driver is used? NIC as a comparison factor,
   stratification and the kernel × NIC interaction: §10.7 in
   [08](08-report.md).

[02](02-measurement-core.md) §3.0 states exactly what the measured quantity covers
and what it does not.

## Parts

| Part | Covers | Content |
|---|---|---|
| [01 Build, dependencies and code structure](01-build-and-structure.md) | §1–§2 | toolchain, glibc vs musl, verified crate versions, crates and modules, file-size and `unsafe` rules |
| [02 Measurement core](02-measurement-core.md) | §3.0–§3.5 | what is measured, per-sample syscall sequence, outcomes, SYN retransmissions, pacing, cycles |
| [03 Validity, CPU checks, selftest and probe](03-validity-selftest-probe.md) | §3.6–§3.9 | invalidity reasons, CPU quota and throttling, the `selftest` and `probe` subcommands |
| [04 Listener and control port](04-listener-and-control-port.md) | §4, §7 | the two ports, measured listener, HTTP control port, probes, watchdog |
| [05 Result file](05-result-file.md) | §5 | schema, columnar samples, node context, node listener for overlay NIC context |
| [06 Load, process behaviour and configuration](06-runtime.md) | §6, §8, §9 | background load, shutdown, exit codes, env vars, labels, log fields |
| [07 Statistics](07-statistics.md) | §10.1–§10.5 | quantiles, block bootstrap, replication levels, verdicts, multiplicity, evidence labels |
| [08 Report model](08-report.md) | §10.6–§10.7 | declared designs, crossover, comparability, NIC comparison and stratification |
| [09 Tests and quality gates](09-tests-and-ci.md) | §11–§12 | edge-case tests, simulation suite, CI gates |
| [10 Container image](10-container-image.md) | §13 | Containerfile, registry, mirroring, ad hoc use |
| [11 Implementation plan and prerequisites](11-implementation-plan.md) | §14–§15 | milestone order with early validation on the real cluster |
| [12 Decisions and review log](12-decisions-and-reviews.md) | §16 | all decisions, review rounds, points left open on purpose |

## Reading guide

- **Section numbers are global** and unchanged from the original single document, so
  every cross-reference such as "§3.1" stays valid. The table below says which part
  holds which section. §4 and §7 share one part because they belong together; the
  numbering is kept anyway.
- Each part is self-contained enough to review on its own; it names the sections it
  depends on.
- Where the design deviates from or adds to the prompt, it says so explicitly:
  **[deviation]**, **[addition]**. Changes from review rounds are marked "(review
  round 1)" … "(review round 3)".
- The metric prefix `netmeasure_` from the prompt is kept; no objection.
- For a review: [12](12-decisions-and-reviews.md) lists what changed in each round.
  The parts most likely to profit from a second pair of eyes are
  [02](02-measurement-core.md), [07](07-statistics.md) and [08](08-report.md).

## Section map

| § | Title | Part |
|---|---|---|
| §1 | Toolchain and dependencies | [01](01-build-and-structure.md) |
| §2 | Crate and module layout | [01](01-build-and-structure.md) |
| §3.0 | What the measured quantity covers, and what it does not | [02](02-measurement-core.md) |
| §3.1 | Per-sample sequence | [02](02-measurement-core.md) |
| §3.2 | Outcome classification | [02](02-measurement-core.md) |
| §3.3 | SYN retransmissions | [02](02-measurement-core.md) |
| §3.4 | Pacing, round-robin, warm-up | [02](02-measurement-core.md) |
| §3.5 | Run and cycle | [02](02-measurement-core.md) |
| §3.6 | Validity: proposed thresholds | [03](03-validity-selftest-probe.md) |
| §3.7 | CPU quota floor | [03](03-validity-selftest-probe.md) |
| §3.8 | `selftest` subcommand | [03](03-validity-selftest-probe.md) |
| §3.9 | `probe` subcommand | [03](03-validity-selftest-probe.md) |
| §4.1 | Two ports per process | [04](04-listener-and-control-port.md) |
| §4.2 | Measured listener | [04](04-listener-and-control-port.md) |
| §5 | Result file schema | [05](05-result-file.md) |
| §6 | Background load interface | [06](06-runtime.md) |
| §7 | Control port: HTTP/1.1 + JSON | [04](04-listener-and-control-port.md) |
| §8 | Process behaviour, shutdown, exit codes | [06](06-runtime.md) |
| §9.1 | Configuration surface | [06](06-runtime.md) |
| §9.2 | Labels | [06](06-runtime.md) |
| §9.3 | Log fields | [06](06-runtime.md) |
| §10.1 | Per side | [07](07-statistics.md) |
| §10.2 | Block bootstrap | [07](07-statistics.md) |
| §10.3 | Replication levels | [07](07-statistics.md) |
| §10.4 | Compared quantity, verdicts, margins | [07](07-statistics.md) |
| §10.5 | Evidence label | [07](07-statistics.md) |
| §10.6 | Report model | [08](08-report.md) |
| §10.7 | NIC as comparison factor, stratification, kernel × NIC | [08](08-report.md) |
| §11 | Tests | [09](09-tests-and-ci.md) |
| §12 | Quality gates and CI | [09](09-tests-and-ci.md) |
| §13 | Container image | [10](10-container-image.md) |
| §14 | Implementation order | [11](11-implementation-plan.md) |
| §15 | Prerequisites found during Step 0 | [11](11-implementation-plan.md) |
| §16 | Decisions, reviews, points left open | [12](12-decisions-and-reviews.md) |

## Maintenance rule

Keep every part under about 250 lines. A part that grows beyond that is split again,
and this index is updated in the same change. Section numbers are never reused; a new
section gets the next free number in its area.
