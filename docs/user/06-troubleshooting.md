# Troubleshooting

[User guide](README.md) · previous: [Running in Kubernetes and OpenShift](05-kubernetes-openshift.md)

## Exit codes of all subcommands

| Code | `run` | `listen` | `selftest` | `probe` |
|---|---|---|---|---|
| 0 | every cycle valid | — | every check established its result | every connect `ok` |
| 1 | internal error | internal error | internal error / report not writable | internal error |
| 2 | usage error | usage error | usage error | usage error |
| 3 | refused to start | port cannot be bound | — | — |
| 4 | at least one cycle invalid | — | — | — |
| 6 | — | — | a check was inconclusive | — |
| 7 | — | — | — | a connect was not `ok` |
| 130 / 143 | SIGINT / SIGTERM | SIGINT / SIGTERM | — | — |

## Why is a cycle invalid?

The reasons are in `measurement.cycles[].validity.reasons` and in the WARN log line.

| Reason | Usual cause | What to do |
|---|---|---|
| `cpu_throttled` | the measuring pod hit its CPU limit | raise the CPU limit; check other containers in the pod |
| `listener_drops` | an accept queue overflowed: own loopback listener or a target | check other load on the target; raise `--backlog` and `net.core.somaxconn` |
| `listener_drops_suspected` | the kernel does not expose per-socket drops, and the namespace counters rose | see `selftest` `listener-drop-indicator` |
| `target_set_changed` | a target restarted, its node rebooted, or its control port stopped answering | check the target pod; restart the run |
| `sample_count_short` | `local_error` or `anomaly` samples, or slots missing | see `errno` in the file; `EMFILE` → raise the fd limit; `EADDRNOTAVAIL` → ephemeral ports exhausted |
| `nic_changed` | a NIC, its driver or its ring sizes changed during the run | expected around host maintenance; repeat the run |
| `interrupted` | the run was stopped | — |

Annotations (`scheduling_jitter`, `pacing_overrun`, `netns_listen_drops`,
`nic_path_unknown`, `node_context_unavailable`, `node_listener_mismatch`,
`cpu_throttle_unchecked`, `target_unchecked`) do not invalidate a cycle; they explain conditions that may matter when results are compared.

## Common start errors

| Message | Cause | Fix |
|---|---|---|
| `CPU quota …m is below the floor of 500m` | pod CPU limit too small | limit ≥ 500m |
| `cgroup v2 cpu.max/cpu.stat unreadable` | cgroup v1 or an unusual sandbox | `--no-cgroup-check` (throttling then unchecked) |
| `target control port check failed` | the target is not a `listen`, or its control port is filtered | open 17701, or `--no-target-control` |
| `cannot write --output` | the directory does not exist | create it, or write to `/tmp` / a volume |
| `node listener … is not on this node` | `--node-context-from` points to another node's listener | use the node IP of the pod's own node (`status.hostIP`) |
| `node listener … sees no physical interface` | the node listener does not run with `hostNetwork` | start it with `hostNetwork` |
| `--control-bind auto: no interface with an IPv4 default route` | unusual network setup | set `--control-bind` explicitly |
| `Permission denied` on the report or result file | the working directory is not writable for the pod's UID | write to `/tmp` or a volume |
| `CreateContainerError` … `image not known` | the image reference is a `sha256-…` tag (signature/attestation), not the image | use `digest: sha256:…` or a `sha-<commit>` tag |
| `GLIBC_2.xx not found` | the dynamic build on an older host | use the static (`musl`) build |

## Reading a slow path

`probe --count 100` shows single connects. A spread of milliseconds with
`syn_retrans` 0 points at path latency (encapsulation, connection tracking,
hypervisor); about 1 s with `syn_retrans` 1 is a lost SYN. Compare pod-to-pod on the
same node (local datapath only), node-to-node (`hostNetwork`, NIC path only) and
pod-to-pod across nodes to see which layer adds the time.
