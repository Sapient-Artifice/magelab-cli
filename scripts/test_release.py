import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock

from prepare_workspace import NATIVE_TLS, RUSTLS, prepare
from verify_release import REQUIRED_COMMANDS, manifest_version, verify_binary


class ReleaseVerificationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binary = self.root / "mage"
        self.binary.touch()
        self.manifest = self.root / "Cargo.toml"
        self.manifest.write_text('[package]\nname = "magelab-cli"\nversion = "0.1.13"\n')

    def test_tag_must_match_package_version(self):
        self.assertEqual(manifest_version(self.manifest, "v0.1.13"), "0.1.13")
        for tag in ("v0.1.12", "0.1.13", "latest"):
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, "does not match"):
                manifest_version(self.manifest, tag)

    def test_old_embedded_version_fails_before_command_checks(self):
        run = Mock(return_value=subprocess.CompletedProcess([], 0, "mage 0.1.0\n", ""))
        with self.assertRaisesRegex(ValueError, "version mismatch"):
            verify_binary(self.binary, "0.1.13", run)
        self.assertEqual(run.call_count, 1)

    def test_all_required_commands_use_only_bounded_help_probes(self):
        run = Mock(side_effect=[
            subprocess.CompletedProcess([], 0, "mage 0.1.13\n", ""),
            *[subprocess.CompletedProcess([], 0, "Usage: mage command\n", "") for _ in REQUIRED_COMMANDS],
        ])
        verify_binary(self.binary, "0.1.13", run)
        self.assertEqual(run.call_count, 13)
        self.assertEqual(
            [call.args[0][1:] for call in run.call_args_list],
            [["--version"], *[[*command, "--help"] for command in REQUIRED_COMMANDS]],
        )
        for call in run.call_args_list:
            self.assertEqual(call.kwargs["timeout"], 10)
            self.assertTrue(call.kwargs["check"])

    def test_missing_command_blocks_release(self):
        run = Mock(side_effect=[
            subprocess.CompletedProcess([], 0, "mage 0.1.13\n", ""),
            subprocess.CalledProcessError(2, ["mage", "skills", "add", "--help"]),
        ])
        with self.assertRaises(subprocess.CalledProcessError):
            verify_binary(self.binary, "0.1.13", run)

    def test_missing_help_or_hanging_command_blocks_release(self):
        for result in (
            subprocess.CompletedProcess([], 0, "", ""),
            subprocess.TimeoutExpired(["mage", "skills", "add", "--help"], 10),
        ):
            run = Mock(side_effect=[subprocess.CompletedProcess([], 0, "mage 0.1.13\n", ""), result])
            with self.subTest(result=result), self.assertRaises((ValueError, subprocess.TimeoutExpired)):
                verify_binary(self.binary, "0.1.13", run)


class WorkspacePreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "magelab-cli").mkdir()
        (self.root / "magelab-cli" / "Cargo.lock").write_text("reviewed lock\n")
        (self.root / "Cargo.lock").write_text("upstream lock\n")

    def test_release_uses_rustls_and_reviewed_lock(self):
        (self.root / "Cargo.toml").write_text("[workspace.dependencies]\n" + NATIVE_TLS + "\n")
        prepare(self.root)
        self.assertIn(RUSTLS, (self.root / "Cargo.toml").read_text())
        self.assertEqual((self.root / "Cargo.lock").read_text(), "reviewed lock\n")
        prepare(self.root)
        self.assertIn(RUSTLS, (self.root / "Cargo.toml").read_text())

    def test_unreviewed_tls_change_fails_without_replacing_lock(self):
        (self.root / "Cargo.toml").write_text('[workspace.dependencies]\nreqwest = "0.13"\n')
        with self.assertRaisesRegex(ValueError, "Unexpected workspace reqwest"):
            prepare(self.root)
        self.assertEqual((self.root / "Cargo.lock").read_text(), "upstream lock\n")


if __name__ == "__main__":
    unittest.main()
