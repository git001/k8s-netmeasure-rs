# k8s-netperf — Step 0 evaluation

Source: https://github.com/cloud-bulldozer/k8s-netperf, commit `cff00a390a25` (2026-11-04).
Paths below are relative to that repository. Line numbers refer to that commit.

Status: the pre-check of this document was re-verified line by line against the code.
Every statement of the pre-check held. Items marked **[added]** were found during
verification and were not in the pre-check. External sources (netperf, uperf) are
named where a statement depends on them rather than on this repository.

## 1. What it measures, in code

k8s-netperf does not time anything itself. It execs a benchmark binary inside a
long-sleeping client pod and parses that binary's text output.

### netperf driver (default)

- `pkg/drivers/netperf.go` `(*netperf).Run` builds
  `super-netperf <parallelism> <port> -H <ip> -l <duration> -t <profile> -- -k rt_latency,p99_latency,throughput,...`
  (lines 52–57) and runs it via the `pods/exec` subresource over SPDY (lines 142–165),
  i.e. through the API server, with a TTY.
- `containers/super-netperf` starts `parallelism` netperf processes in the background
  (`run_netperf`, lines 14–18), then in `process_netperf` sums throughput and computes
  `l=$(echo $l/$counter | bc)` (line 64): the **arithmetic mean of the per-process
  `P99_LATENCY` values**, with `bc` at default `scale=0`, i.e. **truncated to integer µs**.
  The result is not a p99 of anything once parallelism > 1. `RT_LATENCY` gets the same
  treatment (line 62).
- **[added]** netperf's own per-process `P99_LATENCY` is reasonable on its own: upstream
  netperf at the pinned commit `3bc455b` (`containers/Containerfile` line 113) keeps
  100 bins per decade (`src/hist.h`, `HIST_NUM_OF_BUCKET 100`) and interpolates linearly
  inside a bin (`src/netlib.c`, `HIST_search_bucket`), returned as `int` µs. The damage
  is done afterwards, by averaging. (Verified in the upstream netperf repository, not in
  this one.)
- `(*netperf).ParseResults` (line 176) reads `P99_LATENCY`, `RT_LATENCY`, `THROUGHPUT`,
  `LOCAL_TRANSPORT_RETRANS` and send/recv calls by substring match on `KEY=VALUE` lines.
  Parse errors from `strconv.ParseFloat` are discarded (`_`).
- `cmd/k8s-netperf/k8s-netperf.go` `executeWorkload` (line 960) repeats this
  `nc.Samples` times (loop at line 1103) and appends each value to `LatencySummary`
  etc. (lines 1121–1126). A "sample" is therefore one whole `duration`-second benchmark
  run, not one transaction; typical configs use 3 samples.
- `pkg/results/result.go` `ShowLatencyResult` (line 309) prints
  `Average(r.LatencySummary)` (line 330): **a mean of (means of) p99 values**, labelled
  "Avg 99%tile value". `archive.WriteCSVResult` writes the same mean under the header
  "99%tile Observed Latency" (`pkg/archive/archive.go` lines 462, 471).

### uperf driver **[added]**

- The uperf driver computes "p99" differently. `(*uperf).ParseResults`
  (`pkg/drivers/uperf.go` line 325) reads uperf's per-interval totals (`-i 1`, line 225),
  computes per 1-s interval `(Δtimestamp_ms / Δops) · 1000` (line 351) — the **mean**
  time per operation in that second — and then takes `stats.Percentile(latSummary, 99)`
  (line 366): the **p99 of about `duration` per-second means**. With the default 10 s
  that is effectively the maximum of ~9 means.
- Because uperf reports group totals across `nprocs` threads (uperf semantics, not
  verified in this repository), with parallelism > 1 the per-interval value is inverse
  throughput, ≈ latency / parallelism, not latency.
- Only `TCP_STREAM_LAT` uses a real histogram (a uperf fork, `/opt/uperf-histogram`,
  regex-parsed "99th" line, lines 369–385). The uperf driver does not support `TCP_CRR`
  (`IsTestSupported`, line 41–43).
