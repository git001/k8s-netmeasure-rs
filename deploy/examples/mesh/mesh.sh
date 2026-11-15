#!/usr/bin/env bash
# k8s-netmeasure mesh: every node measures every other node, without hand-edited
# manifests. See README.md next to this script and docs/user/05-kubernetes-openshift.md.
#
#   IMAGE=ghcr.io/git001/k8s-netmeasure-rs@sha256:<digest> ./mesh.sh deploy
#   ./mesh.sh inventory
#   IMAGE=… ./mesh.sh series before 5        # 5 rounds, overlay and underlay alternating
#   ./mesh.sh cleanup                        # jobs only; `cleanup --all` deletes the namespace
#
# Environment:
#   IMAGE          image reference, preferably by digest (deploy, round, series)
#   NODE_SELECTOR  extra node labels, comma-separated key=value (default: none);
#                  node-role.kubernetes.io/worker="" is always required
#   TOLERATIONS    JSON array of tolerations for tainted nodes (default: [])
#   SAMPLES        counted samples per target and run (default: 1000)
#   OUT            local result directory (default: ./mesh-results)
#   TIMEOUT_S      seconds to wait for one round (default: 900)
#   OC             client binary (default: oc; kubectl works as well)
#
# Never expose `listen` to the internet; this script creates no Service, Route or
# Ingress.
set -euo pipefail

NS=netmeasure
NODE_SELECTOR=${NODE_SELECTOR:-}
TOLERATIONS=${TOLERATIONS:-[]}
SAMPLES=${SAMPLES:-1000}
OUT=${OUT:-./mesh-results}
TIMEOUT_S=${TIMEOUT_S:-900}
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

cmd_deploy() {
  need_image
  $OC apply -f "$LISTEN_DIR/namespace.yaml"
  $OC kustomize "$LISTEN_DIR" |
    sed "s|image: ghcr.io/git001/k8s-netmeasure-rs.*|image: $IMAGE|" |
    $OC apply -f -
  local ds patch
  patch="{\"spec\":{\"template\":{\"spec\":{\"nodeSelector\":$(selector_json),\"tolerations\":$TOLERATIONS}}}}"
  for ds in netmeasure-listen-underlay netmeasure-listen-overlay; do
    $OC -n "$NS" patch ds "$ds" --type=merge -p "$patch"
  done
  for ds in netmeasure-listen-underlay netmeasure-listen-overlay; do
    $OC -n "$NS" rollout status "ds/$ds" --timeout=300s
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

cmd_inventory() {
  local node ip pod
  while read -r node ip; do
    pod=$($OC -n "$NS" get pods -l app.kubernetes.io/component=listen-underlay \
      --field-selector "spec.nodeName=$node" -o name | head -n1)
    printf '%s ' "$node"
    $OC -n "$NS" exec "$pod" -- curl -s "http://$ip:17701/v1/context"
    echo
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
            requests: {cpu: 500m, memory: 256Mi}
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

cmd_round() { # path phase round
  need_image
  local path=$1 phase=$2 round=$3 node ip i=0 name start pod done_n
  [[ $phase =~ ^[a-z0-9]([a-z0-9-]{0,18}[a-z0-9])?$ ]] || die "phase must be lowercase letters, digits and '-', at most 20 characters"
  [[ $round =~ ^[0-9]{1,3}$ ]] || die "round must be a number"
  local -a nodes=() ips=() names=()
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
  done
  start=$SECONDS
  while :; do
    done_n=0
    for name in "${names[@]}"; do
      pod=$($OC -n "$NS" get pods -l "job-name=$name" -o name 2>/dev/null | head -n1)
      [[ -n $pod ]] && $OC -n "$NS" exec "$pod" -- test -f /results/exit-code 2>/dev/null && done_n=$((done_n + 1))
    done
    ((done_n == ${#names[@]})) && break
    ((SECONDS - start > TIMEOUT_S)) && { log "timeout: $done_n of ${#names[@]} finished"; break; }
    sleep 10
  done
  local dir rc
  for name in "${names[@]}"; do
    pod=$($OC -n "$NS" get pods -l "job-name=$name" -o name 2>/dev/null | head -n1)
    node=$($OC -n "$NS" get "$pod" -o jsonpath='{.spec.nodeName}' 2>/dev/null || echo unknown)
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
  inventory) cmd_inventory ;;
  round) [[ $# -eq 4 ]] || die "usage: mesh.sh round overlay|underlay <phase> <round>"; cmd_round "$2" "$3" "$4" ;;
  series) [[ $# -eq 3 ]] || die "usage: mesh.sh series <phase> <rounds>"; cmd_series "$2" "$3" ;;
  cleanup) cmd_cleanup "${2:-}" ;;
  *) die "usage: mesh.sh deploy | inventory | round <path> <phase> <n> | series <phase> <rounds> | cleanup [--all]" ;;
esac
