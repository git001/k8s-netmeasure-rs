#!/usr/bin/env bash
# k8s-netmeasure mesh: every node measures every other node, without hand-edited
# manifests. See README.md next to this script and docs/user/05-kubernetes-openshift.md.
#
#   IMAGE=ghcr.io/git001/k8s-netmeasure-rs@sha256:<digest> ./mesh.sh deploy
#   ./mesh.sh nodes                          # candidate nodes with zone and instance type
#   ./mesh.sh inventory
#   IMAGE=… ./mesh.sh series before 5        # 5 rounds, overlay and underlay alternating
#   ./mesh.sh cleanup                        # jobs only; `cleanup --all` deletes the namespace
#
# Environment:
#   IMAGE          image reference, preferably by digest (deploy, round, series)
#   NODE_SELECTOR  extra node labels, comma-separated key=value (default: none);
#                  node-role.kubernetes.io/worker="" is always required
#   NODES          only these nodes, comma-separated node names (default: every node
#                  matching NODE_SELECTOR); e.g. two per zone and machine type
#   TOLERATIONS    JSON array of tolerations for tainted nodes (default: [])
#   SAMPLES        counted samples per target and run (default: 1000)
#   OUT            local result directory (default: ./mesh-results)
#   TIMEOUT_S      seconds to wait for one round (default: 900)
#   PENDING_S      give up on a Job pod still Pending after this many seconds, e.g.
#                  `Insufficient cpu` on a full node (default: 120)
#   RUN_CPU_REQUEST  CPU request of a run pod (default: 250m); the limit stays 1 CPU,
#                  which is the quota run checks (>= 500m)
#   OC             client binary (default: oc; kubectl works as well)
#
# Never expose `listen` to the internet; this script creates no Service, Route or
# Ingress.
set -euo pipefail

NS=netmeasure
NODE_SELECTOR=${NODE_SELECTOR:-}
NODES=${NODES:-}
TOLERATIONS=${TOLERATIONS:-[]}
SAMPLES=${SAMPLES:-1000}
OUT=${OUT:-./mesh-results}
TIMEOUT_S=${TIMEOUT_S:-900}
PENDING_S=${PENDING_S:-120}
RUN_CPU_REQUEST=${RUN_CPU_REQUEST:-250m}
OC=${OC:-oc}
HERE=$(cd "$(dirname "$0")" && pwd)
LISTEN_DIR=$HERE/../listen
MESH_LABEL=app.kubernetes.io/part-of=k8s-netmeasure-mesh

log() { echo "mesh: $*" >&2; }
die() { log "$*"; exit 1; }
need_image() { [[ -n ${IMAGE:-} ]] || die "set IMAGE (e.g. ghcr.io/git001/k8s-netmeasure-rs@sha256:…)"; }

# {"node-role.kubernetes.io/worker":"","key":"value",…}
selector_json() {
  local out='"node-role.kubernetes.io/worker":""' kv kvs
  IFS=',' read -ra kvs <<<"$NODE_SELECTOR"
  for kv in "${kvs[@]}"; do
    [[ -z $kv ]] && continue
    [[ $kv == *=* ]] || die "NODE_SELECTOR: '$kv' is not key=value"
    out+=",\"${kv%%=*}\":\"${kv#*=}\""
  done
  printf '{%s}' "$out"
}

# Node affinity on metadata.name for NODES; `null` removes an earlier one.
affinity_json() {
  local names=() n
  IFS=',' read -ra names <<<"$NODES"
  local values=''
  for n in "${names[@]}"; do
    [[ -z $n ]] && continue
    values+="${values:+,}\"$n\""
  done
  if [[ -z $values ]]; then
    printf 'null'
  else
    printf '{"nodeAffinity":{"requiredDuringSchedulingIgnoredDuringExecution":{"nodeSelectorTerms":[{"matchFields":[{"key":"metadata.name","operator":"In","values":[%s]}]}]}}}' "$values"
  fi
}