- The same table column "Avg 99%tile value" therefore contains two incompatible
  quantities depending on the driver column.

### What is timed

- For `TCP_CRR` the timed unit is a full transaction: connect + request + response +
  close. **[added]** Since the client waits for the response, this includes the server
  application's `accept()`, read and write — exactly what our metric deliberately
  excludes. For `TCP_RR` it is request/response on an established connection. No
  profile isolates `connect()`.
- `docs/output-and-results.md` (lines 45–50) shows a "Stream Latency Results" table
  for `TCP_STREAM`/`UDP_STREAM` with netperf; the code only renders that table for
  profile `STREAM_LAT` (`result.go` line 311), which netperf does not support
  (`netperf.go` line 228). The docs are stale.
- `ConfidenceInterval` (`result.go` line 91) is `go-moremath` `MeanCI` over the
  per-sample throughput values (n = samples, usually 3). It is display-only and never
  feeds a decision. There is no interval on any latency value.

## 2. Process lifecycle

One-shot CLI, run from a workstation with a kubeconfig.

- `rootCmd.Run` in `cmd/k8s-netperf/k8s-netperf.go`: optional `cleanup()` first
  (line 281), `k8s.BuildInfra` creates namespace `netperf`, ServiceAccount and a
  RoleBinding to `system:openshift:scc:hostnetwork` (`pkg/k8s/kubernetes.go` lines
  245–298), `k8s.BuildSUT` (line 510) creates Deployments whose containers run
  `sleep 10000000`, benchmarks run via exec, results are written, `cleanup()` again
  (line 703), `os.Exit(retCode)` (line 709).
- No service mode, no operator, no metrics endpoint, no signal handling.
- **[added]** Failure handling: a failed `driver.Run` is `log.Fatal` (line 1107); a
  failed parse is retried up to 3 times (`rerunSample`, line 1137) and then
  `log.Fatalf` (line 1118). Either way the process exits **without writing any result
  and without cleanup**: one bad sample loses the whole run. Contrast with our rule
  "timeouts and errors are results, not gaps" and "always write what exists".
- **[added]** `WaitForReady` (line 1537) watches Deployments without a timeout; a pod
  that never becomes ready blocks forever. `GetPodNodeInfo` indexes `pods.Items[0]`
  without a length check (line 1844).
- **[added]** Test order is not deterministic: `config.ParseConf` and `ParseV2Conf`
  iterate over a Go map (`pkg/config/config.go` lines 153, 185). Irrelevant for a
  single run, but two runs do not execute scenarios in the same order.

## 3. Placement and discovery across nodes and zones

- Node selection: `node-role.kubernetes.io/worker` In `""`, `infra` and `workload`
  NotIn (`BuildSUT`, `pkg/k8s/kubernetes.go` lines 676–686).
- Client/server separation: required pod anti-affinity on the `role` label
  (`clientRoleAffinity`, line 768; server vs `client-across` lines 1212–1224;
  `server-host` vs `client-host` lines 1225–1237).
- Zones: `GetZone` (line 1592) counts worker nodes per `topology.kubernetes.io/zone`
  and returns a zone with more than one node (the last one encountered), else the last
  zone; `zoneNodeSelectorExpression` (line 1449) adds a **preferred**, not required,
  node affinity for that zone (and for a `netperf=client|server` node label, if any
  node carries it).
- Endpoint discovery: server `Status.PodIP`, or Service `ClusterIP` with
  `service: true`, or network-status annotations for UDN/bridge/SR-IOV/MACVLAN, or a
  static IP for localnet (`executeWorkload`, lines 970–1066). Node info via
  `GetPodNodeInfo` (line 1834): node name, host IP and `status.nodeInfo` (kernel,
  OS image, container runtime, kubelet). **[added]** No zone, no instance type, no NIC
  information is recorded per result.
- **Bug: `--across` has no effect.** `Run` declares `var acrossAZ bool` (line 99),
  shadowing the package-level variable (line 56) bound to the flag at line 1173.
  `s.AcrossAZ` (line 287) is therefore always `false`, so the across-zone branch in
  `BuildSUT` (lines 747–755, 1181–1183) never runs. Later the local variable is set to
  `nodesInZone <= 1` (line 526), which only feeds the `AcrossAZ` field in the results
  (line 539, 1083–1087), not placement. **[added]** That field is therefore an
  inference from zone node counts, never an observation of where the pods actually
  landed.
