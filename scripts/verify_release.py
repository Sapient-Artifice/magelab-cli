#!/usr/bin/env python3
"""Check release identity and the desktop command contract without backend access."""

import argparse
from pathlib import Path
import re
import subprocess
import tomllib


REQUIRED_COMMANDS = (
    ("skills", "add"),
    ("skills", "list"),
    ("skills", "update"),
    ("skills", "remove"),
    ("mcp", "approve"),
    ("mcp", "revoke"),
    ("mcp", "policy"),
    ("sessions",),
    ("chats",),
    ("ask",),
    ("internal", "auth-token"),
    ("internal", "vault-secrets"),
)


def manifest_version(manifest: Path, expected_tag: str | None = None) -> str:
    with manifest.open("rb") as source:
        version = tomllib.load(source)["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
        raise ValueError(f"Invalid package version: {version}")
    if expected_tag is not None and expected_tag != f"v{version}":
        raise ValueError(f"Release tag {expected_tag!r} does not match package version v{version}")
    return version


def verify_binary(binary: Path, version: str, run=subprocess.run) -> None:
    binary = binary.resolve(strict=True)

    def invoke(args: tuple[str, ...]) -> str:
        result = run(
            [str(binary), *args],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
        return result.stdout.strip()

    actual = invoke(("--version",))
    if actual != f"mage {version}":
        raise ValueError(f"CLI version mismatch: expected mage {version}, got {actual!r}")
    for command in REQUIRED_COMMANDS:
        output = invoke((*command, "--help"))
        if "Usage:" not in output:
            raise ValueError(f"CLI help missing usage for {' '.join(command)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest", type=Path, default=Path(__file__).resolve().parents[1] / "Cargo.toml"
    )
    parser.add_argument("--expected-tag")
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    version = manifest_version(args.manifest, args.expected_tag)
    if args.binary is not None:
        verify_binary(args.binary, version)
        print(f"Verified mage {version}: all {len(REQUIRED_COMMANDS)} desktop command checks passed")
    else:
        print(f"Verified package version {version}")


if __name__ == "__main__":
    main()