# Label selector for `oc get nodes`: worker role plus NODE_SELECTOR.
selector_arg() {
  printf 'node-role.kubernetes.io/worker%s' "${NODE_SELECTOR:+,$NODE_SELECTOR}"
}

# Candidates to pick NODES from: zone and instance type from the node labels.
cmd_nodes() {
  $OC get nodes -l "$(selector_arg)" \
    -L topology.kubernetes.io/zone -L node.kubernetes.io/instance-type
}

cmd_deploy() {
  need_image
  $OC apply -f "$LISTEN_DIR/namespace.yaml"
  $OC kustomize "$LISTEN_DIR" |
    sed "s|image: ghcr.io/git001/k8s-netmeasure-rs.*|image: $IMAGE|" |
    $OC apply -f -
  local ds patch
  patch="{\"spec\":{\"template\":{\"spec\":{\"nodeSelector\":$(selector_json),\"tolerations\":$TOLERATIONS,\"affinity\":$(affinity_json)}}}}"
  for ds in netmeasure-listen-underlay netmeasure-listen-overlay; do
    $OC -n "$NS" patch ds "$ds" --type=merge -p "$patch"
  done
  for ds in netmeasure-listen-underlay netmeasure-listen-overlay; do
    $OC -n "$NS" rollout status "ds/$ds" --timeout=300s
  done
  # A misspelt name, or a node NODE_SELECTOR or a taint excludes, gets no listener.
  local n names=()
  IFS=',' read -ra names <<<"$NODES"
  for n in "${names[@]}"; do
    [[ -z $n ]] && continue
    listeners underlay | awk -v n="$n" '$1 == n {f = 1} END {exit !f}' ||
      log "WARNING: no ready listener on '$n' (name, NODE_SELECTOR or taint?)"
  done
}

# "<node> <ip>" for every ready listener of the path (overlay: pod IP, underlay:
# node IP), sorted by node.
listeners() {
  local component field
  case $1 in
    overlay) component=listen-overlay field=podIP ;;
    underlay) component=listen-underlay field=hostIP ;;
    *) die "path must be overlay or underlay, not '$1'" ;;
  esac
  $OC -n "$NS" get pods -l "app.kubernetes.io/component=$component" \
    -o jsonpath="{range .items[*]}{.spec.nodeName}{\" \"}{.status.$field}{\" \"}{.status.containerStatuses[0].ready}{\"\\n\"}{end}" |
    awk '$3 == "true" && $2 != "" {print $1, $2}' | sort
}

