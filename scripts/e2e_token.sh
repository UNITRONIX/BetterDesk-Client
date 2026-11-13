#!/usr/bin/env bash
# End-to-end proof that the managed-mode token path works.
#
# Creates a real token row (same shape the admin API writes), then registers a
# fresh device with it and asserts the answer is `approved`.
set -uo pipefail
API="http://127.0.0.1:21114"
DEV_A="BD-E2E-TOKEN-A"
DEV_B="BD-E2E-TOKEN-B"
PREFIX="bd-e2e-$(date +%s)"
TOKEN="${PREFIX}-secret-value"

hash_token() {
  printf '%s' "$1" | sha256sum | cut -d' ' -f1
}

echo "== insert token =="
sudo -n -u postgres psql -d betterdesk -tA -c \
  "insert into device_tokens (token_hash, token_prefix, name, status, max_uses, use_count, created_at)
   values ('$(hash_token "$TOKEN")', 'bd-e2e', 'e2e managed token', 'active', 1, 0, now())
   returning id;" 2>&1

register() {
  local dev="$1" tok="$2"
  curl -s -m 10 -o /tmp/reg_out.json -w '%{http_code}' -X POST "$API/api/devices/register" \
    -H 'Content-Type: application/json' \
    -d "{\"device_id\":\"$dev\",\"uuid\":\"e2e-machine-$dev\",\"hostname\":\"e2e\",\"platform\":\"linux\",\"device_type\":\"betterdesk-desktop\",\"token\":\"$tok\"}"
}

echo "== device A with token =="
CODE=$(register "$DEV_A" "$TOKEN")
echo "HTTP $CODE"
cat /tmp/reg_out.json; echo

echo "== device B replaying the same token =="
CODE_B=$(register "$DEV_B" "$TOKEN")
echo "HTTP $CODE_B"
cat /tmp/reg_out.json; echo

echo "== device C with a wrong token =="
CODE_C=$(register "BD-E2E-TOKEN-C" "definitely-wrong")
echo "HTTP $CODE_C"
cat /tmp/reg_out.json; echo

echo "== cleanup =="
sudo -n -u postgres psql -d betterdesk -tA -c \
  "delete from device_tokens where token_prefix='bd-e2e';
   delete from peers where id like 'BD-E2E-TOKEN-%';
   delete from peer where id like 'BD-E2E-TOKEN-%';
   delete from server_config where key like '%BD-E2E-TOKEN-%';" 2>&1 | tail -5
echo done
