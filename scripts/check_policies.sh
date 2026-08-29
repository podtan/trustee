#!/usr/bin/env bash
# Cedar policy gate for Trustee — run before tagging/deploying (P2, nghr 645809c3).
#
# The failure class this catches: an authz layer that silently disables
# (trustee pre-P2 tolerated Cedar init failure; fame v0.1.0-0.1.1 shipped
# with a schema that never parsed). cargo test covers the embedded sources;
# this script validates the on-disk files exactly as a release would, plus
# runtime-faithful positive AND negative authorization controls.
#
# Usage: scripts/check_policies.sh   (from repo root)
# Requires: cedar-policy-cli (cargo install cedar-policy-cli)
set -euo pipefail

cd "$(dirname "$0")/.."

SCHEMA=crates/trustee-api/policies/trustee_schema.cedarschema
POLICY=crates/trustee-api/policies/trustee_default.cedar

echo "── cedar validate ──────────────────────────────────────────"
cedar validate --schema "$SCHEMA" --policies "$POLICY"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Runtime-faithful entities: User principal with role attribute + the
# TrusteeApp resource — exactly what check_cedar_authorized builds.
for role in admin user agent service; do
  printf '[\n {"uid": {"type": "User", "id": "gate-%s"}, "attrs": {"role": "%s"}, "parents": []},\n {"uid": {"type": "TrusteeApp", "id": "default"}, "attrs": {}, "parents": []}\n]\n' "$role" "$role" > "$TMP/$role.json"
done
printf '[\n {"uid": {"type": "User", "id": "gate-norole"}, "attrs": {}, "parents": []},\n {"uid": {"type": "TrusteeApp", "id": "default"}, "attrs": {}, "parents": []}\n]\n' > "$TMP/norole.json"

check() { # role json action expected(allow|deny)
  local desc="[$1] $3 -> $4"
  if [ "$4" = allow ]; then
    cedar authorize --schema "$SCHEMA" --policies "$POLICY" \
      --entities "$2" --principal "User::\"gate-$1\"" \
      --action "Action::\"$3\"" --resource 'TrusteeApp::"default"' >/dev/null &&
      echo "  OK  $desc" || { echo "  FAIL $desc"; exit 1; }
  else
    if cedar authorize --schema "$SCHEMA" --policies "$POLICY" \
      --entities "$2" --principal "User::\"gate-$1\"" \
      --action "Action::\"$3\"" --resource 'TrusteeApp::"default"' >/dev/null 2>&1; then
      echo "  FAIL $desc (expected DENY)"; exit 1
    else
      echo "  OK  $desc"
    fi
  fi
}

echo "── authorization controls ──────────────────────────────────"
check admin   "$TMP/admin.json"  DeleteSession   allow
check admin   "$TMP/admin.json"  CommandSession  allow
check user    "$TMP/user.json"   DeleteSession   allow
check user    "$TMP/user.json"   ViewHistory     allow
check agent   "$TMP/agent.json"  CommandSession  allow
check agent   "$TMP/agent.json"  DeleteSession   deny
check service "$TMP/service.json" ViewSession    allow
check service "$TMP/service.json" CommandSession deny
check norole  "$TMP/norole.json"  ViewSession    deny

echo "── policy gate PASSED ──────────────────────────────────────"
