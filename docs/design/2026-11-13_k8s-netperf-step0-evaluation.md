# k8s-netperf — Step 0 evaluation

Source: https://github.com/cloud-bulldozer/k8s-netperf, commit `cff00a390a25` (2026-11-04).
Paths below are relative to that repository. Line numbers refer to that commit.

## 1. What it measures, in code

k8s-netperf does not time anything itself. It execs a benchmark binary inside a
long-sleeping client pod and parses that binary's text output.

- `pkg/drivers/netperf.go` `(*netperf).Run` builds a `super-netperf <parallelism> <port> -H <ip> -l <duration> -t <profile> -- -k rt_latency,p99_latency,throughput,...`
  command and runs it via the `pods/exec` subresource (SPDY, line 147).
- `containers/super-netperf` starts `parallelism` netperf processes, then sums
  throughput and computes the **arithmetic mean of the per-process `P99_LATENCY`
  values** with `bc` without `scale`, i.e. **truncated to integer µs**. The result is
  not a p99 of anything once parallelism > 1.
- `(*netperf).ParseResults` (line 176) reads `P99_LATENCY`, `RT_LATENCY`,
  `THROUGHPUT`, retransmits and send/recv calls.
- `cmd/k8s-netperf/k8s-netperf.go` `executeWorkload` (line 960) repeats this `samples`
  times and appends each value to `LatencySummary` etc.
- `pkg/results/result.go` `ShowLatencyResult` (line 309) prints
  `Average(r.LatencySummary)`: **a mean of (means of) p99 values**, labelled
  "Avg 99%tile value".
- For TCP_CRR the timed unit is a full transaction: connect + request + response +
  close. For TCP_RR it is request/response on an established connection. No
  profile isolates `connect()`.
- `docs/output-and-results.md` shows a "Stream Latency Results" table for
  TCP_STREAM; the code only renders it for profile `STREAM_LAT`. The docs are stale.
- `ConfidenceInterval` (result.go:91) is `go-moremath` `MeanCI` over the per-sample
  throughput values. It is display-only and never feeds a decision.

## 2. Process lifecycle

One-shot CLI, run from a workstation with a kubeconfig.

- `Run` in `cmd/k8s-netperf/k8s-netperf.go`: `cleanup()` first (line 281),
  `k8s.BuildInfra` creates namespace `netperf` and the ServiceAccount,
  `k8s.BuildSUT` (line 510) creates Deployments whose containers run
  `sleep 10000000`, benchmarks run via exec, results are written, `cleanup()`
  again (line 703), `os.Exit(retCode)`.
- No service mode, no operator, no metrics endpoint.

## 3. Placement and discovery across nodes and zones

- Node selection: `node-role.kubernetes.io/worker` In `""`, `infra` and `workload`
  NotIn (`pkg/k8s/kubernetes.go` `BuildSUT`, ~line 676).
- Client/server separation: required pod anti-affinity on the `role` label
  (`clientRoleAffinity`, line 768; `sdp.PodAntiAffinity`, line 1224).
- Zones: `GetZone` (line 1592) counts nodes per `topology.kubernetes.io/zone` and
  picks a zone with more than one node; `zoneNodeSelectorExpression` (line 1449)
  adds a **preferred**, not required, node affinity for that zone.
- Endpoint discovery: server `Status.PodIP`, or Service `ClusterIP` with
  `service: true`, or network-status annotations for UDN/bridge/SR-IOV/MACVLAN
  (`executeWorkload`). Node info via `GetPodNodeInfo` (line 1834) from the Node
  object's `status.nodeInfo`.
- **Bug: `--across` has no effect.** `Run` declares `var acrossAZ bool` (line 99),
  shadowing the package-level flag variable bound at line 1173. `s.AcrossAZ`
  (line 287) is therefore always `false`, so the across-zone branch in `BuildSUT`
  never runs. Later the local variable is set to `nodesInZone <= 1` (line 526), which
  only affects the `AcrossAZ` field in the results, not placement.

## 4. `--all` / `--tcp-tolerance`

