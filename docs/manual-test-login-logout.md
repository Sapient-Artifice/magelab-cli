# Manual Test: Login/Logout (WorkOS Auth)

Test the CLI's login and logout flows against local servers.

## Prerequisites

### 1. Gateway backend running

The gateway proxies WorkOS API calls (requires `WORKOS_API_KEY` and `WORKOS_CLIENT_ID`).

```bash
cd gateway-backend
./run.sh  # Starts on port 65535
```

Verify it's up:

```bash
curl http://localhost:65535/v1/auth/token \
  -X POST -H 'Content-Type: application/json' \
  -d '{"grant_type":"refresh_token"}' 2>&1 | head -5
# Should return 400/422, not connection refused
```

### 2. Point CLI at local gateway

```bash
cd magelab-cli
cargo run -- config set gateway_url http://localhost:65535
```

Verify:

```bash
cargo run -- config
# Should show: gateway_url = "http://localhost:65535"
```

### 3. (Optional) SaaS frontend running on :3007

Only needed if you want to test the auth URL override path through the web frontend's proxy.

```bash
cd magelab-saas-frontend
GATEWAY_BACKEND_URL=http://localhost:65535 pnpm run dev --port 3007
```

---

## Test Cases

### T1: Magic auth login (happy path)

This is the default login method. It sends a code to your email via WorkOS, then exchanges it for tokens through the gateway.

```bash
cargo run -- login
```

Expected:
1. Prompts `Email:` — enter your email
2. Prints `Sending login code to <email>...`
3. Prints `Code sent! Check your inbox.`
4. Prompts `Code:` — enter the 6-digit code from email
5. Prints `Authenticating...`
6. Prints `Logged in as <email>!`
7. Prints `Credentials saved to ~/.config/magelab/credentials.json`

Verify:

```bash
cargo run -- login --status
# Expected: "Logged in as: <email>" and "Token: valid"
```

### T2: Magic auth login — wrong code

```bash
cargo run -- login
```

1. Enter valid email
2. When prompted for code, enter `000000`

Expected: error from WorkOS (e.g., `Token exchange failed (400): ...`)

### T3: Magic auth login — unknown email

```bash
cargo run -- login
```

1. Enter an email not registered in WorkOS (e.g., `nobody@example.com`)

Expected:
- `No MageLab account found for nobody@example.com.`
- Offers option to sign in with Google or sign up
- Entering `2` prints signup URL and exits

### T4: Google OAuth login

```bash
cargo run -- login --method google
```

Expected:
1. Prints `Opening browser for login...`
2. Browser opens to Google OAuth consent screen (via WorkOS)
3. After granting, browser shows "Login successful! You can close this tab."
4. Terminal prints `Exchanging authorization code...`
5. Prints `Logged in as <email>!`

Note: Requires port `19872` to be free (loopback callback server).

If port is busy:

```bash
lsof -i :19872
# Kill the conflicting process, then retry
```

### T5: Login status

```bash
# When logged in:
cargo run -- login --status
# Expected:
#   Logged in as: <email>
#   Token: valid

# With API key set:
MAGELAB_API_KEY=sk-test-12345678 cargo run -- login --status
# Expected: also shows "API key: sk-t...5678"
```

### T6: Auth token output

```bash
cargo run -- auth token
```

Expected: prints raw JWT to stdout (no newline). Useful for piping:

```bash
# Verify it's a valid JWT
cargo run -- auth token | cut -d. -f2 | base64 -d 2>/dev/null | python3 -m json.tool
```

### T7: Token refresh

After logging in, manually expire the token to test refresh:

```bash
# 1. Check current state
cargo run -- login --status
# Token: valid

# 2. Tamper with expiry (set to past)
python3 -c "
import json, pathlib
p = pathlib.Path.home() / '.config/magelab/credentials.json'
if p.exists():
    d = json.loads(p.read_text())
    d['expires_at'] = 1000
    p.write_text(json.dumps(d))
    print('Expired token manually')
else:
    # Try keychain — credentials may be stored there instead
    print('Credentials not in file (check keychain)')
"

# 3. Request a token — should auto-refresh via refresh_token
cargo run -- auth token
# Expected: prints a new valid JWT (not an error)

# 4. Verify status shows valid again
cargo run -- login --status
# Token: valid
```

Note: If credentials are stored in macOS Keychain rather than the file, you'll need to use Keychain Access.app to edit the `magelab-cli` entry, or clear and re-login:

```bash
cargo run -- logout
cargo run -- login
```

### T8: Logout

```bash
cargo run -- logout
```

Expected: `Logged out.`

Verify:

```bash
cargo run -- login --status
# Expected: "Not logged in."

cargo run -- auth token
# Expected: error — "Not logged in. Run: magelab login"
```

### T9: Logout when already logged out

```bash
cargo run -- logout
cargo run -- logout
```

Expected: both succeed with `Logged out.` (idempotent).

### T10: Authenticated commands after login

After a successful login, verify auth works end-to-end:

```bash
cargo run -- models
# Expected: table of available models from gateway

cargo run -- balance
# Expected: account balance info

cargo run -- usage
# Expected: usage summary
```

### T11: Authenticated commands after logout

```bash
cargo run -- logout
cargo run -- models
# Expected: error — "Not authenticated. Run: magelab login"
```

---

## Alternative: Auth through SaaS frontend proxy

If you want to test with the frontend proxy (`:3007`) instead of hitting the gateway directly:

```bash
MAGELAB_AUTH_URL=http://localhost:3007/api/gateway/auth cargo run -- login
```

This routes `magic-auth` and `token` calls through the SaaS frontend's gateway proxy at `/api/gateway/auth/*`, which forwards to the gateway backend's `/v1/auth/*`.

Note: The frontend proxy requires an authenticated Supabase session for most routes. Auth endpoints may or may not be gated depending on your frontend config — if you get 401s, use the direct gateway approach above instead.

---

## Environment variable reference

| Variable | Purpose | Example |
|----------|---------|---------|
| `gateway_url` (config) | Gateway base URL | `http://localhost:65535` |
| `MAGELAB_AUTH_URL` | Override auth endpoint base | `http://localhost:65535/v1/auth` |
| `MAGELAB_API_KEY` | API key (skips JWT auth) | `mage_abc123...` |
| `WORKOS_CLIENT_ID` | Override WorkOS client ID | `client_01KKJ...` |

## Troubleshooting

**"Failed to connect to gateway for token exchange"**
- Gateway not running. Start it with `cd gateway-backend && ./run.sh`

**"WorkOS not configured on this gateway"**
- Gateway missing `WORKOS_API_KEY` or `WORKOS_CLIENT_ID` in its `.env`

**"Failed to start loopback server on port 19872"**
- Another `magelab login --method google` instance is running, or something else is on that port
- `lsof -i :19872` to find it

**"OAuth state mismatch"**
- Stale browser tab from a previous login attempt. Close it and retry.

**Token shows "expired" immediately after login**
- Check system clock: `date -u` vs actual UTC. Token expiry is timestamp-based.
