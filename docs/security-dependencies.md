# Dependency security review

Reviewed on 2026-10-27 for the CLI 0.1.17 candidate, using the pinned workspace/core revision `10518d6a21a9a2f56f20813913fde5cbfdb5d61e`.

## Audit evidence

`cargo-audit 0.22.2` checked all 457 packages in the reviewed workspace lock against RustSec database commit `99670cfc0bc574433034b75876a4b791ccaa2efb` (1,380 advisories; database updated 2026-10-27). The workspace and CLI copies of `Cargo.lock` were identical, with SHA256 `91837e3da57596bde9b7ff69fbf2106bb0c1fca63595b2b542511e470b9a47ba`.

The audit still exits unsuccessfully: four vulnerability reports remain across two upstream dependencies, plus three maintenance warnings. No advisory is suppressed. This is not a clean Rust audit.

Compatible updates include Rustls 0.23.46, h2 0.4.16, webpki-roots and webpki-root-certs 1.0.10, anyhow 1.0.103, Tokio 1.53.3, quinn-proto 0.11.18, tar 0.4.46, and serial_test 3.5.0. The serial_test update removes the affected scc dependency. The TLS and HTTP crates are in the CLI's normal runtime graph; quinn-proto is not enabled by the release's selected features.

From the prepared workspace, rerun with current RustSec data:

```sh
cargo audit --file Cargo.lock
```

The Pi extension's production and full pnpm audits reported zero vulnerabilities.
The reviewed extension lock selects ws 8.22.0, Vitest 4.1.12, Vite 7.3.7, and
esbuild 0.28.1; Vitest no longer depends on Tinypool. From `extension/`, rerun
`pnpm audit --prod` and `pnpm audit`. Packaged `mage setup-pi` installs only runtime
dependencies; test tools remain in the development checkout.

## Stronghold's crypto-bigint dependency

[`RUSTSEC-2026-0387`](https://rustsec.org/advisories/RUSTSEC-2026-0387.html) reports secret-dependent timing in crypto-bigint arithmetic, including subtraction, zero counting, division and Montgomery multiplication. The advisory lists versions at or above 0.7.6 as patched. Exploitability depends on which secret operations are used and whether the attacker can observe their timing or microarchitectural effects.

The CLI's normal dependency graph contains `crypto-bigint 0.5.5` through `elliptic-curve 0.13.8 → k256 0.13.4 → iota-crypto 0.23.2 → iota_stronghold 2.1.0 → magelab-core`. Stronghold explicitly enables iota-crypto's secp256k1 feature. Its constraints require iota-crypto 0.23, k256 0.13, elliptic-curve 0.13, and crypto-bigint 0.5. The current published Stronghold and iota-crypto releases are already the latest stable versions; there is no compatible lockfile-only fix.

Source review of the pinned core's `crates/magelab-core/src/vault.rs` found calls to Stronghold's store and snapshot APIs only: loading/creating clients, inserting/reading/deleting bytes, and committing snapshots. The inner Stronghold object is private. Neither core nor CLI source calls `execute_procedure`, secp256k1 signing, or hierarchical key derivation. Stronghold's snapshot implementation uses the age format with scrypt and ChaCha20-Poly1305; core derives its vault key with Argon2id. Those reviewed paths do not use k256 integer arithmetic.

This supports treating the advisory as an upstream follow-up for current CLI use, not declaring the dependency fixed. Reassess before adding Stronghold signing/key-derivation procedures or changing vault use. A fix requires a compatible upstream release or a separately reviewed backport; forcing an incompatible crypto-bigint version is not an appropriate lockfile repair.

## Windows build-time ZIP extraction

`zip 8.6.0` remains a build dependency of `libsodium-sys-stable 1.24.0`, used by Stronghold. It has three reports:

- [`RUSTSEC-2026-0367`](https://rustsec.org/advisories/RUSTSEC-2026-0367.html): preserving SUID/SGID bits during extraction.
- [`RUSTSEC-2026-0368`](https://rustsec.org/advisories/RUSTSEC-2026-0368.html): extracting symlinks that point outside the destination.
- [`RUSTSEC-2026-0369`](https://rustsec.org/advisories/RUSTSEC-2026-0369.html): applying permissions through symlinks outside the destination.

The reported fixes require zip 9.0.0; libsodium-sys-stable's current constraint selects zip 8.x. The ZIP parser is not in the shipped CLI's runtime dependency graph. The Windows MSVC build script does call `ZipArchive::extract`, after verifying the libsodium distribution archive with its embedded Minisign public key. This restricts the affected input to authenticated upstream build archives; it does not make extraction intrinsically safe or remove the advisory.

Track a compatible libsodium-sys-stable update or reviewed extraction fix. Do not reuse this build helper for untrusted ZIP archives or suppress the reports as resolved.

## Maintenance warnings

- [`RUSTSEC-2025-0141`](https://rustsec.org/advisories/RUSTSEC-2025-0141.html): bincode 1.3.3, through Stronghold.
- [`RUSTSEC-2025-0119`](https://rustsec.org/advisories/RUSTSEC-2025-0119.html): number_prefix 0.4.0, through indicatif.
- [`RUSTSEC-2024-0436`](https://rustsec.org/advisories/RUSTSEC-2024-0436.html): paste 1.0.15, a Stronghold procedural macro dependency.

These are maintenance notices without a patched version in the advisory, not additional vulnerability reports. They remain tracked upgrade/replacement work.