- `--all` only adds hostNetwork runs: `HostNetwork: full || hostNetOnly`.
- After the run (line 688): if any hostNetwork result exists,
  `result.TCPThroughputDiff` (result.go:132) pairs **TCP_STREAM throughput**,
  hostNetwork vs podNetwork, same message size, **parallelism 1 only**, no Service,
  no VM. `calDiff` (line 200) computes `(host − pod) / ((host + pod) / 2) · 100`.
  If that exceeds `--tcp-tolerance` (default 10), exit code 1.
- What it is: a symmetric percent difference between two **paths within one run**,
  on mean throughput.
- What it is not: a comparison across runs or time, a latency comparison, or a
  statistical test. Variance, sample count and confidence intervals are ignored.
  Note also that pod faster than host gives a negative difference and always passes.

## 5. `--label` / `--annotation`

- Flags: `StringArrayVar`, repeatable (lines 1200–1201). Parsed by `parseLabels` /
  `parseAnnotations` (lines 765, 789): KEY=VALUE, Kubernetes name/value validation,
  duplicate keys rejected, tool-managed keys (`managedLabelKeys`,
  `managedAnnotationKeys`) rejected.
- Flow: `PerfScenarios.Labels/Annotations` → `DeploymentParams.WorkloadLabels/Annotations`
  → `newDeployment` (line 1747) puts them on the **pod template** only;
  `mergeLabels` (line 1822) applies managed labels last so selectors stay stable.
  Also passed to VMIs (`launchServerVM`/`launchClientVM`).
- **They do not reach the results.** No field in `result.Data`, `result.ScenarioResults`
  or `archive.Doc`, nothing in CSV, JSON or OpenSearch. The only run identifier is
  `--uuid` (OpenSearch documents only).
- Hard-coded: `sidecar.istio.io/inject: "true"` on every benchmark pod (line 1784).

## 6. Where results go

- stdout: `tablewriter` tables (`ShowStreamResult`, `ShowRRResult`,
  `ShowLatencyResult`, `ShowSpecificResults`).
- CSV files (default on): `archive.WriteCSVResult` (archive.go:445),
  `WritePromCSVResult`, `WriteSpecificCSV`.
- JSON file `result-<timestamp>.json` with `--json` (`WriteJSONResult`, archive.go:419).
- OpenSearch with `--search` (`BuildDocs`, archive.go:91), one document per result.
- Prometheus is **read**, not written: node CPU, OVS CPU/memory, top pods, kernel
  (`node_uname_info`), MTU, discovered via go-commons `GetPrometheus()` with
  `SkipTLSVerify = true` (`pkg/metrics/system.go` `Discover`).

## 7. Adopt / avoid

Adopt:

- **Strict parsing of `KEY=VALUE` flags**: Kubernetes validation, duplicate
  rejection and a protected set of tool-managed keys (`parseLabels`). Copy the
  behaviour, not the semantics (in k8s-netmeasure-rs `--label` is run metadata).
- **Managed labels applied last** (`mergeLabels`) so user labels can never break
  selectors.
- **Recording node context with every result** (`GetPodNodeInfo`: kernel, OS image,
  container runtime) — matches the `node_info` requirement.
- **Explicit worker/infra/workload node filtering** for target selection.
- **Guaranteed-QoS option** (`--cpu`/`--memory`, `VerifyGuaranteedQoS`) — relevant
  given our CFS-throttling experience.

Avoid:

- Exec-based measurement through the API server, and an external benchmark
  binary whose output is parsed from text.
- Averaging percentiles (across processes and across samples), and integer
  truncation of latency.
- A verdict as a single percent threshold without variance, sample size or interval.
- Variable shadowing of flag state (`acrossAZ`) — a reason to keep config in one
  typed struct and test the flag → behaviour path.
- No security context on pods; hard-coded sidecar injection.
- Run identity that exists only in one output (UUID in OpenSearch) instead of in
  every output.

Verdict: nothing in k8s-netperf measures `connect()` latency, and its comparison
mechanism is not suitable for before/after verification. Use it as a reference for
the items listed under "Adopt"; build k8s-netmeasure-rs fresh.
