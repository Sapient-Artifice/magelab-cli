# Building and releasing the desktop companion CLI

The standalone CLI inherits Cargo workspace dependencies and uses the sibling
`crates/magelab-core`. A release therefore has two source revisions: this CLI's
commit and the workspace/core commit pinned in `.github/workflows/build.yml`.
The reviewed workspace revision is
`10518d6a21a9a2f56f20813913fde5cbfdb5d61e`.

## Prepare an isolated workspace

Use Python 3.11+ and a stable Rust toolchain. The example starts in the CLI
checkout and creates a separate temporary workspace; it does not replace the
CLI in an existing monorepo or install anything into the user's PATH.

```sh
cli_source=$(pwd)
cli_revision=$(git rev-parse HEAD)
release_workspace=$(mktemp -d)
git clone https://github.com/Sapient-Artifice/magelab.git "$release_workspace"
git -C "$release_workspace" checkout 10518d6a21a9a2f56f20813913fde5cbfdb5d61e
# Replace only the CLI subtree in this new temporary checkout.
rm -rf "$release_workspace/magelab-cli"
git clone --no-hardlinks "$cli_source" "$release_workspace/magelab-cli"
git -C "$release_workspace/magelab-cli" checkout "$cli_revision"
cd "$release_workspace"
python magelab-cli/scripts/prepare_workspace.py
```

`prepare_workspace.py` applies the reviewed Rustls configuration and copies the
CLI repository's `Cargo.lock` to the workspace root. Cargo uses the root lockfile
for workspace members; it ignores a member's own lockfile. Keep the reviewed
workspace lock in this CLI repository so the release cannot silently resolve a
different dependency set. Build and test with `--locked`; if a dependency change
requires lockfile updates, regenerate and review that lock in this same workspace
and copy it back into the CLI repository before committing.

The CI workflow uses the same preparation script after checking out the exact
workspace commit and overlaying the CLI revision being tested. The private
workspace checkout requires the `MAGELAB_READ_TOKEN` repository secret.

## Validate the candidate

Run the Rust suite in an isolated account/config environment. Some existing
credential tests access the real OS keychain unless `MAGELAB_SKIP_KEYCHAIN_TESTS`
is set, so keep this variable enabled for automated release validation.

```sh
python -m unittest discover -s magelab-cli/scripts -p 'test_*.py' -v
python magelab-cli/scripts/verify_release.py --expected-tag v0.1.17
cargo fmt --check -p magelab-cli
cargo clippy --locked -p magelab-cli -- -D warnings
MAGELAB_SKIP_KEYCHAIN_TESTS=1 cargo test --locked --workspace
cargo build --locked --release -p magelab-cli
python magelab-cli/scripts/verify_release.py --expected-tag v0.1.17 --binary target/release/mage
```

On Windows the executable is `target/release/mage.exe`. Set environment
variables using the shell's equivalent syntax.

The verifier requires the package version, tag, and actual `mage --version`
output to agree. It then checks all twelve commands required by the desktop:
`skills add/list/update/remove`, `mcp approve/revoke/policy`, `sessions`, `chats`,
`ask`, `internal auth-token`, and `internal vault-secrets`. These bounded
`--help` probes exercise the parsers without accessing credentials or a backend.
The Rust suite separately exercises command behavior against mock services.

The shared build workflow also installs the locked Pi extension dependencies,
type-checks the extension, and runs its full test suite before publication.

Review both production and development dependency audits when updating either
lockfile. The [dependency security review](security-dependencies.md) records the
0.1.17 patches and the remaining upstream findings with their current exposure.
Those findings remain visible in RustSec; a passing test suite is not a clean
security audit.

## Platform gates and publication

`CI` runs on pull requests, `main`, and `release/**` branches. It and the tag
release workflow call the same build workflow, which runs the complete core/CLI
test suite and builds and verifies the release executable on native hosts:

| Asset | Target | Runner |
| --- | --- | --- |
| `mage-linux-x86_64` | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` |
| `mage-linux-aarch64` | `aarch64-unknown-linux-gnu` | `ubuntu-22.04-arm` |
| `mage-macos-x86_64` | `x86_64-apple-darwin` | `macos-15-intel` |
| `mage-macos-aarch64` | `aarch64-apple-darwin` | `macos-15` |
| `mage-windows-x86_64.exe` | `x86_64-pc-windows-msvc` | `windows-2022` |

The two macOS assets are combined with `lipo`. The resulting
`mage-macos-universal` is verified on both Apple Silicon and Intel before the
shared workflow succeeds. No cross-target executable skips the parser checks.

Before tagging, update `Cargo.toml` and the reviewed workspace `Cargo.lock`,
commit the candidate, and wait for its branch CI to pass. Push an exact `vX.Y.Z`
tag pointing to that tested commit to run the release workflow. A tag/package
version mismatch fails before compilation. The release job only starts after
every target and both universal-binary checks succeed.

The release job assembles the six expected binaries, creates and checks
`checksums.txt`, uploads all seven files into a draft release, then publishes it.
It refuses to overwrite an already published release; fixes require a new
version. A failed draft upload can be retried before publication.

After publication, download the actual release assets, verify their published
checksums and native executable contracts, and update the desktop's exact
`DEFAULT_VERSION` pin. Run the desktop bundler against the published release
before shipping the desktop. Passing these tests does not replace the desktop's
installer, signing, or live backend interoperability checks.
