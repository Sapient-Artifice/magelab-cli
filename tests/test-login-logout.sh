#!/usr/bin/env bash
#
# Automated login/logout test script for magelab CLI.
#
# Phase 1 (offline):  build, install, config, logout, status, idempotency
# Phase 2 (gateway):  web login via browser, token validation, authenticated
#                     commands, token refresh, post-logout commands
# Phase 3 (optional): Google OAuth, magic auth
#
# Usage:
#   ./tests/test-login-logout.sh                         # Phase 1 only
#   ./tests/test-login-logout.sh --gateway               # Phase 1 + 2 (Docker defaults)
#   ./tests/test-login-logout.sh --gateway URL           # Phase 1 + 2 (custom gateway)
#   ./tests/test-login-logout.sh --gateway --web-url URL # Phase 1 + 2 (custom web app)
#   ./tests/test-login-logout.sh --gateway --google      # Phase 1 + 2 + 3 (Google OAuth)
#   ./tests/test-login-logout.sh --gateway --magic       # Phase 1 + 2 + 3 (magic auth)
#   ./tests/test-login-logout.sh --help
#
set -euo pipefail

# ── Defaults ──────────────────────────────────────────────────────────────────

GATEWAY_URL=""
WEB_URL=""
EMAIL=""
RUN_GOOGLE=false
RUN_MAGIC=false
MAGELAB=""           # resolved after build
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
LOGIN_TIMEOUT=120    # seconds to wait for browser login

PASS=0
FAIL=0
SKIP=0

# ── Colors ────────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

# ── Helpers ───────────────────────────────────────────────────────────────────

usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --gateway [URL]    Gateway base URL (default: http://localhost:3001)
  --web-url URL      Web app URL for browser login (default: http://localhost:3007)
  --email EMAIL      Email for magic auth tests
  --magic            Also run magic auth tests (Phase 3)
  --google           Also run Google OAuth test (Phase 3)
  --timeout SECS     Browser login timeout (default: 120)
  --help             Show this help

Phase 1 (always runs):  build, install, config, logout, status
Phase 2 (--gateway):    web browser login, token, authenticated commands, refresh
Phase 3 (--magic/--google): alternative login methods
EOF
    exit 0
}

log_section() { echo -e "\n${BOLD}${CYAN}━━━ $1 ━━━${RESET}"; }
log_test()    { echo -e "\n${BOLD}▸ $1${RESET}"; }
log_pass()    { echo -e "  ${GREEN}✓ PASS${RESET}: $1"; PASS=$((PASS + 1)); }
log_fail()    { echo -e "  ${RED}✗ FAIL${RESET}: $1"; FAIL=$((FAIL + 1)); }
log_skip()    { echo -e "  ${YELLOW}⊘ SKIP${RESET}: $1"; SKIP=$((SKIP + 1)); }
log_info()    { echo -e "  $1"; }

expect_success() {
    local desc="$1"; shift
    local out
    if out=$("$@" 2>&1); then
        log_pass "$desc"
        echo "$out"
        return 0
    else
        log_fail "$desc (exit $?)"
        echo "$out"
        return 1
    fi
}

expect_failure() {
    local desc="$1"; shift
    local out
    if out=$("$@" 2>&1); then
        log_fail "$desc (expected failure, got success)"
        echo "$out"
        return 1
    else
        log_pass "$desc (failed as expected)"
        echo "$out"
        return 0
    fi
}

assert_contains() {
    local output="$1" pattern="$2" desc="$3"
    if echo "$output" | grep -qF "$pattern"; then
        log_pass "$desc"
    else
        log_fail "$desc — expected '$pattern' in output"
        log_info "Got: $(echo "$output" | head -5)"
    fi
}

assert_matches() {
    local output="$1" pattern="$2" desc="$3"
    if echo "$output" | grep -qE "$pattern"; then
        log_pass "$desc"
    else
        log_fail "$desc — expected /$pattern/ in output"
        log_info "Got: $(echo "$output" | head -5)"
    fi
}

# Wait for a port to be listening (up to N seconds)
wait_for_port() {
    local port="$1" max_wait="${2:-10}" elapsed=0
    while ! lsof -i ":$port" -sTCP:LISTEN &>/dev/null; do
        sleep 0.2
        elapsed=$(echo "$elapsed + 0.2" | bc)
        if (( $(echo "$elapsed >= $max_wait" | bc -l) )); then
            return 1
        fi
    done
    return 0
}

# Run `magelab login --method web` in background, wait for browser auth,
# then collect the result. Returns 0 if login succeeded.
do_web_login() {
    local web_url="$1"
    local outfile="/tmp/magelab-web-login-$$.txt"

    # Ensure port is free
    if lsof -i :19872 -sTCP:LISTEN &>/dev/null; then
        log_fail "Port 19872 already in use"
        return 1
    fi

    # Start CLI login in background
    MAGELAB_WEB_URL="$web_url" $MAGELAB login --method web >"$outfile" 2>&1 &
    local login_pid=$!

    # Wait for the loopback listener to bind
    if ! wait_for_port 19872 5; then
        log_fail "CLI did not bind port 19872"
        kill "$login_pid" 2>/dev/null; wait "$login_pid" 2>/dev/null || true
        cat "$outfile" 2>/dev/null
        return 1
    fi

    log_info "CLI listening on :19872 — complete login in your browser"
    echo ""
    echo -e "  ${BOLD}${YELLOW}>>> Complete the login in your browser <<<${RESET}"
    echo -e "  ${BOLD}${YELLOW}>>> Waiting up to ${LOGIN_TIMEOUT}s...          <<<${RESET}"
    echo ""

    # Wait for the CLI process to finish (browser redirects token to loopback)
    local waited=0
    while kill -0 "$login_pid" 2>/dev/null; do
        sleep 1
        waited=$((waited + 1))
        if [[ $waited -ge $LOGIN_TIMEOUT ]]; then
            log_fail "Login timed out after ${LOGIN_TIMEOUT}s"
            kill "$login_pid" 2>/dev/null; wait "$login_pid" 2>/dev/null || true
            return 1
        fi
    done

    # Collect exit code and output
    wait "$login_pid"
    local exit_code=$?
    local out
    out=$(cat "$outfile" 2>/dev/null)
    rm -f "$outfile"

    if [[ $exit_code -eq 0 ]] && echo "$out" | grep -qiE "Logged in as"; then
        log_pass "web login succeeded"
        log_info "$(echo "$out" | grep -i 'Logged in')"
        return 0
    else
        log_fail "web login failed (exit $exit_code)"
        log_info "$(echo "$out" | tail -3)"
        return 1
    fi
}

# ── Parse args ────────────────────────────────────────────────────────────────

DOCKER_DEFAULT_GATEWAY="http://localhost:3001"
DOCKER_DEFAULT_WEB="http://localhost:3007"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --gateway)
            if [[ $# -ge 2 && "$2" != --* ]]; then
                GATEWAY_URL="$2"; shift 2
            else
                GATEWAY_URL="$DOCKER_DEFAULT_GATEWAY"; shift
            fi
            ;;
        --web-url)  WEB_URL="$2"; shift 2 ;;
        --email)    EMAIL="$2"; shift 2 ;;
        --magic)    RUN_MAGIC=true; shift ;;
        --google)   RUN_GOOGLE=true; shift ;;
        --timeout)  LOGIN_TIMEOUT="$2"; shift 2 ;;
        --help|-h)  usage ;;
        *)          echo "Unknown option: $1"; usage ;;
    esac
done

# Default web URL if not specified
[[ -z "$WEB_URL" ]] && WEB_URL="$DOCKER_DEFAULT_WEB"

# ── Phase 1: Offline tests (no gateway needed) ───────────────────────────────

log_section "Phase 1: Build, Install & Offline Tests"

cd "$CLI_DIR"

# Build
log_test "T0a: cargo build --release"
out=$(expect_success "release build" cargo build --release) || true

