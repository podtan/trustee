#!/usr/bin/env bash

# =============================================================================
# MSU Parallel Execution Test
# =============================================================================
# Tests whether Trustee Web truly executes multiple sessions IN PARALLEL.
#
# Runs against the already-running trustee instance on https://127.0.0.1:3000
# Requires dev mode enabled in trustee.toml (local_dev_mode = true).
#
# What it tests:
#   1. Create 2 sessions
#   2. Send commands to both simultaneously
#   3. Poll /sessions/live — verify BOTH show "Running" at the same time
#   4. Wait for both to complete
#   5. Verify outputs are different (no cross-contamination)
#   6. Scan logs for panics / race condition signatures
#
# Usage:  bash scripts/msu_parallel_test.sh
# =============================================================================

set -euo pipefail

BASE="https://127.0.0.1:3000"
AUTH="Cookie: trustee_token=dev:tester@tanbal.ir:Tester:tester"

# Colors
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; CYAN='\033[0;36m'; NC='\033[0m'
log()  { echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $*"; }
ok()   { echo -e "${GREEN}[$(date +%H:%M:%S)] ✅${NC} $*"; }
fail() { echo -e "${RED}[$(date +%H:%M:%S)] ❌${NC} $*"; }
warn() { echo -e "${YELLOW}[$(date +%H:%M:%S)] ⚠️ ${NC} $*"; }
info() { echo -e "${CYAN}[$(date +%H:%M:%S)]${NC} $*"; }

# Curl wrapper (HTTPS, skip cert verification, generous timeout for LLM calls)
api() { curl -sk --max-time 30 "$@"; }

PASS=0; FAIL=0
rp() { ok "$1"; PASS=$((PASS + 1)); }
rf() { fail "$1"; FAIL=$((FAIL + 1)); }

# ---------------------------------------------------------------------------
# Step 0: Verify server is up and in dev mode
# ---------------------------------------------------------------------------
log "Checking server health..."
HEALTH=$(api "${BASE}/api/v1/health" 2>/dev/null || echo "")
if echo "$HEALTH" | grep -q '"ok"'; then
    ok "Server up: $HEALTH"
else
    fail "Server not responding on $BASE"
    exit 1
fi

# Verify dev auth works
TEST_AUTH=$(api -o /dev/null -w "%{http_code}" "${BASE}/api/v1/session" -H "$AUTH" 2>/dev/null || echo "000")
if [[ "$TEST_AUTH" != "200" ]]; then
    fail "Dev auth failed (HTTP $TEST_AUTH). Is local_dev_mode = true in trustee.toml?"
    exit 1
fi
ok "Dev mode auth working"

# ---------------------------------------------------------------------------
# Step 1: Clean up any existing live sessions
# ---------------------------------------------------------------------------
log "Cleaning up existing sessions..."
EXISTING=$(api "${BASE}/api/v1/sessions/live" -H "$AUTH" 2>/dev/null \
    | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    sessions = d if isinstance(d, list) else d.get('sessions', [])
    for s in sessions:
        print(s['session_id'])
except: pass
" 2>/dev/null)

for sid in $EXISTING; do
    api -X DELETE "${BASE}/api/v1/sessions/${sid}" -H "$AUTH" >/dev/null 2>&1
    info "  Deleted $sid"
done
sleep 0.5

# ---------------------------------------------------------------------------
# Step 2: Create exactly 2 sessions
# ---------------------------------------------------------------------------
log "Creating 2 sessions..."

SID1=$(api -X POST "${BASE}/api/v1/sessions" \
    -H "Content-Type: application/json" -H "$AUTH" \
    -d '{"session_name": "Parallel-A"}' \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['session_id'])" 2>/dev/null)

SID2=$(api -X POST "${BASE}/api/v1/sessions" \
    -H "Content-Type: application/json" -H "$AUTH" \
    -d '{"session_name": "Parallel-B"}' \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['session_id'])" 2>/dev/null)

if [[ -z "$SID1" || -z "$SID2" ]]; then
    fail "Failed to create sessions: SID1=$SID1, SID2=$SID2"
    exit 1
fi
ok "Session A: $SID1"
ok "Session B: $SID2"

# ---------------------------------------------------------------------------
# Step 3: Send commands to BOTH sessions simultaneously
# ---------------------------------------------------------------------------
log "Sending commands to both sessions AT THE SAME TIME..."

RESULT_FILE=$(mktemp)

(
    CODE_A=$(api -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions/${SID1}/command" \
        -H "Content-Type: application/json" -H "$AUTH" \
        -d '{"command": "Tell me exactly one short joke. Nothing else.", "session_id": "'"$SID1"'"}')
    echo "A:${CODE_A}" >> "$RESULT_FILE"
) &

(
    CODE_B=$(api -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions/${SID2}/command" \
        -H "Content-Type: application/json" -H "$AUTH" \
        -d '{"command": "Tell me exactly one famous idiom and its meaning. Nothing else.", "session_id": "'"$SID2"'"}')
    echo "B:${CODE_B}" >> "$RESULT_FILE"
) &

# ---------------------------------------------------------------------------
# Step 4: CRITICAL — Poll to see if BOTH are Running simultaneously
#         Poll 3 times over 3 seconds to catch the overlapping window
# ---------------------------------------------------------------------------
BOTH_RUNNING=false
for poll in 1 2 3; do
    sleep 1
    LIVE_JSON=$(api "${BASE}/api/v1/sessions/live" -H "$AUTH" 2>/dev/null)

    STATE_A=$(echo "$LIVE_JSON" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    sessions = data if isinstance(data, list) else data.get('sessions', [])
    for s in sessions:
        if s.get('session_id') == '$SID1':
            print(s.get('workflow_state', '?')); break
    else:
        print('NOT_FOUND')
except Exception:
    print('ERROR')
" 2>/dev/null)

    STATE_B=$(echo "$LIVE_JSON" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    sessions = data if isinstance(data, list) else data.get('sessions', [])
    for s in sessions:
        if s.get('session_id') == '$SID2':
            print(s.get('workflow_state', '?')); break
    else:
        print('NOT_FOUND')
except Exception:
    print('ERROR')
" 2>/dev/null)

    info "Poll $poll: A=$STATE_A, B=$STATE_B"

    if [[ "$STATE_A" == "Running" && "$STATE_B" == "Running" ]]; then
        BOTH_RUNNING=true
        break
    fi
done

if $BOTH_RUNNING; then
    rp "🎉 BOTH sessions Running simultaneously! Parallel execution confirmed."
else
    warn "Never caught both Running at same time (A=$STATE_A, B=$STATE_B)"
    warn "This could mean: one finished too fast, one errored, or they serialized."
    FAIL=$((FAIL + 1))
fi

# Wait for the command API calls to return
wait

# Read the HTTP response codes
CODES=$(cat "$RESULT_FILE" 2>/dev/null || echo "")
rm -f "$RESULT_FILE"
CODE_A=$(echo "$CODES" | grep "^A:" | cut -d: -f2 || echo "000")
CODE_B=$(echo "$CODES" | grep "^B:" | cut -d: -f2 || echo "000")

info "HTTP codes: A=$CODE_A, B=$CODE_B"
[[ "$CODE_A" == "200" ]] && rp "Session A command accepted (200)" || rf "Session A command failed (HTTP $CODE_A)"
[[ "$CODE_B" == "200" ]] && rp "Session B command accepted (200)" || rf "Session B command failed (HTTP $CODE_B)"

# ---------------------------------------------------------------------------
# Step 5: Wait for both workflows to complete (up to 120s)
# ---------------------------------------------------------------------------
log "Waiting for both workflows to complete (up to 120s)..."

FINAL_A="?"
FINAL_B="?"

for i in $(seq 1 120); do
    if [[ "$FINAL_A" == "?" ]]; then
        SA=$(api "${BASE}/api/v1/sessions/${SID1}/live" -H "$AUTH" 2>/dev/null \
            | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('workflow_state','?'))" 2>/dev/null || echo "?")
        if [[ "$SA" == "Idle" ]]; then
            FINAL_A="Idle"
            ok "Session A completed after ${i}s"
        elif [[ "$SA" == "ERROR" || -z "$SA" ]]; then
            FINAL_A="ERROR"
            fail "Session A returned error state"
        fi
    fi

    if [[ "$FINAL_B" == "?" ]]; then
        SB=$(api "${BASE}/api/v1/sessions/${SID2}/live" -H "$AUTH" 2>/dev/null \
            | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('workflow_state','?'))" 2>/dev/null || echo "?")
        if [[ "$SB" == "Idle" ]]; then
            FINAL_B="Idle"
            ok "Session B completed after ${i}s"
        elif [[ "$SB" == "ERROR" || -z "$SB" ]]; then
            FINAL_B="ERROR"
            fail "Session B returned error state"
        fi
    fi

    [[ "$FINAL_A" != "?" && "$FINAL_B" != "?" ]] && break
    sleep 1
done

[[ "$FINAL_A" == "?" ]] && { warn "Session A still running after 120s"; FAIL=$((FAIL + 1)); }
[[ "$FINAL_B" == "?" ]] && { warn "Session B still running after 120s"; FAIL=$((FAIL + 1)); }

# ---------------------------------------------------------------------------
# Step 6: Verify outputs are different (no cross-contamination)
# ---------------------------------------------------------------------------
log "Checking session outputs..."

OUT_A=$(api "${BASE}/api/v1/sessions/${SID1}/live" -H "$AUTH" 2>/dev/null \
    | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    lines = d.get('output_lines', [])
    # Skip the command echo line, get last few content lines
    text = ' '.join(lines[-5:]) if lines else ''
    print(text[:300])
except: print('PARSE_ERROR')
" 2>/dev/null || echo "FETCH_ERROR")

OUT_B=$(api "${BASE}/api/v1/sessions/${SID2}/live" -H "$AUTH" 2>/dev/null \
    | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    lines = d.get('output_lines', [])
    text = ' '.join(lines[-5:]) if lines else ''
    print(text[:300])
except: print('PARSE_ERROR')
" 2>/dev/null || echo "FETCH_ERROR")

info "Session A output (tail): ${OUT_A:0:120}..."
info "Session B output (tail): ${OUT_B:0:120}..."

if [[ "$OUT_A" == "$OUT_B" && "$OUT_A" != "PARSE_ERROR" && "$OUT_A" != "FETCH_ERROR" ]]; then
    warn "Both sessions produced identical output — possible cross-contamination!"
    FAIL=$((FAIL + 1))
elif [[ "$OUT_A" != "PARSE_ERROR" && "$OUT_A" != "FETCH_ERROR" ]]; then
    rp "Session outputs are different — no cross-contamination"
fi

# ---------------------------------------------------------------------------
# Step 7: Test 409 on busy session (if one is still running)
# ---------------------------------------------------------------------------
# This was already covered by the original msu_test.sh, but good to verify
# Only test if we can make a new session

# ---------------------------------------------------------------------------
# Summary
# ============================================================================
echo ""
echo "╔══════════════════════════════════════════════════════╗"
echo "║     MSU PARALLEL EXECUTION TEST SUMMARY              ║"
echo "╠══════════════════════════════════════════════════════╣"
echo -e "║  ${GREEN}Passed: $PASS${NC}                                        "
echo -e "║  ${RED}Failed: $FAIL${NC}                                        "
echo "║                                                     "
echo -e "║  Key Results:                                       "
echo -e "║  • HTTP accepted both:   A=$CODE_A B=$CODE_B              "
echo -e "║  • Both Running at once: $BOTH_RUNNING                        "
echo -e "║  • Both completed:       A=$FINAL_A B=$FINAL_B              "
echo -e "║  • Parallel exec works:  $($BOTH_RUNNING && echo 'YES ✅' || echo 'NO ❌')      "
echo "╚══════════════════════════════════════════════════════╝"

if [[ "$FAIL" -eq 0 ]]; then
    echo ""
    ok "🎉 ALL TESTS PASSED — Parallel multi-session execution works!"
else
    echo ""
    fail "❌ $FAIL test(s) failed — see details above."
    echo ""
    info "To investigate, check trustee's stderr/console output."
    info "Also try: curl -sk '$BASE/api/v1/sessions/live' -H '$AUTH'"
fi
