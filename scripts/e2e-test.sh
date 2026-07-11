#!/usr/bin/env bash
# End-to-end test: unmodified VPP loads the Rust rateguard plugin and
# rate-limits a 100-packet burst from one source to burst=10 allowed.
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
VPP_PREFIX="${VPP_PREFIX:-$(readlink -f "$DIR/../vpp/build-root/install-oxidize/vpp")}"
VPP="$VPP_PREFIX/bin/vpp"
SOCK="${VPP_SOCK:-/tmp/vpp-oxidize-test-cli.sock}"
VPPCTL="$VPP_PREFIX/bin/vppctl -s $SOCK"
WORK="$(mktemp -d)"
cleanup() {
  rc=$?
  kill "$VPP_PID" 2>/dev/null || true
  if [ $rc -ne 0 ] && [ -f "$WORK/vpp.log" ]; then
    echo "--- vpp.log (last 20 lines) ---" >&2
    tail -20 "$WORK/vpp.log" >&2
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

make -C "$DIR" plugins >/dev/null

cat > "$WORK/startup.conf" <<EOF
unix { nodaemon cli-listen $SOCK }
socksvr { socket-name $WORK/api.sock }
plugins { path $VPP_PREFIX/lib/x86_64-linux-gnu/vpp_plugins:$DIR/target/plugins }
EOF

"$VPP" -c "$WORK/startup.conf" > "$WORK/vpp.log" 2>&1 &
VPP_PID=$!

for i in $(seq 1 30); do
  $VPPCTL show version >/dev/null 2>&1 && break
  sleep 0.5
done

$VPPCTL show plugins | grep -q rateguard_plugin || {
  echo "FAIL: rateguard plugin not loaded"; exit 1; }

$VPPCTL create packet-generator interface pg0
$VPPCTL set interface ip address pg0 10.0.0.1/24
$VPPCTL set interface state pg0 up
$VPPCTL set rateguard rate 100 burst 10
$VPPCTL rateguard interface pg0
MAC=$($VPPCTL show hardware pg0 | awk '/Ethernet address/ {print $3}' | tr -d ':')
MACFMT="${MAC:0:4}.${MAC:4:4}.${MAC:8:4}"
$VPPCTL packet-generator new "{
  name rg
  limit 100
  rate 1e6
  node ethernet-input
  size 100-100
  interface pg0
  data { IP4: 000a.0a0a.0a0a -> $MACFMT
         UDP: 10.0.0.2 -> 10.0.0.1
         UDP: 1234 -> 2345
         incrementing 8 }
}"
$VPPCTL trace add pg-input 5
$VPPCTL packet-generator enable
sleep 2

SHOW=$($VPPCTL show rateguard)
echo "$SHOW"
echo "$SHOW" | grep -q "10 allowed, 90 dropped" || {
  echo "FAIL: expected 10 allowed / 90 dropped"; exit 1; }
$VPPCTL show trace max 1 | grep -q "rateguard: src 10.0.0.2" || {
  echo "FAIL: no rateguard trace record"; exit 1; }
DROPS=$($VPPCTL show errors | awk '/rate limited/ {print $1}')
[ "${DROPS:-0}" = "90" ] || { echo "FAIL: error counter $DROPS != 90"; exit 1; }
echo "PASS: Rust plugin loaded, rate-limited 100-pkt burst to 10 allowed / 90 dropped"