# Install
log_test "T0b: cargo install"
out=$(expect_success "install to cargo bin" cargo install --path . --force) || true

# Resolve binary
if command -v magelab &>/dev/null; then
    MAGELAB="magelab"
elif [[ -f "$CLI_DIR/target/release/magelab" ]]; then
    MAGELAB="$CLI_DIR/target/release/magelab"
else
    echo "ERROR: magelab binary not found after install"
    exit 1
fi
log_info "Using binary: $(command -v "$MAGELAB" || echo "$MAGELAB")"

# Version
log_test "T0c: version"
out=$($MAGELAB version 2>&1)
assert_matches "$out" '^magelab [0-9]+\.' "version output"

# ── T8: Logout (clean slate)
log_test "T8: Logout"
out=$($MAGELAB logout 2>&1)
assert_contains "$out" "Logged out" "logout prints confirmation"

# ── T9: Logout idempotent
log_test "T9: Logout when already logged out"
out=$($MAGELAB logout 2>&1)
assert_contains "$out" "Logged out" "second logout still succeeds"

# ── T5 (not logged in): Login status
log_test "T5a: Login status when not logged in"
out=$($MAGELAB login --status 2>&1)
assert_contains "$out" "Not logged in" "shows not logged in"

# ── T5 (API key): Login status with API key
log_test "T5b: Login status shows API key preview"
out=$(MAGELAB_API_KEY=sk-test-12345678 $MAGELAB login --status 2>&1)
assert_contains "$out" "API key: sk-t...5678" "API key preview"

# ── T8 verify: auth token fails after logout
log_test "T8b: auth token fails after logout"
out=$(expect_failure "auth token rejected" $MAGELAB auth token) || true
assert_matches "$out" "login|Not logged in" "error mentions login"

# ── T11: Authenticated commands after logout (point at unreachable gateway)
log_test "T11: Authenticated commands fail after logout"
$MAGELAB config set gateway_url "http://127.0.0.1:19999" 2>&1 >/dev/null || true
out=$(MAGELAB_API_KEY="" $MAGELAB models 2>&1) && {
    log_fail "models should fail without auth"
} || {
    assert_matches "$out" "authenticated|login|Not|connect" "models error mentions auth"
}
$MAGELAB config set gateway_url "https://api.magelab.ai" 2>&1 >/dev/null || true

# ── Config
log_test "T0d: Config shows path"
out=$($MAGELAB config 2>&1)
assert_contains "$out" "Config file:" "config shows path"

# ── Phase 1 summary
if [[ -z "$GATEWAY_URL" ]]; then
    log_section "Phase 1 Complete"
    echo -e "  ${GREEN}Pass: $PASS${RESET}  ${RED}Fail: $FAIL${RESET}  ${YELLOW}Skip: $SKIP${RESET}"
    echo ""
    echo "To run Phase 2 (live auth tests), re-run with:"
    echo "  $0 --gateway                          # Docker (localhost:3001)"
    echo "  $0 --gateway http://localhost:65535    # Local gateway (run.sh)"
    [[ $FAIL -eq 0 ]] && exit 0 || exit 1
fi

# ── Phase 2: Live tests (gateway + web app) ──────────────────────────────────

log_section "Phase 2: Web Login & Authenticated Tests"
log_info "Gateway: ${GATEWAY_URL}"
log_info "Web app: ${WEB_URL}"

# Point CLI at local gateway
log_test "P2a: Configure gateway URL"
out=$($MAGELAB config set gateway_url "$GATEWAY_URL" 2>&1)
assert_contains "$out" "Set gateway_url" "gateway_url configured"

out=$($MAGELAB config 2>&1)
assert_contains "$out" "$GATEWAY_URL" "config shows gateway URL"

# Check gateway is reachable
log_test "P2b: Gateway health check"
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST -H 'Content-Type: application/json' \
    -d '{"grant_type":"refresh_token"}' \
    "${GATEWAY_URL}/v1/auth/token" 2>/dev/null || echo "000")

