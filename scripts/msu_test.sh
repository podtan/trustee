#!/usr/bin/env bash

# =============================================================================
# MSU Parallel Session Test Script
# =============================================================================
# Starts trustee on port 3020 in dev mode, creates 4 sessions, and sends
# a small task to each in parallel. Verifies that all 4 sessions can run
# simultaneously without 409 CONFLICT.
#
# This script tests the MSU API mechanics (create, parallel command,
# session limit, destroy, legacy compat) — NOT agent execution.
# It does not wait for LLM workflows to complete.
#
# Usage:  bash scripts/msu_test.sh
# =============================================================================

BIN="${TRUSTEE_BIN:-/Projects/Podtan/trustee/target/release/trustee}"
PORT=3020
BASE="http://127.0.0.1:${PORT}"
AUTH="Cookie: trustee_token=dev:tester@tanbal.ir:Tester:tester"
LOG_FILE="/tmp/trustee_msu_test.log"
TOML="$HOME/.trustee/config/trustee.toml"

# Colors
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
log()  { echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $*"; }
ok()   { echo -e "${GREEN}[$(date +%H:%M:%S)] ✅${NC} $*"; }
fail() { echo -e "${RED}[$(date +%H:%M:%S)] ❌${NC} $*"; }
warn() { echo -e "${YELLOW}[$(date +%H:%M:%S)] ⚠️ ${NC} $*"; }

# Quick curl wrapper with timeout
cq() { curl --max-time 5 -s "$@"; }

PASS_COUNT=0
FAIL_COUNT=0

check() {
    if [[ "$1" == "$2" ]]; then
        ok "$3"
        ((PASS_COUNT++))
    else
        fail "$3 (expected $2, got $1)"
        ((FAIL_COUNT++))
    fi
}

# --- Cleanup ---
ORIG_TOML=$(cat "$TOML")
cleanup() {
    # Kill trustee by port
    pkill -f "trustee web -a 127.0.0.1:${PORT}" 2>/dev/null || true
    # Restore config
    echo "$ORIG_TOML" > "$TOML"
    log "Config restored."
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Step 0: Enable dev mode + file backend
# ---------------------------------------------------------------------------
log "Enabling dev mode in trustee config..."
sed -i 's/local_dev_mode = false/local_dev_mode = true/' "$TOML" 2>/dev/null || true
if grep -q 'backend_type = "mongodb"' "$TOML"; then
    sed -i 's/backend_type = "mongodb"/backend_type = "file"/' "$TOML"
    warn "Switched backend from mongodb to file"
fi

# ---------------------------------------------------------------------------
# Step 1: Start trustee
# ---------------------------------------------------------------------------
log "Starting trustee on port $PORT (logs → $LOG_FILE)..."
# Use setsid to fully detach trustee from this script's process group
setsid "$BIN" web -a 127.0.0.1:${PORT} --no-tls >"$LOG_FILE" 2>&1 &
TRUSTEE_PID=$!

log "Waiting for server..."
for i in $(seq 1 15); do
    cq "${BASE}/api/v1/health" >/dev/null 2>&1 && break
    sleep 1
done

HEALTH=$(cq "${BASE}/api/v1/health")
if echo "$HEALTH" | grep -q '"ok"'; then
    ok "Server is up! ($HEALTH)"
else
    fail "Server failed to start"; cat "$LOG_FILE"; exit 1
fi

# ---------------------------------------------------------------------------
# Step 2: Create 4 sessions
# ---------------------------------------------------------------------------
log "Creating 4 sessions..."

SESSIONS=()
NAMES=("Joke" "Idiom" "SpaceFact" "Quote")
TASKS=(
    "Tell me exactly one short joke. Nothing else."
    "Tell me exactly one famous idiom and its meaning. Nothing else."
    "Tell me exactly one interesting fact about space. Nothing else."
    "Tell me exactly one short motivational quote. Nothing else."
)

for i in 0 1 2 3; do
    NUM=$((i + 1))
    RESP=$(cq -X POST "${BASE}/api/v1/sessions" \
        -H "Content-Type: application/json" -H "$AUTH" \
        -d "{\"session_name\": \"${NAMES[$i]}\"}")
    SID=$(echo "$RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['session_id'])" 2>/dev/null)
    if [[ -n "$SID" ]]; then
        SESSIONS+=("$SID")
        ok "Session $NUM created: $SID (${NAMES[$i]})"
    else
        fail "Failed to create session $NUM: $RESP"
        exit 1
    fi
done

# ---------------------------------------------------------------------------
# Step 3: Send commands to all 4 sessions IN PARALLEL
# ---------------------------------------------------------------------------
log "Sending commands to all 4 sessions simultaneously..."

RESULTS_FILE=$(mktemp)

for i in 0 1 2 3; do
    NUM=$((i + 1))
    (
        CODE=$(cq -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions/${SESSIONS[$i]}/command" \
            -H "Content-Type: application/json" -H "$AUTH" \
            -d "{\"command\": \"${TASKS[$i]}\"}")
        echo "${NUM}:${CODE}" >> "$RESULTS_FILE"
    ) &
done

# Wait for background curl commands to finish (not trustee — detached via setsid)
sleep 3

# Read results
while IFS=: read -r NUM CODE; do
    RESULTS[$NUM]=$CODE
    if [[ "$CODE" == "200" ]]; then
        ok "Session $NUM command accepted (200)"
    else
        fail "Session $NUM command failed (HTTP $CODE)"
    fi
done < "$RESULTS_FILE"
rm -f "$RESULTS_FILE"

# ---------------------------------------------------------------------------
# Step 4: Verify 409 on busy session
# ---------------------------------------------------------------------------
sleep 1
log "Testing 409 on busy session..."
BUSY_CODE=$(cq -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions/${SESSIONS[0]}/command" \
    -H "Content-Type: application/json" -H "$AUTH" \
    -d '{"command": "another task while busy"}' 2>/dev/null || echo "000")
check "$BUSY_CODE" "409" "Busy session rejected with 409 CONFLICT"

# ---------------------------------------------------------------------------
# Step 5: Test session limit (5th = 429)
# ---------------------------------------------------------------------------
log "Testing session limit (5th session should fail)..."
LIMIT_CODE=$(cq -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions" \
    -H "Content-Type: application/json" -H "$AUTH" \
    -d '{"session_name": "should fail"}' 2>/dev/null || echo "000")
check "$LIMIT_CODE" "429" "5th session rejected with 429"

# ---------------------------------------------------------------------------
# Step 6: Destroy a running session (should fail with 409)
# ---------------------------------------------------------------------------
log "Testing destroy on running session (should fail)..."
DESTROY_RUN_CODE=$(cq -o /dev/null -w "%{http_code}" -X DELETE "${BASE}/api/v1/sessions/${SESSIONS[0]}" \
    -H "$AUTH" 2>/dev/null || echo "000")
check "$DESTROY_RUN_CODE" "409" "Destroy on running session rejected with 409"

# ---------------------------------------------------------------------------
# Step 7: Legacy route compatibility
# ---------------------------------------------------------------------------
log "Testing legacy routes..."
LEGACY_CODE=$(cq -o /dev/null -w "%{http_code}" "${BASE}/api/v1/session" -H "$AUTH" 2>/dev/null || echo "000")
check "$LEGACY_CODE" "200" "Legacy GET /session returns 200"

# ---------------------------------------------------------------------------
# Step 8: Test 404 on non-existent session
# ---------------------------------------------------------------------------
log "Testing 404 on non-existent session..."
NF_CODE=$(cq -o /dev/null -w "%{http_code}" "${BASE}/api/v1/sessions/nonexistent/live" \
    -H "$AUTH" 2>/dev/null || echo "000")
check "$NF_CODE" "404" "Non-existent session returns 404"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================="
log "MSU TEST SUMMARY"
echo "============================================="
log "Sessions created:     4"
log "Parallel commands:    4 sent simultaneously"

ALL_OK=true
for i in 0 1 2 3; do
    NUM=$((i + 1))
    CODE="${RESULTS[$NUM]:-N/A}"
    if [[ "$CODE" == "200" ]]; then
        ok "Session $NUM: 200 OK"
    else
        fail "Session $NUM: HTTP $CODE"
        ALL_OK=false
    fi
done

echo ""
log "Additional checks: $PASS_COUNT passed, $FAIL_COUNT failed"

if $ALL_OK && [[ "$FAIL_COUNT" -eq 0 ]]; then
    echo ""
    ok "🎉 ALL TESTS PASSED — Multi-Session Per User (MSU) is working!"
    ok "4 sessions ran in parallel without 409 CONFLICT."
else
    echo ""
    fail "Some tests failed — check output above."
fi
echo "============================================="
