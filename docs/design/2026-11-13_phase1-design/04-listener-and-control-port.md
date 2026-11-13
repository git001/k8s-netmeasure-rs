# Phase 1 design — 04 Listener and control port

Part of the [Phase 1 design](README.md) · covers §4, §7 · previous: [Validity, CPU checks, selftest and probe](03-validity-selftest-probe.md) · next: [Result file](05-result-file.md)

---

## 4. Listener

### 4.1 Two ports per process

| Port | Flag (env) | Protocol | Purpose |
|---|---|---|---|
| **Measured port** | `--port` / `--bind` (`K8S_NETMEASURE_PORT`, `K8S_NETMEASURE_BIND`) | raw TCP: accept, close 64 connections later (§4.2), nothing is ever read or written | the only port whose handshakes are measured |
| **Control port** | `--control-port` / `--control-bind` (`K8S_NETMEASURE_CONTROL_PORT`, `K8S_NETMEASURE_CONTROL_BIND`) | HTTP/1.1 + JSON (§7) | probes, `/v1/*`, `/metrics` (Phase 3) |

The two ports must differ, and the same `bind:port` used twice is a configuration error
(exit 2). Separate bind addresses allow, for example, the measured port on all
interfaces and the control port on the pod IP only.

**Defaults per subcommand.** This avoids port conflicts when a `listen` DaemonSet and a
`run` Job share a node with `hostNetwork`. Overlay pods have their own netns and cannot
conflict.

| Subcommand | Measured port | Control port |
|---|---|---|
| `listen` | `0.0.0.0:17700` | `<pod IP>:17701` |
| `run` | `127.0.0.1:0` — loopback control target only, ephemeral port, never reachable from outside | **off by default**; on only with `--control-port` (suggested 17702) |
| `serve` (Phase 3) | `0.0.0.0:17700` | `<pod IP>:17701` |
| `selftest`, `probe`, `report` | — (selftest: own loopback listeners only) | none |

**Control port of `run` is opt-in (review round 1).** A measurement must never fail
because an administrative port is taken. Several `run` processes with `hostNetwork`
on one node would collide on a fixed port. Without `--control-port`, `run` has no
HTTP server and therefore no Kubernetes probes. The Phase 2 chart sets the flag for
its overlay Job, where pods cannot collide. A port that is set explicitly and cannot be
bound is still exit 3: an explicit request is never silently ignored.

**Control bind defaults to the pod IP, not `0.0.0.0` (review round 1).**
- `--control-bind` default `auto` means: the IPv4 address of the interface that holds
  the default route. In a pod that is the pod IP; with `hostNetwork` it is the node's
  primary address.
- The Phase 2 chart passes the Downward API's `status.podIP` explicitly.
- Kubelet probes target the pod IP, so they work unchanged. `127.0.0.1` and other
  interfaces are not served, so `oc port-forward` (which connects to localhost in the
  pod) needs an explicit `--control-bind 0.0.0.0`; the README says so.
- With `hostNetwork` this does not close the port to the cluster, because pod IP and
  node IP are the same there. NetworkPolicy does **not** apply to hostNetwork pods, so
  in the underlay variant `/v1/context` stays readable for anyone who can reach the
  node. Decision: accepted, documented in the README (no token authentication).

**Start order.**
1. Self-checks.
2. Bind the measured port and start the accept thread.
3. Collect the initial context.
4. Bind the control port.

`/startup` and `/ready` report `200` only after all four steps (§7). If any bind fails,
the process refuses to start (exit 3).

### 4.2 Measured listener

- Binds `--bind:--port` with `SO_REUSEADDR`, non-blocking, backlog `--backlog`
  (default 4096); the effective backlog `min(backlog, somaxconn)` is recorded.
- Dedicated accept thread: `poll` on the listening socket with a 1 s timeout, so it can
  update its liveness heartbeat (§7) while idle; then `accept4(SOCK_CLOEXEC)`
  until `EAGAIN`. An accepted connection is **not** closed at once but kept in a ring
  of the last 64 and closed when it drops out (decided after the first cluster
  `selftest`): an immediate close sends a FIN that can reach the client before it
  reads `TCP_INFO` (`CLOSE_WAIT`, §3.1). The client resets the connection right after
  the sample (`SO_LINGER=0`), so a held connection costs one fd and no traffic.
  `ECONNABORTED` is expected, because the client may RST before accept, and is counted
  but not treated as an error. Counters: accepted, aborted.
- Never set `TCP_DEFER_ACCEPT` or `TCP_FASTOPEN`: both change what the handshake looks
  like to the client.
- `SK_MEMINFO_DROPS` (index 8 of the `SO_MEMINFO` array) is read by the main thread before
  and after each cycle. **[addition]** `TcpExt.SyncookiesSent` goes into the netstat
  diagnostic set: SYN-backlog overflow (as opposed to accept-queue overflow) shows up
  there and in no other counter.

