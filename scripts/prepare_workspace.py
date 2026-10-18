#!/usr/bin/env python3
"""Apply the release TLS configuration and use the CLI's reviewed workspace lock."""

import argparse
from pathlib import Path
import shutil


NATIVE_TLS = 'reqwest = { version = "0.12", features = ["json", "stream", "native-tls"] }'
RUSTLS = 'reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }'


def prepare(workspace: Path) -> None:
    manifest = workspace / "Cargo.toml"
    content = manifest.read_text(encoding="utf-8")
    if content.count(NATIVE_TLS) == 1:
        content = content.replace(NATIVE_TLS, RUSTLS)
    elif content.count(RUSTLS) != 1:
        raise ValueError("Unexpected workspace reqwest declaration; review TLS dependencies")
    lockfile = workspace / "magelab-cli" / "Cargo.lock"
    if not lockfile.is_file():
        raise ValueError("The CLI repository must contain the reviewed workspace Cargo.lock")
    manifest.write_text(content, encoding="utf-8")
    shutil.copyfile(lockfile, workspace / "Cargo.lock")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=Path.cwd())
    prepare(parser.parse_args().workspace)
