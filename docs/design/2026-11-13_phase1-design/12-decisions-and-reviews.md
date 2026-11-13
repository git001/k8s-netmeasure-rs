# Phase 1 design — 12 Decisions and review log

Part of the [Phase 1 design](README.md) · covers §16 · previous: [Implementation plan and prerequisites](11-implementation-plan.md)

---

## 16. Open questions

1. ~~**Multiplicity.**~~ **Decided 2026-11-13:** one primary quantity (`--primary`,
   default p99) with a verdict, Bonferroni across pairs only, other quantiles
   descriptive. See §10.4.
2. ~~**Context in the overlay variant.**~~ **Decided 2026-11-13:** already in
   Phase 1, overlay processes fetch NIC context from an underlay `listen` on the same
   node (`--node-context-from`), verified by `boot_id`; no hand-written context file.
   See §5.
3. ~~**`selftest` subcommand.**~~ **Decided 2026-11-13:** yes, as a regular subcommand,
   documented exactly like the others (not hidden). See §3.8.
4. ~~**Compression.**~~ **Decided 2026-11-13:** none; result files are plain JSON
   (≈ 2 MB per 100,000 samples; corrected in review round 3). See §5.
5. ~~**Default ports.**~~ **Decided 2026-11-13:** no environment constraints; defaults
   stay as in §4.1: `listen`/`serve` 17700 (measured) and 17701 (control), `run`
   loopback-only ephemeral (measured). *Amended in review round 1:* `run`'s control
   port is opt-in (17702 suggested), and the control port binds to the pod IP.
6. ~~**glibc binary for ad hoc `oc cp`.**~~ **Decided 2026-11-13:** UBI 10 image
   with dynamic glibc. Ad hoc use goes through the image, including ephemeral
   containers. *Amended in review round 1:* an additional static musl binary as an
   emergency release artifact only. See §1.
7. ~~**Image registry and name.**~~ **Decided 2026-11-13:**
   `ghcr.io/git001/k8s-netmeasure-rs`, pushed by CI. See §13.
8. ~~**Coverage thresholds.**~~ **Decided 2026-11-13:** 90 % line coverage for
   `netmeasure-core` and `netmeasure-util`, 70 % for `k8s-netmeasure`. See §12.

### External review round 1 (2026-11-13)

Two external reviews. Points taken over after evaluation:

- connect timing: EINTR loop on an absolute deadline, signals blocked in the measuring
  thread, POLLERR/POLLHUP as completion; `SO_ERROR` deliberately stays outside the
  timed section, with the reason stated (§3.1);
- SYN-drop verification made robust, with an explicit `inconclusive` result (§3.3);
- block-length floor declared an assumption and made transparent (§10.2);
- independent resampling conservative only for non-negative covariance, now checked
  (§10.3);
- explicit crossover estimand and period × treatment interaction (§10.6);
- statistics simulation suite with acceptance criteria (§11);
- design declared with `--design`, not detected (§10.6);
- ancestor cgroup throttling recorded, or explicitly `unknown` (§3.7);
- ephemeral-container runs capped at `preliminary`; own pod preferred (§1, §10.5);
- mirrored registries: arbitrary image references everywhere (§13);
- implementation order with early validation on the real cluster (§14).

Decisions taken by the user in this round:

| Topic | Decision |
|---|---|
| full libcurl in the image | removed; `probe` subcommand + `/dev/tcp` cross-check instead (§3.9, §13) |
| `run` control port | off by default, opt-in via `--control-port` (§4.1) |
| static musl build | yes, as an emergency CI release artifact only (§1, §12) |
| duplicate dependency versions | forbidden for direct dependencies; transitive only as reasoned, dated exceptions (§1, §12) |
| file-size limit | target 200 lines, up to 400 with a `//! line-limit:` justification (§2, §12) |
| control-port exposure | bind to the pod IP by default; no token; underlay exposure accepted and documented (§4.1) |

### External review round 2 (2026-11-13)

Points taken over after evaluation:

- §3.0: why connect latency, what it covers and what not; report header sentence;
  echo mode noted as a later extension;
- §3.1: pseudocode order fixed; `ok` requires `tcpi_state == ESTABLISHED`, otherwise
  `anomaly`;
- §3.4: the four overrun rules stated exactly and tested one by one;
- §3.5: 10 cycles per run with even distribution instead of a fixed 20 s cycle, so the
  tail reserve covers two invalid cycles and there is no remainder cycle;
- §3.6: "substantially more annotated cycles" defined as > 10 percentage points;
- §3.7: ancestor throttling inferred where the kubelet's quota rules allow it,
  `unverified` until checked;
- §3.8: signal-isolation check uses SIGUSR2, not SIGTERM;
- §5: dense `sched_delta_ns` replaced by a sparse `reanchor` column;
- §6: load-induced listener drops stay invalidating; `run` warns;
- §10.2: floor/cap conflict for runs shorter than 50 s is a warning;
- §10.5: reboot must be recent (`--max-boot-age`); `strong` also needs the same
  effect direction in both periods and leave-one-node-out stability;