- **[added] Host and pod paths may run on different node pairs, and the recorded node
  info is wrong for one of them.** `client-host` and `client-across` are separate
  Deployments; both are anti-affine only to `role=client` (which exists only in
  `--local`/external mode), and the server pods are anti-affine only to their own
  client. Nothing ties the hostNetwork pair to the pod-network pair. `s.ClientNodeInfo`
  and `s.ServerNodeInfo` are set once from the pod-network pods (lines 1265–1271) and
  copied into every result (lines 1129–1130), including the hostNetwork results.
- **[added]** Dead code: in `--local` mode the preferred zone affinity (lines 808–812)
  is overwritten immediately by the required worker affinity (lines 814–816).

## 4. `--all` / `--tcp-tolerance`

- `--all` only adds hostNetwork runs: `HostNetwork: full || hostNetOnly` (line 284);
  `--all` and `--hostNet` are mutually exclusive (line 1180).
- After the run (lines 686–701): if any hostNetwork result exists and `--hostNet` is
  not set, `result.TCPThroughputDiff` (`result.go` line 132) pairs **`TCP_STREAM`
  throughput**, hostNetwork vs podNetwork, same message size, **parallelism 1 only**
  (`doPerfDiff(..., 1)`, line 160), no Service, no VM. Each side's value is
  `Average(ThroughputSummary)` (lines 142, 147). `calDiff` (line 200) computes
  `(host − pod) / ((host + pod) / 2) · 100`. If `tcpt < diff.Result` (line 695),
  exit code 1.
- What it is: a symmetric percent difference between two **paths within one run**, on
  mean throughput.
- What it is not: a comparison across runs or time, a latency comparison, or a
  statistical test. Variance, sample count and confidence intervals are ignored.
  Pod faster than host gives a negative difference and always passes.
- **[added] Further defects of the mechanism:**
  - `doPerfDiff` (line 170) matches only on message size and stream count, **not on
    driver**, and overwrites as it goes (last match wins). With `--netperf --uperf`
    the comparison silently uses the last driver's values, and a host value from one
    driver can be paired with a pod value from another if one of them is missing.
  - If the pod-network value for a message size is missing, `pod = 0`, so
    `calDiff(host, 0) = 200 %` and the run fails for a reason unrelated to performance.
  - Combined with section 3, host and pod may have been measured on different node
    pairs, so the verdict confounds path with node pair.
- For our purposes the mechanism's lesson is negative: a verdict needs a declared
  comparison design, matched context, a variance estimate and an interval.

## 5. `--label` / `--annotation`

- Flags: `StringArrayVar`, repeatable (lines 1200–1201). Parsed by `parseLabels` /
  `parseAnnotations` (lines 765, 789): `KEY=VALUE` via `strings.Cut`, non-empty,
  `validation.IsQualifiedName` for keys, `IsValidLabelValue` for label values,
  duplicate keys rejected, tool-managed keys (`managedLabelKeys` `app`, `role`, line
  721; `managedAnnotationKeys`, line 713) rejected.
- Flow: `PerfScenarios.Labels/Annotations` (lines 303–304) →
  `DeploymentParams.WorkloadLabels/Annotations` → `newDeployment` (line 1747) puts them
  on the **pod template** only; `mergeLabels` (line 1822) applies managed labels last
  so selectors stay stable. Also passed to VMIs (`pkg/k8s/kubevirt.go` lines 319, 497,
  528–529). **[added]** Not on the Deployment object itself, not on Services, not on
  the namespace.
- **[added]** Reuse guard: `CreateDeployment` refuses to reuse an existing Deployment
  whose template lacks the requested labels/annotations
  (`deploymentTemplateMetadataMatches`, line 1646) — a good detail.
- **They do not reach the results.** No field in `result.Data`, `result.ScenarioResults`
  or `archive.Doc`, nothing in CSV, JSON or OpenSearch.