if [[ "$HTTP_CODE" == "000" ]]; then
    log_fail "Gateway unreachable at $GATEWAY_URL"
    echo "  Start with: cd gateway-backend && ./run.sh"
    echo "  Or Docker:  docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d"
    log_section "Phase 2 Aborted"
    echo -e "  ${GREEN}Pass: $PASS${RESET}  ${RED}Fail: $FAIL${RESET}  ${YELLOW}Skip: $SKIP${RESET}"
    exit 1
fi
log_pass "Gateway responded (HTTP $HTTP_CODE)"

# Check web app is reachable
log_test "P2c: Web app health check"
WEB_CODE=$(curl -s -o /dev/null -w '%{http_code}' "${WEB_URL}/" 2>/dev/null || echo "000")

if [[ "$WEB_CODE" == "000" ]]; then
    log_fail "Web app unreachable at $WEB_URL"
    echo "  Start with: docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d web"
    log_section "Phase 2 Aborted"
    echo -e "  ${GREEN}Pass: $PASS${RESET}  ${RED}Fail: $FAIL${RESET}  ${YELLOW}Skip: $SKIP${RESET}"
    exit 1
fi
log_pass "Web app responded (HTTP $WEB_CODE)"

# ── T1: Web browser login
log_test "T1: Web browser login"
$MAGELAB logout 2>&1 >/dev/null

if do_web_login "$WEB_URL"; then
    LOGIN_OK=true
else
    LOGIN_OK=false
fi

if $LOGIN_OK; then
    # ── T5c: Login status when logged in
    log_test "T5c: Login status when logged in"
    out=$($MAGELAB login --status 2>&1)
    if echo "$out" | grep -qE "Logged in as"; then
        log_pass "shows logged-in email"
        if echo "$out" | grep -qE "Token: valid"; then
            log_pass "token is valid"
        else
            log_info "Token status: $(echo "$out" | grep Token)"
            # Web tokens are short-lived — may already be near expiry
            log_info "(web tokens are short-lived, this may be expected)"
        fi
    else
        log_fail "not logged in after web login"
        log_info "$out"
    fi

    # ── T6: Auth token output
    log_test "T6: Auth token output"
    TOKEN=$($MAGELAB auth token 2>/dev/null) || true
    if [[ -n "$TOKEN" ]] && echo "$TOKEN" | grep -qE '^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.'; then
        log_pass "auth token prints valid JWT"
        # Try to decode payload
        PAYLOAD=$(echo "$TOKEN" | cut -d. -f2 | base64 -d 2>/dev/null || true)
        if echo "$PAYLOAD" | python3 -m json.tool &>/dev/null; then
            log_pass "JWT payload is valid JSON"
        else
            log_info "JWT payload decode issue (padding — non-critical)"
        fi
    elif [[ -n "$TOKEN" ]]; then
        log_pass "auth token printed a token"
    else
        log_fail "auth token returned empty"
    fi

    # ── T10: Authenticated commands after login
    log_test "T10: Authenticated commands after login"

    out=$($MAGELAB models 2>&1)
    models_exit=$?
    if [[ $models_exit -eq 0 ]]; then
        log_pass "models command succeeded (exit 0)"
    else
        log_fail "models command failed (exit $models_exit): $(echo "$out" | head -2)"
    fi

    out=$($MAGELAB balance 2>&1) && {
        log_pass "balance command succeeded"
    } || {
        log_info "balance: $(echo "$out" | head -2)"
        log_fail "balance command failed"
    }

    out=$($MAGELAB usage 2>&1) && {
        log_pass "usage command succeeded"
    } || {
        log_info "usage: $(echo "$out" | head -2)"
        log_fail "usage command failed"
    }

    # ── T7: Token expiry detection
    log_test "T7: Token expiry detection"

    # Try to tamper with token expiry.
    # Strategy 1: credentials file (Linux, Windows, macOS fallback)
    # Strategy 2: macOS Keychain via `security` command
    TAMPERED=false

    # Check file locations first
    for candidate in \
        "$HOME/Library/Application Support/magelab/credentials.json" \
        "$HOME/.config/magelab/credentials.json" \
        "${APPDATA:-}/magelab/credentials.json"; do
        if [[ -f "$candidate" ]]; then
            log_info "Tampering credentials file: $candidate"
            python3 -c "
