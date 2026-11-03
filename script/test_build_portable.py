"""Portable packaging contracts, runnable without foreign binaries or network."""
import hashlib
import json
import struct
import tarfile
import tempfile
import unittest
import uuid
import zipfile
from pathlib import Path
from unittest.mock import patch

import build_portable as portable
import build_support as support
from build_hybrid_windows import BuildError, verify_zip


class PortableBuildTests(unittest.TestCase):
    def test_preparation_populates_and_reuses_one_platform_wheel_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            (inputs / "runtime").mkdir(parents=True)
            (inputs / "runtime/python.exe").write_bytes(b"synthetic pristine Python")
            (inputs / "pdfium").mkdir()
            assets = inputs / f"offline_assets_{portable.windows.ASSET_TAG}.zip"
            lock = root / "requirements.txt"
            lock.write_bytes(b"synthetic pinned requirements")
            commands = []

            def run(command, work, env, **kwargs):
                commands.append((command, env.copy()))
                self.assertEqual(env["UV_CACHE_DIR"], str(inputs / "uv-cache"))
                if kwargs["label"] == "seed-runtime-install":
                    self.assertIn("--require-hashes", command)
                    (inputs / "uv-cache").mkdir(exist_ok=True)
                else:
                    self.assertIn("--generate-offline-assets", command)
                    assets.write_bytes(b"synthetic verified asset archive")

            with patch.object(portable.windows, "runtime_files"), patch.object(portable.windows, "verify_zip"), patch.object(portable, "platform_profile", return_value=([], lock)), patch.object(portable, "run", side_effect=run), patch.object(portable, "fetch", side_effect=AssertionError("No archive is missing")):
                first = root / "first"
                first.mkdir()
                portable.prepare_inputs("windows-x64", inputs, Path("uv"), Path("bins"), first)
                self.assertEqual(len(commands), 2)
                second = root / "second"
                second.mkdir()
                portable.prepare_inputs("windows-x64", inputs, Path("uv"), Path("bins"), second)
                self.assertEqual(len(commands), 2, "A second preparation must reuse verified cached inputs")
                lock.write_bytes(b"changed pinned requirements")
                third = root / "third"
                third.mkdir()
                portable.prepare_inputs("windows-x64", inputs, Path("uv"), Path("bins"), third)
                self.assertEqual(len(commands), 3, "A changed lock must populate its wheel cache again")

    def test_exact_mac_closure_keeps_existing_versions_without_windows_only_dependency(self):
        mac, _ = portable.platform_profile("macos-arm64")
        win, _ = portable.platform_profile("windows-x64")
        left = {row["name"]: row["version"] for row in win}
        right = {row["name"]: row["version"] for row in mac}
        self.assertEqual(set(left) - set(right), {"colorama"})
        self.assertEqual(right, {key: value for key, value in left.items() if key != "colorama"})
        self.assertTrue(all(row["url"].startswith("https://files.pythonhosted.org/") and len(row["sha256"]) == 64 for row in mac))

    def test_foreign_architecture_never_publishes_a_placeholder(self):
        destination = support.ROOT / "dist" / ("foreign-refused-" + uuid.uuid4().hex)
        with patch.object(portable.sys, "platform", "unsupported"):
            with self.assertRaisesRegex(BuildError, "architecture"):
                portable.main(["--platform", "macos-arm64", "--output", str(destination), "--uv", str(Path(__file__).absolute())])
        self.assertFalse(destination.exists())

    def test_archive_traversal_and_external_links_fail_before_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            malicious = root / "bad.zip"
            with zipfile.ZipFile(malicious, "w") as archive:
                archive.writestr("../outside", b"unexpected")
            with self.assertRaises(BuildError):
                support.extract_archive(malicious, root / "zip")
            linked = root / "bad.tar"
            with tarfile.open(linked, "w") as archive:
                member = tarfile.TarInfo("link")
                member.type = tarfile.SYMTYPE
                member.linkname = "/outside"
                archive.addfile(member)
            with self.assertRaises(tarfile.TarError):
                support.extract_archive(linked, root / "tar")
            self.assertFalse((root / "outside").exists())

    def test_asset_identity_uses_member_hashes_not_zip_timestamps(self):
        data = b"bounded synthetic font bytes"
        expected = {"fonts/example.ttf": {"bytes": len(data), "sha3_256": hashlib.sha3_256(data).hexdigest()}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "assets.zip"
            for year in (2020, 2026):
                with zipfile.ZipFile(path, "w") as archive:
                    archive.writestr(zipfile.ZipInfo("fonts/example.ttf", (year, 1, 1, 0, 0, 0)), data)
                verify_zip(path, expected)
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("fonts/example.ttf", b"x" * len(data))
            with self.assertRaisesRegex(BuildError, "checksum"):
                verify_zip(path, expected)

    def test_manifest_detects_changed_content_and_unexpected_private_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "runtime").mkdir()
            data = root / "runtime/library"
            data.write_bytes(b"verified")
            output = root / "RELEASE_FILES.json"
            support.manifest(root, output)
            support.verify_manifest(root, output)
            data.write_bytes(b"modified")
            with self.assertRaises(BuildError):
                support.verify_manifest(root, output)
            (root / "config.v3.toml").write_text("private = true")
            with self.assertRaises(BuildError):
                support.manifest(root, output)

    def test_macho_requires_arm64_and_preserves_rpath_dependency_names(self):
        dependency = b"@rpath/libexample.dylib\0"
        command_size = (24 + len(dependency) + 7) // 8 * 8
        command = struct.pack("<IIIIII", 0xC, command_size, 24, 0, 0, 0) + dependency
        command += b"\0" * (command_size - len(command))
        header = struct.pack("<IIIIIIII", 0xFEEDFACF, 0x0100000C, 0, 6, 1, len(command), 0, 0)
        parsed = support._macho_commands(header + command)
        self.assertEqual(parsed["dependencies"], ["@rpath/libexample.dylib"])
        bad = bytearray(header + command)
        bad[4:8] = struct.pack("<I", 0x01000007)
        with self.assertRaises(BuildError):
            support._macho_commands(bytes(bad))

    def test_pins_are_platform_specific_https_archives(self):
        pins = json.loads(support.PINS.read_bytes())
        self.assertEqual(set(pins["platforms"]), {"windows-x64", "macos-arm64"})
        for platform in pins["platforms"].values():
            for key in ("python_archive", "pdfium_archive", "uv_wheel"):
                self.assertTrue(platform[key]["url"].startswith("https://"))
                self.assertEqual(len(platform[key]["sha256"]), 64)
                self.assertGreater(platform[key]["bytes"], 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