- Hard-coded: `sidecar.istio.io/inject: "true"` on every benchmark pod (line 1784),
  and the key is in `managedAnnotationKeys`, so the user cannot turn it off. With
  `--cpu/--memory`, `VerifyGuaranteedQoS` (line 1733) at least detects a sidecar that
  breaks Guaranteed QoS.

## 6. Where results go

- stdout: `tablewriter` tables (`ShowStreamResult`, `ShowRRResult`,
  `ShowLatencyResult`, `ShowSpecificResults`), or JSON with `--json` instead.
- CSV files (default on): `archive.WriteCSVResult` (`archive.go` line 445),
  `WritePromCSVResult`, `WriteSpecificCSV`, named `result-<unix-ts>.csv` in the CWD.
- JSON file `result-<unix-ts>.json` with `--json` (`writeJSONResult`, line 424), also
  echoed to stdout.
- OpenSearch with `--search` (`BuildDocs`, line 91), one document per result.
- All outputs contain **only aggregates** (mean throughput, mean of p99s, mean
  retransmits, throughput CI). No per-sample values, let alone per-transaction values,
  are written anywhere.
- **[added] Run identity is weaker than the pre-check said.** `--uuid` reaches only
  OpenSearch; the JSON file is built with `BuildDocs(r, "k8s-netperf")` (line 425), so
  its `uuid` field is the constant string `"k8s-netperf"`.
- **[added] No measurement window in any output.** `result.Data` has `StartTime` and
  `EndTime`, but `archive.Doc` has no such field; `Doc.Timestamp` is `time.Now()` at
  document-building time (line 92). Two runs cannot be shown to have overlapped from
  the files.
- Prometheus is **read**, not written: node CPU, OVS CPU/memory, top pods, discovered
  via go-commons `GetPrometheus()` with `SkipTLSVerify = true`
  (`pkg/metrics/system.go` `Discover`, line 88). **[added]** `NodeDetails` (line 94)
  takes the kernel from the **first** series of `kube_node_info` — an arbitrary node,
  stored once per run as `Metadata.Kernel` — and `NodeMTU` (line 137) takes the first
  series of `node_network_mtu_bytes`, an arbitrary interface on an arbitrary node.

## 7. Adopt / avoid

Adopt:

- **Strict parsing of `KEY=VALUE` flags**: Kubernetes validation, duplicate rejection
  and a protected set of tool-managed keys (`parseLabels`). Copy the behaviour, not the
  semantics (in k8s-netmeasure-rs `--label` is run metadata).
- **Managed labels applied last** (`mergeLabels`) so user labels can never break
  selectors.
- **Recording node context with every result** (`GetPodNodeInfo`: kernel, OS image,
  container runtime) — but per actual endpoint, and extended by zone, instance type and
  NIC data.
- **Explicit worker/infra/workload node filtering** for target selection.
- **Guaranteed-QoS option** (`--cpu`/`--memory`, `VerifyGuaranteedQoS`,
  `verifyPodCompute`) — relevant given our CFS-throttling experience. Including the
  check that admission did not silently change the requested resources.
- **Refusing to reuse objects whose spec differs from the request**
  (`deploymentComputeMatches`, `deploymentPodsCurrent`,
  `deploymentTemplateMetadataMatches`).

Avoid:

- Exec-based measurement through the API server, and an external benchmark binary
  whose output is parsed from text, with parse errors discarded.
- Averaging percentiles (across processes and across samples), percentiles of
  per-interval means, and integer truncation of latency.
- One column name for quantities that differ by driver.
- A verdict as a single percent threshold without variance, sample size, interval or
  matched context; a comparison that does not check that both sides come from the same
  driver and node pair.
- Inferring placement facts (`AcrossAZ`) instead of observing them; attaching one
  node-info record to results measured on other nodes.
- Variable shadowing of flag state (`acrossAZ`) — a reason to keep config in one typed
  struct and test the flag → behaviour path.
- `log.Fatal` on a single failed sample; no partial results; no timeouts on waits.
- No security context on pods (only optional `privileged`); hard-coded, non-overridable
  sidecar injection; `SkipTLSVerify` by default.
