#!/bin/bash
# bench.sh <label> <app container> <url> <host header> [auth header] [seconds] [concurrency]
# Fixed-duration closed-loop load with hey; CPU from the container's cgroup counter so the
# number is CPU-milliseconds actually burned per request, independent of latency noise.
L=$1; C=$2; URL=$3; HOST=$4; AUTH=${5:-}; SECS=${6:-20}; CONC=${7:-8}
HEY=$HOME/go/bin/hey
hdr=(); if [ -n "$AUTH" ]; then IFS="|" read -ra parts <<< "$AUTH"; for h in "${parts[@]}"; do hdr+=(-H "$h"); done; fi
$HEY -z 3s -c $CONC -host "$HOST" "${hdr[@]}" "$URL" >/dev/null 2>&1     # warm-up
cpu0=$(docker exec $C cat /sys/fs/cgroup/cpu.stat | awk '/^usage_usec/{print $2}')
g0=$(docker exec galileo-server cat /sys/fs/cgroup/cpu.stat | awk '/^usage_usec/{print $2}')
out=$($HEY -z ${SECS}s -c $CONC -host "$HOST" "${hdr[@]}" "$URL" 2>&1)
cpu1=$(docker exec $C cat /sys/fs/cgroup/cpu.stat | awk '/^usage_usec/{print $2}')
g1=$(docker exec galileo-server cat /sys/fs/cgroup/cpu.stat | awk '/^usage_usec/{print $2}')
mem=$(docker stats --no-stream --format '{{.MemUsage}}' $C | cut -d/ -f1 | tr -d ' ')
rps=$(echo "$out" | awk '/Requests\/sec/{print $2}')
p50=$(echo "$out" | awk '/50%% in/{print $3}'); p95=$(echo "$out" | awk '/95%% in/{print $3}'); p99=$(echo "$out" | awk '/99%% in/{print $3}')
codes=$(echo "$out" | awk '/responses/{printf "%s:%s ", $1, $2}' | tr -d '[]')
n=$(echo "$out" | awk '/responses/{s+=$2} END{print s}')
python3 - "$L" "$rps" "$p50" "$p95" "$p99" "$cpu0" "$cpu1" "$g0" "$g1" "$n" "$mem" "$codes" <<'PY'
import sys,json
L,rps,p50,p95,p99,c0,c1,g0,g1,n,mem,codes=sys.argv[1:]
n=int(n or 0); cpu_ms=(int(c1)-int(c0))/1000; gal_ms=(int(g1)-int(g0))/1000
row={"label":L,"rps":float(rps or 0),"p50_ms":float(p50 or 0)*1000,"p95_ms":float(p95 or 0)*1000,"p99_ms":float(p99 or 0)*1000,
     "requests":n,"app_cpu_ms_per_req":round(cpu_ms/n,3) if n else None,"galileo_cpu_ms_per_req":round(gal_ms/n,3) if n else None,"mem":mem,"codes":codes.strip()}
print(json.dumps(row)); open("bench-results.jsonl","a").write(json.dumps(row)+"\n")
PY