---

## 7. Control port: HTTP/1.1 + JSON

No custom protocol. The control port (`--control-port`; `listen` default 17701, `run`
opt-in, §4.1) is a plain HTTP/1.1 server returning JSON. It is separate from the measured port and is never
used to measure anything. In Phase 3 the same server also serves `/metrics`: **one
port, one protocol**. That matches the prompt's "`/ready` next to `/metrics`".

| Endpoint | Response |
|---|---|
| `GET /startup` | startup probe: `200` once initialisation has finished (config validated, self-checks passed, measured listener bound, initial node context collected); `503` before. Once `200`, it stays `200` for the life of the process |
| `GET /live` | liveness probe: `200` unless an internal thread is stalled (watchdog, below). **Never depends on the network**, on target reachability or on cycle validity. Stays `200` during graceful shutdown |
| `GET /ready` | readiness probe: `200` once startup has finished and the measured listener is accepting; `503` before that and from the first shutdown signal on, so Kubernetes stops routing to a terminating listener |
| `GET /v1/info` | build info, `listener_instance` (ULID created at start), measured port, effective backlog |
| `GET /v1/context` | node context (§5) |
| `GET /v1/snapshot` | monotonic timestamp, `boot_id`, `listener_instance`, own `SK_MEMINFO_DROPS`, accepted/aborted counts, netns `ListenOverflows/ListenDrops`, own cgroup cpu.stat; **NIC path** (§3.6: per physical interface present/driver, default-route interface); underlay: netstat and ethtool allowlist |
| `GET /metrics` | Phase 3 only |

- Versioning is in the path (`/v1/`). Response bodies are the `schema/control.rs`
  types; the client parses them with `deny_unknown_fields` (§9.1).
- Strictness: an unknown path returns `404`, any method other than `GET` returns `405`,
  and any query parameter returns `400`. That is the "unknown parameters are hard
  failures" rule applied to HTTP.
- All three Kubernetes probes are `httpGet` on the control port. Probes never touch
  the measured port. Manual check: `curl http://<host>:17701/v1/context`.
- **The control server always runs in `listen`** (and in `serve` in Phase 3). In `run`
  it runs only with `--control-port`; a long `run` Job that wants startup and liveness
  probes sets it. `run`'s own measured listener is loopback-only (§4.1), so `run` is
  never a target for others. `report`, `selftest` and `probe` have no server.
- **Liveness watchdog.** Every long-lived thread stores a monotonic heartbeat in an
  atomic after each unit of work. The measuring thread does so per sample; the accept
  thread waits in `poll` with a 1 s timeout so that it can beat while idle; load
  threads per connect; the main thread per loop iteration. `/live` returns `503` if a
  heartbeat is older than that thread's bound:
  - measuring and load threads: `2 × connect timeout + interval` (default ≈ 10 s);
  - accept thread: 5 s;
  - main thread: `cycle duration + snapshot timeouts + 30 s`.

  A stalled thread is the only failure a restart can fix. A bad network must never
  restart the pod, because that would destroy the evidence.
- Recommended probe settings (go into the Phase 2 chart):
  - `startupProbe`: period 2 s, `failureThreshold` 30 (60 s budget, covers context
    collection and ethtool reads on a busy node).
  - `livenessProbe`: period 10 s, `failureThreshold` 3.
  - `readinessProbe`: period 5 s, `failureThreshold` 1, so a terminating listener drops
    out of rotation within one period.

  All probes use `timeoutSeconds: 2`.
- `run` keeps one HTTP keep-alive connection per target for the duration of the run.
  If a request fails and a reconnect fails too, the result is `target_set_changed`.
  Timeouts: 2 s per request.
- The control port discloses node context, such as the kernel version, to anyone who
  can reach it. It binds to the pod IP by default (§4.1), and the Phase 2 chart adds a
  NetworkPolicy for the overlay. In the underlay variant NetworkPolicy does not apply
  to hostNetwork pods; that exposure is accepted and documented.

**Implementation: hyper on an isolated tokio runtime.**

- `listen` (and `run` with `--control-port`) runs one dedicated **control thread** with a `current_thread`
  tokio runtime and a hyper 1.x server. It answers probes and `/v1/*`, and only reads
  atomics and preassembled snapshots, so it never blocks other threads.
- In `run`, the main thread additionally holds its own `current_thread` runtime for the
  hyper client and calls it via `block_on`, between cycles only.
- The measuring, accept and load threads never touch tokio. Signals stay with
  `signal-hook` flags, not `tokio::signal`, so shutdown behaves identically in every
  thread.
- Reason for tokio already in Phase 1: Phase 2's `kube` client is async and Phase 3's
  `/metrics` needs this same server. Introducing the runtime now, confined to one
  thread, avoids a rewrite later.