- Run identity and measurement window missing from the outputs.

Verdict: nothing in k8s-netperf measures `connect()` latency, its latency figures are
not percentiles of anything once parallelism > 1 or samples > 1, and its comparison
mechanism is not suitable for before/after verification. Use it as a reference for the
items listed under "Adopt"; build k8s-netmeasure-rs fresh.

---

## Appendix: goldpinger claims in the agent prompt, re-verified

Source: https://github.com/bloomberg/goldpinger, commit `e2023f2` (2026-10-05).
`pg/` = `pkg/goldpinger/`. Checks the "Known traps" statements and the chart remark
of the agent prompt.

| Prompt statement | Result | Evidence |
|---|---|---|
| TCP probe (`doTCPProbe`, `net.DialTimeout`) only for `--tcp-targets`, never peers | confirmed | `pg/probes.go:38-44`, called only from `checkTargets` with `TCPTargets` (`pg/client.go:222-228`) |
| Only Prometheus output is `goldpinger_tcp_errors_total` | confirmed | `pg/stats.go:106-115`; incremented only on error (`pg/client.go:240-242`) |
| Timed only for the `/check_all` JSON response, `time.Since(start).Milliseconds()` | **partly** | Integer ms confirmed (`pg/client.go:245`). The value is produced by **`/check`** (`CheckNeighbours` → `checkTargets`, `pg/client.go:45`) and only relayed per peer by `/check_all` (`pg/client.go:345-355`). It is also recorded on failure (time-to-error), and the probe runs per request, not on a schedule. |
| Dial includes DNS for hostname targets | confirmed | `start` taken before `net.DialTimeout` (`pg/client.go:238`) |
| Peer latency is an HTTP round trip, `goldpinger_peers_response_time_s` | confirmed | `pg/pinger.go:113-140`. Peers are addressed by IP (no DNS), plain HTTP (no TLS; README.md:297 claims TLS). Keep-alive transport, but the server's `IdleTimeout` (`--cleanup-timeout`, default 10 s, `pkg/restapi/server.go:71,208`) is shorter than the default 30 s ping period, so each ping most likely includes a fresh handshake (inferred from code, not measured). With UDP enabled, the UDP burst runs concurrently inside the timed HTTP window. |
| UDP probe `--udp-enabled`, `goldpinger_peers_udp_rtt_s`, mean of a burst (default 10), once per ping | confirmed | `pg/udp_probe.go:123,229-232`, `pg/pinger.go:165-167`. Additionally: all packets are sent before any reply is read (`pg/udp_probe.go:169-177`), so per-packet RTTs include burst queuing; lost packets are excluded from the mean. |
| UDP histogram starts at 100 µs | confirmed | `.0001 … 1` s (`pg/stats.go:152`) |
| HTTP histograms start at 5 ms | confirmed | `.005 … 30` s, hard-coded, no flag (`pg/stats.go:65,79`) |
| Chart hard-codes `runAsUser: 1000`, `fsGroup: 2000` | confirmed | `charts/goldpinger/values.yaml:146-151`; container context drops ALL, `allowPrivilegeEscalation: false`, `readOnlyRootFilesystem: true`, `runAsNonRoot: true` (`:136-142`); pod seccomp `RuntimeDefault` |

Also relevant for our design:

- Lifecycle: DaemonSet, long-running HTTP server; one goroutine per peer pinging with
  `wait.JitterUntil` (30 s, jitter 0.05) (`pg/updater.go:120`, `pg/pinger.go:228`).
- Discovery: periodic pod **List**, no watch, selector `app=goldpinger`,
  `status.phase=Running` (`pg/k8s.go:107-115`); pod IPs by default, host IPs with
  `--use-host-ip` (`pg/client.go:192-197`).
- **Probes target the measured port:** liveness and readiness are `httpGet /` on the
  same port as `/ping` (`charts/goldpinger/templates/daemonset.yaml:80-87`). Every probe
  is traffic on the measured endpoint — the pattern our prompt forbids for the listener.
