#!/usr/bin/env bash

# =============================================================================
# MSU Parallel LONG Task Test
# =============================================================================
# Sends two LONG, multi-iteration tasks to 2 sessions simultaneously.
# Each task asks the LLM to investigate the trustee repo and write a report
# to /tmp/. This stresses parallel execution with real tool calls, multiple
# LLM iterations, and file I/O — far more demanding than simple one-shot
# questions.
#
# Usage:  bash scripts/msu_parallel_long_task_test.sh
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

api() { curl -sk --max-time 30 "$@"; }

# ---------------------------------------------------------------------------
# Step 0: Verify server
# ---------------------------------------------------------------------------
log "Checking server health..."
HEALTH=$(api "${BASE}/api/v1/health" 2>/dev/null || echo "")
if echo "$HEALTH" | grep -q '"ok"'; then
    ok "Server up: $HEALTH"
else
    fail "Server not responding on $BASE"
    exit 1
fi

TEST_AUTH=$(api -o /dev/null -w "%{http_code}" "${BASE}/api/v1/session" -H "$AUTH" 2>/dev/null || echo "000")
if [[ "$TEST_AUTH" != "200" ]]; then
    fail "Dev auth failed (HTTP $TEST_AUTH). Is local_dev_mode = true?"
    exit 1
fi
ok "Dev mode auth working"

# ---------------------------------------------------------------------------
# Step 1: Clean up existing live sessions
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
except Exception: pass
" 2>/dev/null)

for sid in $EXISTING; do
    # Force destroy even if running (wait for idle first)
    for _ in 1 2 3; do
        CODE=$(api -o /dev/null -w "%{http_code}" -X DELETE "${BASE}/api/v1/sessions/${sid}" -H "$AUTH" 2>/dev/null || echo "000")
        [[ "$CODE" == "200" ]] && break
        sleep 2
    done
    info "  Deleted $sid (HTTP $CODE)"
done
sleep 1

# ---------------------------------------------------------------------------
# Step 2: Clean up old reports
# ---------------------------------------------------------------------------
rm -f /tmp/trustee_report_a.md /tmp/trustee_report_b.md 2>/dev/null || true

# ---------------------------------------------------------------------------
# Step 3: Create 2 sessions
# ---------------------------------------------------------------------------
log "Creating 2 sessions..."

SID1=$(api -X POST "${BASE}/api/v1/sessions" \
    -H "Content-Type: application/json" -H "$AUTH" \
    -d '{"session_name": "LongTask-Auth-investigation"}' \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['session_id'])" 2>/dev/null)

SID2=$(api -X POST "${BASE}/api/v1/sessions" \
    -H "Content-Type: application/json" -H "$AUTH" \
    -d '{"session_name": "LongTask-MSU-investigation"}' \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['session_id'])" 2>/dev/null)

if [[ -z "$SID1" || -z "$SID2" ]]; then
    fail "Failed to create sessions: SID1=$SID1, SID2=$SID2"
    exit 1
fi
ok "Session A: $SID1"
ok "Session B: $SID2"

# ---------------------------------------------------------------------------
# Step 4: Define two LONG tasks — real repo investigation with report output
# ---------------------------------------------------------------------------
TASK_A="Investigate the authentication system in /Projects/Podtan/trustee. Read the auth.rs file in trustee-api/src/, understand how JWT validation works, how dev mode tokens work, and how the cookie-based session auth works. Then write a detailed markdown report to /tmp/trustee_report_a.md summarizing your findings with code references."

TASK_B="Investigate the multi-session architecture in /Projects/Podtan/trustee. Read state.rs and routes.rs in trustee-api/src/, understand how SessionRegistry works, how sessions are created/destroyed, and how the drain task relays workflow messages. Then write a detailed markdown report to /tmp/trustee_report_b.md summarizing your findings with code references."

echo ""
info "Task A: Investigate auth system, write report to /tmp/trustee_report_a.md"
info "Task B: Investigate MSU architecture, write report to /tmp/trustee_report_b.md"
echo ""

# ---------------------------------------------------------------------------
# Step 5: Send BOTH commands simultaneously
# ---------------------------------------------------------------------------
log "Firing both long tasks AT THE SAME TIME..."

RESULT_FILE=$(mktemp)