- §10.6: overlap defined (intersection / union per period); θ written on the
  margin's scale; loopback compared on the median;
- §11: pairwise scenario selection, fixed seeds, R = 2000, explicit Monte-Carlo
  tolerances, semi-synthetic check on real data;
- §12: `clippy.toml` cannot raise the argument limit.

Decisions taken by the user in this round:

| Topic | Decision |
|---|---|
| scope of the connect measure | document strength and limit, state what a verdict does not mean; echo mode after Phase 1 (§3.0) |
| failure-rate verdict | block bootstrap like the latency; verdict only with ≥ 10 failures, otherwise descriptive (§10.4) |
| loopback share | every 10th slot (`--loopback-every 10`) (§3.4) |
| env vars of other subcommands | ignored and logged; only variables of no subcommand are hard failures (§9.1) |
| NIC families (raised by the user after the round) | NIC as a declared comparison factor, per-family stratification without pooling, kernel × NIC interaction with negative-control reading, observational evidence labels for direct NIC comparisons (§10.7); `--factor` replaces `--expect-change` |
| document size | the single 1,550-line document was split into this directory with an index; section numbers unchanged (README) |

### External review round 3 (2026-11-13)

Last round before implementation. Points taken over after evaluation:

- §3.5: cycles split by counted samples per target, warm-up added to cycle 0; with
  evenly split slots two excluded cycles left 39 exceedances above p99, not 40;
- §16: file size corrected to ≈ 2 MB per 100,000 samples (§5 was right);
- §2: `rates.rs` is descriptive only, never a verdict;
- §3.1: `ppoll` remaining time clamped to 0;
- §3.2, §3.6, §3.8: `anomaly` listed consistently under `sample_count_short`; `selftest`
  reports the anomaly rate;
- §10.4: k defined once; timeouts couple the latency and failure families;
- §10.5: boot age and boot skew checked per crossover period;
- §5, §9.1: `config_source` in the example; clap `value_source()` cannot detect CLI
  and env both set, the config layer reads the environment itself;
- §10.6: `sequential` tolerates ≤ 1 s intersection (clock steps); `clock.synchronized`
  recorded;
- §11: cost estimate corrected to 1–2.5 CPU-hours; censored-p99 dimension and
  criteria for the censored rule and the intersection-union reading.

Decisions taken by the user in this round:

| Topic | Decision |
|---|---|
| Bonferroni across strata makes the negative control expensive | with `--expect-effect-on`, strata get roles (effect / control / mixed) and form one intersection-union hypothesis without correction across strata; without it, Bonferroni over pairs × strata stays and the report prints the sample cost (§10.7) |
| verdict under censoring | timeouts as +∞; `worse`/`better` only from a finite bound, never `equivalent` with an infinite bound, otherwise `inconclusive` "censored" (§10.4) |

### Milestone 2 planning: NIC family is per host (2026-11-13)

Real node data from the target environment showed nodes of the **same instance
type, kernel and OS build with different NIC families**; the family follows the host,
and machines are recreated often enough to move between families. Decided by the user
(all five points):

| Topic | Decision |
|---|---|
| same instance type, different family | report detects and states it; for `--factor nic` it removes the instance-type confound (§10.7) |
| NIC path during a run | checked around every cycle (VF present, bound driver, default-route interface); a change invalidates the cycle as `nic_changed` (§3.6) |
| family change between crossover periods | node is a context mismatch: exit 5, or excluded with a warning under `--allow-context-mismatch nic_family`; only stable nodes count for the negative control (§10.6) |
| host CPU as a confound | CPU model and microcode in the node context (§5) |
| choosing pairs | inventory of node → family via `listen` + `/v1/context` before every test series and after every reboot (§13) |

### Left open on purpose, settled by implementation step 2 (§14)

These are not design gaps. They can only be answered with data from the target
cluster, and the implementation order exists precisely for that.

1. **Does the connect measure see the effect at all?** If the regression lives only
   on established connections, connect latency may show no difference even if the
   problem is real (§3.0). Step 2's first real runs show whether the connect tail
   reacts at all; if not, the echo mode moves up.
2. **Failure-rate dependence.** Whether timeouts cluster as strongly as assumed, and
   whether ≥ 10 failures is a sensible threshold, is checked on the first real runs
   (§10.4).
3. **Pod-level cgroup inference.** Verified once on a node (§3.7, §14).
4. **`anomaly` rate on the target kernel** (added in review round 3). Every anomaly
   invalidates a cycle (§3.2). `selftest` measures the rate on loopback, step 2 on
   both NIC families on real paths. If it is not zero, the cycle rule is revisited
   before step 3.
