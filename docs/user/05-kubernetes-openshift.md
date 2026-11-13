# Running in Kubernetes and OpenShift

[User guide](README.md) · previous: [`run`](04-run.md) · next: [Troubleshooting](06-troubleshooting.md)

Example manifests (`listen` DaemonSet for both paths, `run` Job) *follow*. Until then
the patterns below work with plain `kubectl` / `oc`.

## Which pod for which path

| Path | Pod | Notes |
|---|---|---|
| overlay | a normal pod | runs under the default restricted policy; random UID is fine |
| underlay | a pod with `hostNetwork: true` | OpenShift: SCC `hostnetwork-v2` for its service account; Kubernetes: a namespace with Pod Security level `privileged`, or `kubectl debug node/<node>` |

Give the measuring pod a **CPU limit of at least 500m** (`run` refuses less) and run
`selftest` in the same kind of pod first.

## Quick ad hoc runs without manifests

The static binary runs in any pod. Copy it in and run it:

```sh
# overlay: a short-lived pod with a CPU limit, under the restricted SCC
oc run nm --rm -it --image=<any image with a shell> \
  --override-type=strategic \
  --overrides='{"metadata":{"annotations":{"openshift.io/required-scc":"restricted-v2"}},
    "spec":{"containers":[{"name":"nm","resources":{"limits":{"cpu":"1","memory":"256Mi"}}}]}}' \
  --command -- /bin/bash
oc cp k8s-netmeasure nm:/tmp/k8s-netmeasure
```

- `--override-type=strategic` merges the container by name, so `--image` stays; with
  the default merge type the containers list would be replaced.
- When an admin creates the pod, OpenShift otherwise picks an SCC the admin may use
  (often `anyuid`); `openshift.io/required-scc` enforces the one a real measuring pod
  gets.
- Write result files to `/tmp` or a volume: the working directory is usually not
  writable for the random UID.

```sh
# underlay: a debug pod on the node (hostNetwork, privileged)
oc debug node/<node>
```

In a debug pod the node's root file system is under `/host`; a file copied into the
debug pod with `oc cp` lands in the pod, not on the host. The debug pod is
privileged and runs as root: fine for `selftest` and `probe`, but its CPU and cgroup
values are not those of a normal measuring pod.

## Node name and context

Pass the node name through the Downward API so result files name nodes, not IPs:

```yaml
env:
  - name: NODE_NAME
    valueFrom: { fieldRef: { fieldPath: spec.nodeName } }
```

`listen` and `run` use `$NODE_NAME` unless `--node-name` is given. Zone and instance
type are declared with `--zone` / `--instance-type` (Phase 1 reads no API).

## Security

> [!WARNING]
> **`listen` has only minimal protection. Never expose it to the internet.**

- Neither port has authentication, TLS, rate limiting or an allowlist.
- Anyone who reaches the measured port can fill its accept queue and so falsify
  measurements (they show up as `listener_drops`, but the run is lost).
- The control port discloses node details: kernel, boot id, CPU, NIC drivers and ring
  sizes, cgroup limits.
- With `hostNetwork`, NetworkPolicy does not apply, and the ports are open on the
  node address. Cloud security groups of worker nodes often allow all ingress by
  default; check them.

Therefore:

- run `listen` only inside the cluster network and only for the duration of a test
  series;
- restrict access: NetworkPolicy for overlay pods, node firewall or security groups
  for `hostNetwork` pods;
- never create a `LoadBalancer` or `NodePort` Service, Route or Ingress for it;
- `oc port-forward` needs `--control-bind 0.0.0.0`; use it only temporarily.

## Other managed Kubernetes services

Nothing in the tool is OpenShift-specific: it uses Linux system calls, `/proc`, `/sys`
and TCP. On another distribution, run `selftest` first on every node type: kernel
fields and counters differ between kernels, and the node's cgroup version, CPU
architecture (x86_64 builds only) and CNI matter.