(
    CODE_A=$(api -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions/${SID1}/command" \
        -H "Content-Type: application/json" -H "$AUTH" \
        -d '{"command": "'"$TASK_A"'", "session_id": "'"$SID1"'"}')
    echo "A:${CODE_A}" >> "$RESULT_FILE"
) &

(
    CODE_B=$(api -o /dev/null -w "%{http_code}" -X POST "${BASE}/api/v1/sessions/${SID2}/command" \
        -H "Content-Type: application/json" -H "$AUTH" \
        -d '{"command": "'"$TASK_B"'", "session_id": "'"$SID2"'"}')
    echo "B:${CODE_B}" >> "$RESULT_FILE"
) &

# ---------------------------------------------------------------------------
# Step 6: Poll — verify BOTH are Running simultaneously
# ---------------------------------------------------------------------------
sleep 2

log "Checking parallel execution..."
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
except Exception: print('ERROR')
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
except Exception: print('ERROR')
" 2>/dev/null)

info "Live states: A=$STATE_A, B=$STATE_B"

if [[ "$STATE_A" == "Running" && "$STATE_B" == "Running" ]]; then
    ok "🎉 BOTH sessions Running simultaneously — parallel execution confirmed."
else
    warn "Not both Running (A=$STATE_A, B=$STATE_B). One may have errored."
fi

# Wait for command API calls to return
wait

CODES=$(cat "$RESULT_FILE" 2>/dev/null || echo "")
rm -f "$RESULT_FILE"
CODE_A=$(echo "$CODES" | grep "^A:" | cut -d: -f2 || echo "000")
CODE_B=$(echo "$CODES" | grep "^B:" | cut -d: -f2 || echo "000")
info "HTTP codes: A=$CODE_A, B=$CODE_B"

# ---------------------------------------------------------------------------
# Step 7: Monitor progress — poll every 5s and report state + output tail
# ---------------------------------------------------------------------------
echo ""
log "Monitoring progress (polling every 5s, up to 300s)..."
echo ""

START_TIME=$(date +%s)
MAX_SECONDS=300

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))
    if [[ $ELAPSED -gt $MAX_SECONDS ]]; then
        warn "Reached ${MAX_SECONDS}s timeout — stopping monitoring."
        break
    fi

    # Get states
    SA=$(api "${BASE}/api/v1/sessions/${SID1}/live" -H "$AUTH" 2>/dev/null \
        | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    lines = d.get('output_lines', [])
    state = d.get('workflow_state', '?')
    tail = lines[-1][:80] if lines else ''
    print(f'{state}|{tail}')
except Exception: print('?|PARSE_ERROR')
" 2>/dev/null || echo "?|FETCH_ERROR")

    SB=$(api "${BASE}/api/v1/sessions/${SID2}/live" -H "$AUTH" 2>/dev/null \
        | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    lines = d.get('output_lines', [])
    state = d.get('workflow_state', '?')
    tail = lines[-1][:80] if lines else ''
    print(f'{state}|{tail}')
except Exception: print('?|PARSE_ERROR')
" 2>/dev/null || echo "?|FETCH_ERROR")

    STATE_A_VAL="${SA%%|*}"
    STATE_B_VAL="${SB%%|*}"
    TAIL_A="${SA#*|}"
    TAIL_B="${SB#*|}"

    info "[${ELAPSED}s] A=${STATE_A_VAL} (${TAIL_A:0:60})  |  B=${STATE_B_VAL} (${TAIL_B:0:60})"

    # Check if reports exist yet
    REPORT_A_EXISTS="no"; [[ -f /tmp/trustee_report_a.md ]] && REPORT_A_EXISTS="yes"
    REPORT_B_EXISTS="no"; [[ -f /tmp/trustee_report_b.md ]] && REPORT_B_EXISTS="yes"

    if [[ "$REPORT_A_EXISTS" == "yes" && "$REPORT_B_EXISTS" == "yes" ]]; then
        ok "Both reports created!"
        # Give it a couple more seconds to finish writing
        sleep 3
        break
    fi

    # Break if both idle (done or failed)
    if [[ "$STATE_A_VAL" == "Idle" && "$STATE_B_VAL" == "Idle" ]]; then
        ok "Both sessions returned to Idle."
        break
    fi

    sleep 5
done

# ---------------------------------------------------------------------------
# Step 8: Final results
# ---------------------------------------------------------------------------
echo ""
log "Final session states..."

FINAL_A=$(api "${BASE}/api/v1/sessions/${SID1}/live" -H "$AUTH" 2>/dev/null \
    | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('workflow_state','?'))" 2>/dev/null || echo "?")
FINAL_B=$(api "${BASE}/api/v1/sessions/${SID2}/live" -H "$AUTH" 2>/dev/null \
    | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('workflow_state','?'))" 2>/dev/null || echo "?")

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║     MSU PARALLEL LONG TASK TEST — RESULTS               ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo -e "║  Session A (auth investigation):"
echo -e "║    Final state:   $FINAL_A"
echo -e "║    HTTP code:     $CODE_A"
echo -e "║    Report exists: $([[ -f /tmp/trustee_report_a.md ]] && echo 'YES ✅' || echo 'NO ❌')"
echo -e "║    Report size:   $(wc -c < /tmp/trustee_report_a.md 2>/dev/null || echo 0) bytes"
echo "║"
echo -e "║  Session B (MSU investigation):"
echo -e "║    Final state:   $FINAL_B"
echo -e "║    HTTP code:     $CODE_B"
echo -e "║    Report exists: $([[ -f /tmp/trustee_report_b.md ]] && echo 'YES ✅' || echo 'NO ❌')"
echo -e "║    Report size:   $(wc -c < /tmp/trustee_report_b.md 2>/dev/null || echo 0) bytes"
echo "║"
echo -e "║  Session IDs:"
echo -e "║    A: $SID1"
echo -e "║    B: $SID2"
echo "╚══════════════════════════════════════════════════════════╝"

echo ""
info "Session A user dir: ~/.trustee/users/9376318614a0f6b5/"
info "Session B user dir: ~/.trustee/users/9376318614a0f6b5/"
info "Check session files with:"
info "  find ~/.trustee/users/9376318614a0f6b5/ -name '*${SID1}*' -o -name '*${SID2}*' 2>/dev/null"
echo ""
info "Reports:"
info "  /tmp/trustee_report_a.md"
info "  /tmp/trustee_report_b.md"
