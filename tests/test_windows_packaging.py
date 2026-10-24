"""Protect release scope and provenance without building an entire EXE."""

import hashlib
import json
from pathlib import Path

import pytest
import toml
import yaml
from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel
from script.package_windows import finalize_bundle
from script.package_windows import stage_packages


def test_runtime_staging_keeps_resources_and_omits_environment_state(tmp_path):
    source = tmp_path / "installed"
    for name in (
        "package/__init__.py",
        "package/config/default.yaml",
        "package/resources/gui.css",
        "package/tests/test_example.py",
        "package/test_example.py",
        "package/_tests/example.py",
        "package/stats/_bws_test.py",
        "package/__pycache__/module.pyc",
        "package-1.dist-info/licenses/LICENSE.txt",
        "package-1.dist-info/direct_url.json",
        "_virtualenv.py",
        "_virtualenv.pth",
    ):
        path = source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("fixture", encoding="utf-8")
    target = tmp_path / "release" / "site-packages"
    stage_packages(source, target)
    assert (target / "package/config/default.yaml").is_file()
    assert (target / "package/resources/gui.css").is_file()
    assert (target / "package-1.dist-info/licenses/LICENSE.txt").is_file()
    assert not (target / "package/tests").exists()
    assert not (target / "package/test_example.py").exists()
    assert not (target / "package/_tests").exists()
    assert (target / "package/stats/_bws_test.py").exists()
    assert not (target / "package/__pycache__").exists()
    assert not list(target.rglob("direct_url.json"))
    assert not (target / "_virtualenv.pth").exists()


def test_staging_refuses_to_overwrite_existing_release(tmp_path):
    source = tmp_path / "source"
    target = tmp_path / "release"
    source.mkdir()
    target.mkdir()
    existing = target / "keep.txt"
    existing.write_text("stable", encoding="utf-8")
    with pytest.raises(FileExistsError):
        stage_packages(source, target)
    assert existing.read_text(encoding="utf-8") == "stable"


def test_manifest_refuses_local_configuration_in_release(tmp_path):
    (tmp_path / "config").mkdir()
    with pytest.raises(ValueError, match="local state: config"):
        finalize_bundle(tmp_path)
    assert not (tmp_path / "RELEASE_FILES.json").exists()


def test_release_manifest_hashes_shipped_bytes_and_records_notices(tmp_path):
    bundle = tmp_path / "bundle"
    package = bundle / "site-packages/example-1.0.dist-info"
    package.mkdir(parents=True)
    (package / "METADATA").write_text(
        "Metadata-Version: 2.4\nName: example\nVersion: 1.0\nLicense-Expression: MIT\n",
        encoding="utf-8",
    )
    (bundle / "pdf2zh.exe").write_bytes(b"launcher fixture")
    count, size = finalize_bundle(bundle)
    manifest = json.loads((bundle / "RELEASE_FILES.json").read_text())
    assert count == len(manifest)
    assert size == sum(row["bytes"] for row in manifest)
    launcher = next(row for row in manifest if row["path"] == "pdf2zh.exe")
    assert launcher["sha256"] == hashlib.sha256(b"launcher fixture").hexdigest()
    assert all(row["path"] != "RELEASE_FILES.json" for row in manifest)
    notices = json.loads((bundle / "THIRD_PARTY_COMPONENTS.json").read_text())
    assert notices[0]["name"] == "example"
    assert notices[0]["license"] == "MIT"


@pytest.mark.parametrize(
    ("filename", "directml"),
    [
        ("exe-build.yml", False),
        ("release.yml", True),
        ("fork-build.yml", True),
        ("rebuild.yml", True),
    ],
)
def test_windows_release_entrypoints_use_the_locked_builder(filename, directml):
    root = Path(__file__).resolve().parents[1]
    workflow = yaml.safe_load(
        (root / ".github/workflows" / filename).read_text(encoding="utf-8")
    )
    steps = workflow["jobs"]["build-win64-exe"]["steps"]
    scripts = "\n".join(step.get("run", "") for step in steps)
    assert "./script/build_windows.ps1" in scripts
    assert ("-DirectML" in scripts) is directml
    assert "uv pip install" not in scripts
    assert workflow["env"]["WIN_EXE_PYTHON_VERSION"] == "3.13.16"


def test_shipped_config_template_is_valid_and_local_only():
    path = (
        Path(__file__).resolve().parents[1]
        / "script/windows-release/config.example.toml"
    )
    config = CLIEnvSettingsModel(**toml.load(path))
    assert config.basic.gui is True
    assert config.gui_settings.server_name == "127.0.0.1"
    assert config.gui_settings.share is False
    assert config.openai_detail.openai_api_key is None
