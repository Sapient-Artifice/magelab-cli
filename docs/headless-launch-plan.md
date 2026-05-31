# Headless Backend Launch Plan

## Problem

`mage launch` is intended to start the MageLab backend without opening the desktop UI, but the current implementation only understands a development-style repository layout:

```text
<magelab_home>/backend/main.py
```

Packaged desktop installs use a different layout. For example, macOS installs the bundled API under:

```text
/Applications/magelab.app/Contents/Resources/bin/api/backend/main.py
/Applications/magelab.app/Contents/Resources/bin/api/python/bin/python3
```

As a result, users can sign into the desktop app and successfully use local backend features while the app is open, but `mage launch` may fail to start that same backend headlessly. Setting `magelab_home` to the `.app` root does not work because the CLI checks the wrong relative path. Setting it to the packaged `bin/api` directory may pass backend discovery, but the CLI still falls back to system `python3` instead of using the bundled Python.

## Command Contract

The CLI should make these responsibilities explicit:

```text
mage login
```

Authenticates the CLI with MageLab cloud. It opens a browser, receives the callback on `127.0.0.1:19872`, exchanges the login code, and stores credentials in the system keychain or `~/.config/magelab/credentials.json`.

It must not launch the desktop UI or the backend.

```text
mage launch
```

Starts the local MageLab backend in headless mode. It must not open the desktop UI.

It should discover the packaged backend, choose the bundled Python runtime, spawn `uvicorn`, and detach the child process so it survives the CLI invocation.

It should not perform interactive authentication. Users should run `mage login` before `mage launch --wait` when they want the backend initialized with account credentials.

```text
mage launch --wait
```

Starts the backend, waits until the health endpoint is ready, prints the local URL, and pushes available vault secrets or auth material into the backend.

```text
mage connect
```

Resolves a usable connection:

1. Use an explicitly supplied backend URL, if provided.
2. Use an already-running backend at `config.local_url`.
3. Launch the local backend headlessly if allowed.
4. Use relay if available.
5. Use remote REST fallback if authenticated.
6. Report no connection.

Proposed one-off connection flags:

```text
mage connect --url http://127.0.0.1:8787
mage connect --url http://192.168.1.50:8787
mage connect --ws ws://192.168.1.50:8787/ws
```

These flags should probe the supplied endpoint without mutating `cli.toml`. Users who want a persistent default should continue to use:

```sh
mage config set local_url http://127.0.0.1:8787
```

Current behavior is equivalent to:

```text
mage connect
```

probing `config.local_url`, which defaults to:

```text
http://127.0.0.1:11115
```

If a backend is running on another host or port, current users must update `local_url` before connecting:

```sh
mage config set local_url http://192.168.1.50:8787
mage connect
```

For remote hosts, the backend must be launched with a reachable bind address such as `0.0.0.0`, and firewall plus browser origin settings must allow the client.

Desktop UI launch is out of scope for this plan. `mage launch` remains headless-only.

## Desired Packaged Layout Support

The launcher should understand these packaged backend layouts.

### macOS

Default app install:

```text
/Applications/magelab.app/Contents/Resources/bin/api
```

Expected files:

```text
python/bin/python3
backend/main.py
```

Launch command:

```sh
/Applications/magelab.app/Contents/Resources/bin/api/python/bin/python3 \
  -m uvicorn main:app \
  --app-dir /Applications/magelab.app/Contents/Resources/bin/api/backend \
  --host 127.0.0.1 \
  --port 11115 \
  --log-level warning
```

### Linux

Default deb install:

```text
/usr/lib/magelab/bin/api
```

Expected files:

```text
python/bin/python3
backend/main.py
```

Launch command:

```sh
/usr/lib/magelab/bin/api/python/bin/python3 \
  -m uvicorn main:app \
  --app-dir /usr/lib/magelab/bin/api/backend \
  --host 127.0.0.1 \
  --port 11115 \
  --log-level warning
```

### Windows

Default per-user install:

```text
%LOCALAPPDATA%\magelab\bin\api
```

Expected files:

```text
python\python.exe
backend\main.py
```

Launch command:

```powershell
& "$env:LOCALAPPDATA\magelab\bin\api\python\python.exe" `
  -m uvicorn main:app `
  --app-dir "$env:LOCALAPPDATA\magelab\bin\api\backend" `
  --host 127.0.0.1 `
  --port 11115 `
  --log-level warning
```

## Configuration Semantics

Keep `magelab_home`, but define it as an install root or API root, not as a path to `main.py`.

Accepted `magelab_home` values should include:

```text
/path/to/mage-lab
/Applications/magelab.app
/Applications/magelab.app/Contents/Resources/bin/api
/usr/lib/magelab
/usr/lib/magelab/bin/api
%LOCALAPPDATA%\magelab
%LOCALAPPDATA%\magelab\bin\api
```

The CLI should normalize any of these into a concrete backend bundle:

```rust
struct BackendBundle {
    api_dir: PathBuf,
    backend_dir: PathBuf,
    python: PathBuf,
}
```

If a user points `magelab_home` at `main.py`, the CLI should reject it with a clear error:

```text
magelab_home should point to the MageLab install root or bundled API directory, not backend/main.py.
```

## Discovery Order

Discovery should be deterministic and easy to debug.

1. `MAGELAB_API_DIR`, if set.
2. `MAGELAB_HOME`, if set.
3. `magelab_home` from `cli.toml`, if set.
4. Development layouts near the CLI binary or current working directory.
5. Packaged platform defaults.
6. Platform-specific search fallback.

Recommended explicit override:

```text
MAGELAB_API_DIR=/Applications/magelab.app/Contents/Resources/bin/api
```

This avoids ambiguity when a machine has both development and packaged installs.

## Launch Implementation

Replace the current `find_magelab_home` plus `find_python` coupling with bundle discovery.

Current launcher effectively assumes:

```text
home -> home/backend -> backend/.venv/bin/python or system python3
```

Target launcher should resolve:

```text
home or api_dir -> BackendBundle { api_dir, backend_dir, python }
```

Then spawn:

```sh
<python> -m uvicorn main:app \
  --app-dir <backend_dir> \
  --host <host> \
  --port <port> \
  --log-level warning