# One line per node: kernel, NIC drivers with ring sizes (raw JSON without jq).
cmd_inventory() {
  local node ip pod json labels
  while read -r node ip; do
    pod=$($OC -n "$NS" get pods -l app.kubernetes.io/component=listen-underlay \
      --field-selector "spec.nodeName=$node" -o name | head -n1)
    json=$($OC -n "$NS" exec "$pod" -- curl -s "http://$ip:17701/v1/context")
    if command -v jq >/dev/null; then
      labels=$($OC get node "$node" -o jsonpath='{.metadata.labels.topology\.kubernetes\.io/zone}{" "}{.metadata.labels.node\.kubernetes\.io/instance-type}')
      jq -r --arg node "$node" --arg labels "$labels" '[$node, ($labels | split(" ") | .[]), .kernel_release.value,
        ((.nic.value.interfaces // []) | map("\(.driver) \(.rings.rx // "-")/\(.rings.tx // "-")") | join(", "))]
        | @tsv' <<<"$json"
    else
      printf '%s %s\n' "$node" "$json"
    fi
  done < <(listeners underlay)
}

job_manifest() { # name path phase round node targets…
  local name=$1 path=$2 phase=$3 round=$4 node=$5
  shift 5
  local hostnet=false sa=default extra='' t
  if [[ $path == underlay ]]; then
    hostnet=true sa=netmeasure-underlay
  else
    # $(HOST_IP) is expanded by Kubernetes from the pod's env, not by this shell.
    # shellcheck disable=SC2016
    extra='            - --node-context-from=$(HOST_IP):17701'
  fi
  cat <<EOF
apiVersion: batch/v1
kind: Job
metadata:
  name: $name
  namespace: $NS
  labels:
    app.kubernetes.io/name: k8s-netmeasure
    app.kubernetes.io/part-of: k8s-netmeasure-mesh
    app.kubernetes.io/component: run-$path
spec:
  backoffLimit: 0
  activeDeadlineSeconds: $((TIMEOUT_S + 600))
  template:
    metadata:
      labels:
        app.kubernetes.io/name: k8s-netmeasure
        app.kubernetes.io/part-of: k8s-netmeasure-mesh
        app.kubernetes.io/component: run-$path
    spec:
      restartPolicy: Never
      serviceAccountName: $sa
      automountServiceAccountToken: false
      hostNetwork: $hostnet
      dnsPolicy: ClusterFirstWithHostNet
      terminationGracePeriodSeconds: 30
      nodeSelector: {"kubernetes.io/hostname": "$node"}
      tolerations: $TOLERATIONS
      securityContext:
        runAsNonRoot: true
        seccompProfile: {type: RuntimeDefault}
      containers:
        - name: run
          image: $IMAGE
          # The pod stays up after the run so that the result can be copied out;
          # mesh.sh deletes it afterwards.
          command: ["/bin/sh", "-c", 'k8s-netmeasure "\$@"; echo \$? > /results/exit-code; sleep 3600', "k8s-netmeasure"]
          args:
            - run
            - --samples=$SAMPLES
            - --path=$path
            - --label=phase=$phase
            - --label=round=$round
            - --output=/results/
$extra
EOF
  for t in "$@"; do echo "            - --target=$t:17700"; done
  cat <<EOF
          env:
            - name: NODE_NAME
              valueFrom: {fieldRef: {fieldPath: spec.nodeName}}
            - name: HOST_IP
              valueFrom: {fieldRef: {fieldPath: status.hostIP}}
          resources:
            requests: {cpu: $RUN_CPU_REQUEST, memory: 256Mi}
            limits: {cpu: "1", memory: 256Mi}
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities: {drop: ["ALL"]}
          volumeMounts:
            - {name: results, mountPath: /results}
      volumes:
        - {name: results, emptyDir: {}}
EOF
}

# The scheduler's last message for a pending pod, e.g. "0/9 nodes are available: …".
pending_reason() {
  $OC -n "$NS" get events --field-selector "involvedObject.name=${1#pod/},reason=FailedScheduling" \
    -o jsonpath='{.items[-1:].message}' 2>/dev/null || echo unknown
}

cmd_round() { # path phase round
  need_image
  local path=$1 phase=$2 round=$3 node ip i=0 name start pod done_n
  [[ $phase =~ ^[a-z0-9]([a-z0-9-]{0,18}[a-z0-9])?$ ]] || die "phase must be lowercase letters, digits and '-', at most 20 characters"
  [[ $round =~ ^[0-9]{1,3}$ ]] || die "round must be a number"
  local -a nodes=() ips=() names=()
  local -A job_node=() gave_up=()
  while read -r node ip; do nodes+=("$node") ips+=("$ip"); done < <(listeners "$path")
  if [[ $path == overlay ]]; then # an overlay source needs its node listener
    local -A under=()
    while read -r node ip; do under[$node]=1; done < <(listeners underlay)
  fi
  ((${#nodes[@]} >= 2)) || die "$path: fewer than two ready listeners"
  log "$path round $round ($phase): ${#nodes[@]} nodes, $((${#nodes[@]} - 1)) targets each"
  for i in "${!nodes[@]}"; do
    node=${nodes[$i]}
    if [[ $path == overlay && -z ${under[$node]:-} ]]; then
      log "skip $node: no ready node listener"
      continue
    fi
    local -a targets=()
    for j in "${!ips[@]}"; do ((j == i)) || targets+=("${ips[$j]}"); done
    name=$(printf 'nm-%s-%s-r%s-n%02d' "${path:0:2}" "$phase" "$round" "$i")
    job_manifest "$name" "$path" "$phase" "$round" "$node" "${targets[@]}" | $OC apply -f - >/dev/null
    names+=("$name")
    job_node[$name]=$node
  done
  start=$SECONDS
  while :; do
    done_n=0
    for name in "${names[@]}"; do
      if [[ -n ${gave_up[$name]:-} ]]; then
        done_n=$((done_n + 1))
        continue
      fi
      pod=$($OC -n "$NS" get pods -l "job-name=$name" -o name 2>/dev/null | head -n1)
      [[ -z $pod ]] && continue
      if $OC -n "$NS" exec "$pod" -- test -f /results/exit-code 2>/dev/null; then
        done_n=$((done_n + 1))
      elif ((SECONDS - start > PENDING_S)) &&
        [[ $($OC -n "$NS" get "$pod" -o jsonpath='{.status.phase}' 2>/dev/null) == Pending ]]; then
        log "${job_node[$name]}: gave up, pod still Pending after ${PENDING_S}s: $(pending_reason "$pod")"
        gave_up[$name]=1
        $OC -n "$NS" delete job "$name" --wait=false >/dev/null
        done_n=$((done_n + 1))
      fi
    done
    ((done_n == ${#names[@]})) && break
    ((SECONDS - start > TIMEOUT_S)) && { log "timeout: $done_n of ${#names[@]} finished"; break; }
    sleep 10
  done
  local dir rc
  for name in "${names[@]}"; do
    [[ -n ${gave_up[$name]:-} ]] && continue
    pod=$($OC -n "$NS" get pods -l "job-name=$name" -o name 2>/dev/null | head -n1)
    node=${job_node[$name]}
    dir=$OUT/$phase/$path/round-$round/$node
    mkdir -p "$(dirname "$dir")"
    # oc cp creates $dir itself (it must not exist yet) with the pod's /results.
    if [[ -n $pod ]] && $OC -n "$NS" cp "${pod#pod/}:/results" "$dir" >/dev/null 2>&1; then
      rc=$(cat "$dir/exit-code" 2>/dev/null || echo none)
    else
      rc=no-result
      mkdir -p "$dir"
      [[ -n $pod ]] && { $OC -n "$NS" logs "$pod" >"$dir/run.log" 2>&1 || true; }
    fi
    log "$node: exit $rc"
    $OC -n "$NS" delete job "$name" --wait=false >/dev/null
  done
}

cmd_series() { # phase rounds
  local phase=$1 rounds=$2 r
  [[ $rounds =~ ^[0-9]+$ ]] || die "rounds must be a number"
  for ((r = 1; r <= rounds; r++)); do
    cmd_round overlay "$phase" "$r"
    cmd_round underlay "$phase" "$r"
  done
  log "results in $OUT/$phase"
}

cmd_cleanup() {
  $OC -n "$NS" delete jobs -l "$MESH_LABEL" --ignore-not-found
  if [[ ${1:-} == --all ]]; then $OC delete namespace "$NS" --ignore-not-found; fi
}

case ${1:-} in
  deploy) cmd_deploy ;;
  nodes) cmd_nodes ;;
  inventory) cmd_inventory ;;
  round) [[ $# -eq 4 ]] || die "usage: mesh.sh round overlay|underlay <phase> <round>"; cmd_round "$2" "$3" "$4" ;;
  series) [[ $# -eq 3 ]] || die "usage: mesh.sh series <phase> <rounds>"; cmd_series "$2" "$3" ;;
  cleanup) cmd_cleanup "${2:-}" ;;
  *) die "usage: mesh.sh nodes | deploy | inventory | round <path> <phase> <n> | series <phase> <rounds> | cleanup [--all]" ;;
esac