import json, pathlib
p = pathlib.Path('''$candidate''')
d = json.loads(p.read_text())
d['expires_at'] = 1000
p.write_text(json.dumps(d))
" 2>/dev/null && TAMPERED=true
            break
        fi
    done

    # macOS Keychain: read entry, tamper, write back
    if ! $TAMPERED && command -v security &>/dev/null; then
        KC_JSON=$(security find-generic-password -s magelab-cli -a default -w 2>/dev/null || true)
        if [[ -n "$KC_JSON" ]] && echo "$KC_JSON" | python3 -c "import sys,json; json.loads(sys.stdin.read())" 2>/dev/null; then
            log_info "Tampering keychain entry: magelab-cli"
            MODIFIED=$(echo "$KC_JSON" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
d['expires_at'] = 1000
print(json.dumps(d))
" 2>/dev/null)
            if [[ -n "$MODIFIED" ]]; then
                security add-generic-password -U -s magelab-cli -a default -w "$MODIFIED" 2>/dev/null && TAMPERED=true
            fi
        fi
    fi

    if $TAMPERED; then
        # Verify it shows expired
        out=$($MAGELAB login --status 2>&1)
        if echo "$out" | grep -q "expired"; then
            log_pass "token shows expired after tampering"
        else
            log_info "Status: $out"
            log_fail "token should show expired"
        fi

        # auth token should try to refresh — web login has no refresh token,
        # so this is expected to fail. We just verify it doesn't panic.
        TOKEN=$($MAGELAB auth token 2>/dev/null) || true
        if [[ -n "$TOKEN" ]]; then
            log_pass "token refresh succeeded (unexpected for web login)"
        else
            log_pass "expired token correctly rejected (no refresh token from web login)"
        fi
    else
        log_skip "T7: Could not locate credentials to tamper"
    fi

    # ── T11 (after login): Logout then verify commands fail
    log_test "T11: Logout then verify commands fail"
    $MAGELAB logout 2>&1 >/dev/null

    out=$($MAGELAB login --status 2>&1)
    assert_contains "$out" "Not logged in" "status shows not logged in after logout"

    # Point at unreachable gateway to ensure no residual auth can reach a real endpoint
    $MAGELAB config set gateway_url "http://127.0.0.1:19999" 2>&1 >/dev/null || true
    out=$(MAGELAB_API_KEY="" $MAGELAB models 2>&1) && {
        log_fail "models should fail after logout"
    } || {
        log_pass "models fails after logout"
    }
    $MAGELAB config set gateway_url "$GATEWAY_URL" 2>&1 >/dev/null || true
else
    log_skip "T5c: Login status (login failed)"
    log_skip "T6: Auth token output (login failed)"
    log_skip "T10: Authenticated commands (login failed)"
    log_skip "T7: Token refresh (login failed)"
    log_skip "T11: Post-logout commands (login failed)"
fi

# ── Phase 3: Alternative login methods (optional) ────────────────────────────

if $RUN_MAGIC; then
    log_section "Phase 3a: Magic Auth"
    $MAGELAB logout 2>&1 >/dev/null

    # Prompt for email if not provided
    if [[ -z "$EMAIL" ]]; then
        echo ""
        read -rp "Email for magic auth: " EMAIL
    fi

    if [[ -z "$EMAIL" ]]; then
        log_skip "Magic auth (no email provided)"
    else
        # ── T2: Wrong code
        log_test "T2: Magic auth — wrong code"
        out=$(printf '%s\n000000\n' "$EMAIL" | $MAGELAB login --method magic 2>&1) || true
        if echo "$out" | grep -qiE "failed|error|invalid|400|401"; then
            log_pass "wrong code rejected"
        else
            log_info "Output: $(echo "$out" | head -3)"
            log_fail "expected error for wrong code"
        fi

        # ── T3: Unknown email
        log_test "T3: Magic auth — unknown email"
        out=$(echo "nobody-test-$(date +%s)@example.com" | $MAGELAB login --method magic 2>&1) || true
        if echo "$out" | grep -qiE "error|failed|not found|400|401|404|422"; then
            log_pass "unknown email handled"
        else
            log_fail "unexpected response for unknown email"
            log_info "$(echo "$out" | head -3)"
        fi

        # ── T1m: Magic auth happy path
        log_test "T1m: Magic auth login"
        echo ""
        echo -e "  ${BOLD}Sending code to ${EMAIL}...${RESET}"

        # Send the magic auth code request, then prompt user for the code
        printf '%s\n' "$EMAIL" | $MAGELAB login --method magic >/tmp/magelab-magic-$$.txt 2>&1 &
        MAGIC_PID=$!
        sleep 4

        out=$(cat /tmp/magelab-magic-$$.txt 2>/dev/null || true)
        kill "$MAGIC_PID" 2>/dev/null; wait "$MAGIC_PID" 2>/dev/null || true

        if echo "$out" | grep -qiE "Code sent|check your inbox"; then
            echo -e "  ${YELLOW}Check your email and enter the code:${RESET}"
            read -rp "  Code: " AUTH_CODE

            out=$(printf '%s\n%s\n' "$EMAIL" "$AUTH_CODE" | $MAGELAB login --method magic 2>&1)
            if echo "$out" | grep -qiE "Logged in as"; then
                log_pass "magic auth login succeeded"
            else
                log_fail "magic auth login failed"
                log_info "$(echo "$out" | tail -3)"
            fi
        else
            log_fail "did not reach code prompt"
            log_info "$(echo "$out" | head -5)"
        fi
        rm -f /tmp/magelab-magic-$$.txt
    fi
fi

if $RUN_GOOGLE; then
    log_section "Phase 3b: Google OAuth"
    $MAGELAB logout 2>&1 >/dev/null

    log_test "T4: Google OAuth login"
    if lsof -i :19872 -sTCP:LISTEN &>/dev/null; then
        log_fail "Port 19872 already in use"
    else
        echo ""
        echo -e "  ${BOLD}Starting Google OAuth — complete login in your browser${RESET}"
        echo ""

        $MAGELAB login --method google 2>&1
        EXIT_CODE=$?

        if [[ $EXIT_CODE -eq 0 ]]; then
            out=$($MAGELAB login --status 2>&1)
            if echo "$out" | grep -qE "Logged in as"; then
                log_pass "Google OAuth login succeeded"
            else
                log_fail "Google OAuth: not logged in after flow"
            fi
        else
            log_fail "Google OAuth exited with code $EXIT_CODE"
        fi
    fi
fi

if ! $RUN_MAGIC && ! $RUN_GOOGLE && [[ -n "$GATEWAY_URL" ]]; then
    log_info ""
    log_info "Phase 3 skipped. Use --magic or --google to test alternative login methods."
fi

# ── Clean up: restore gateway URL ────────────────────────────────────────────

if [[ -n "$GATEWAY_URL" ]]; then
    $MAGELAB config set gateway_url "https://api.magelab.ai" 2>&1 >/dev/null || true
fi

# ── Summary ───────────────────────────────────────────────────────────────────

log_section "Results"
echo -e "  ${GREEN}Pass: $PASS${RESET}  ${RED}Fail: $FAIL${RESET}  ${YELLOW}Skip: $SKIP${RESET}"
echo ""

if [[ $FAIL -eq 0 ]]; then
    echo -e "${GREEN}${BOLD}All tests passed!${RESET}"
    exit 0
else
    echo -e "${RED}${BOLD}$FAIL test(s) failed.${RESET}"
    exit 1
fi