```

Use `--app-dir` instead of relying on `current_dir`. Setting `current_dir` to `backend_dir` is still acceptable, but `--app-dir` makes the command match packaged usage and is easier to inspect in process listings.

Default host should remain `127.0.0.1`. Binding to `0.0.0.0` should require an explicit flag or config setting because it exposes the API to the network.

Suggested flags:

```text
mage launch --host 127.0.0.1 --port 11115
mage launch --host 0.0.0.0 --port 8787
```

The port should continue to default from `config.local_url` for backward compatibility.

## Auth and Secret Handoff

`mage login` authenticates the CLI, not the running backend.

Recommended user flow:

```sh
mage login
mage launch --wait
mage connect
```

`mage launch` should not open a browser, prompt for login, or perform cloud sign-in itself. Authentication remains explicit so headless startup can work in scripts and server environments.

After `mage launch --wait`, the CLI should initialize the backend with the same auth material the desktop app normally provides at startup:

1. Open the shared vault if available.
2. Push secrets to the backend using the existing backend secret endpoint.
3. If no vault secret exists but the CLI has a valid token or API key, push the supported auth material.
4. Warn clearly if the backend starts but has no usable credentials.

If the user launches first and logs in afterward, they should have an explicit way to initialize the already-running backend:

```sh
mage launch --wait
mage login
mage vault push
```

Longer term, consider a clearer auth-specific command:

```text
mage auth push
```

That command would push the currently available CLI/vault credentials to the backend at `config.local_url` or an explicit `--url`.

Plain `mage launch` can remain fast and detached, but it should print a hint when initialization requires `--wait`:

```text
Backend launched.
Run `mage launch --wait` to wait for readiness and push credentials.
```

Alternatively, make `--wait` the default once launch is reliable, and provide `--detach` for the fire-and-forget behavior.

## Desktop UI Launch

Out of scope. Do not make `mage launch` open the desktop app. The headless CLI path should work independently of whether the desktop UI is running.

## User-Facing Diagnostics

Add a diagnostic command or verbose flag:

```text
mage launch --dry-run
mage launch --verbose
```

It should print:

```text
Backend bundle: /Applications/magelab.app/Contents/Resources/bin/api
Backend dir:    /Applications/magelab.app/Contents/Resources/bin/api/backend
Python:         /Applications/magelab.app/Contents/Resources/bin/api/python/bin/python3
Host:           127.0.0.1
Port:           11115
Log:            ~/.config/magelab/backend.log
```

This would make installation issues obvious without requiring users to read the source.

## Test Plan

Unit tests:

- Resolves dev layout: `<root>/backend/main.py`.
- Resolves packaged API layout: `<root>/bin/api/backend/main.py`.
- Resolves macOS `.app` root to `Contents/Resources/bin/api`.
- Resolves Linux root `/usr/lib/magelab` to `bin/api`.
- Resolves Windows root `%LOCALAPPDATA%\magelab` to `bin\api`.
- Rejects `magelab_home` pointing directly to `main.py`.
- Prefers bundled Python over system Python.
- Falls back to dev `.venv` Python for repository layouts.

Integration tests:

- `mage launch --dry-run` prints the selected bundle without spawning.
- `mage launch --wait` starts a fake or minimal test backend and waits for `/health`.
- `mage connect` launches headless when no backend is running and `--no-launch` is not set.
- `mage connect --no-launch` never spawns a backend.

Manual tests:

- macOS packaged app installed in `/Applications`.
- macOS packaged app installed in a custom location via `magelab_home`.
- Linux deb install.
- Windows per-user install under `%LOCALAPPDATA%`.
- Binding to `127.0.0.1`.
- Binding to `0.0.0.0` with explicit opt-in.

## Migration Notes

This change should preserve existing development workflows. A developer with a sibling `mage-lab/backend/main.py` should still be able to run:

```sh
mage launch --wait
```

The main behavioral change is that packaged installs become first-class launch targets and use the packaged Python runtime instead of system Python.

## Recommendation

Land this as a CLI behavior cleanup, not as a documentation-only change.

The CLI should treat `mage login`, `mage launch`, and any future desktop-opening command as separate operations:

- `mage login`: authenticate the CLI.
- `mage launch`: start the backend headlessly.
- `mage connect`: find or create a usable backend connection.
- `mage desktop`: open the graphical app, if added.

That split matches user expectations, server use cases, and the packaged backend layout across macOS, Linux, and Windows.
