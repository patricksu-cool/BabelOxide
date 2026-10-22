import sys
from pathlib import Path

import pdf2zh_next.config.main as config_main
import pdf2zh_next.const as const
import pytest
from pdf2zh_next.config.main import ConfigManager


def test_source_config_directory_ignores_working_directory(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
):
    monkeypatch.delenv("PDF2ZH_APP_DIR", raising=False)
    monkeypatch.delattr(sys, "frozen", raising=False)
    monkeypatch.chdir(tmp_path)

    project_root = Path(const.__file__).resolve().parent.parent
    assert const.application_directory() == project_root
    assert const.DEFAULT_CONFIG_FILE.parent == const.DEFAULT_CONFIG_DIR
    assert const.WRITE_TEMP_CONFIG_FILE.parent == const.DEFAULT_CONFIG_DIR
    assert (
        const.VERSION_DEFAULT_CONFIG_FILE.parent == const.DEFAULT_CONFIG_DIR / "default"
    )


def test_executable_config_directory_ignores_working_directory(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
):
    app_root = tmp_path / "portable-app"
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("PDF2ZH_APP_DIR", str(app_root))
    assert const.application_directory() == app_root

    monkeypatch.delenv("PDF2ZH_APP_DIR")
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "executable", str(app_root / "pdf2zh.exe"))
    assert const.application_directory() == app_root


def test_installed_python_and_invalid_override(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
):
    monkeypatch.delenv("PDF2ZH_APP_DIR", raising=False)
    monkeypatch.delattr(sys, "frozen", raising=False)
    monkeypatch.setattr(
        const,
        "__file__",
        str(tmp_path / "Lib" / "site-packages" / "pdf2zh_next" / "const.py"),
    )
    monkeypatch.setattr(sys, "executable", str(tmp_path / "Scripts" / "python.exe"))
    assert const.application_directory() == tmp_path / "Scripts"

    monkeypatch.setenv("PDF2ZH_APP_DIR", "relative/path")
    with pytest.raises(ValueError, match="absolute"):
        const.application_directory()


def test_legacy_config_migrates_once_and_round_trips(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
):
    new_config_dir = tmp_path / "portable-app" / "config"
    new_config_file = new_config_dir / "config.v3.toml"
    version_dir = new_config_dir / "default"
    version_file = version_dir / "2.9.0.toml"
    old_config_file = tmp_path / "home" / ".config" / "pdf2zh" / "config.v3.toml"
    old_config_file.parent.mkdir(parents=True)
    original_bytes = b"[basic]\ndebug = true\n"
    old_config_file.write_bytes(original_bytes)

    monkeypatch.setattr(config_main, "DEFAULT_CONFIG_DIR", new_config_dir)
    monkeypatch.setattr(config_main, "DEFAULT_CONFIG_FILE", new_config_file)
    monkeypatch.setattr(
        config_main, "WRITE_TEMP_CONFIG_FILE", new_config_dir / "config.v3.temp.toml"
    )
    monkeypatch.setattr(config_main, "VERSION_DEFAULT_CONFIG_DIR", version_dir)
    monkeypatch.setattr(config_main, "VERSION_DEFAULT_CONFIG_FILE", version_file)
    monkeypatch.setattr(config_main, "LEGACY_CONFIG_FILE", old_config_file)
    monkeypatch.setattr(sys, "argv", ["pdf2zh_next", "--google", "--version"])

    config_manager = ConfigManager()
    monkeypatch.setattr(config_manager, "_default_config_file_path", new_config_file)
    monkeypatch.setattr(config_manager, "parse_env_vars", lambda: {})
    settings = config_manager.initialize_cli_config()

    assert settings.basic.debug is True
    assert new_config_file.read_bytes() == original_bytes
    assert old_config_file.read_bytes() == original_bytes
    assert version_file.is_file()

    old_config_file.write_bytes(b"[basic]\ndebug = false\n")
    assert config_manager.initialize_cli_config().basic.debug is True
    assert new_config_file.read_bytes() == original_bytes

    settings.basic.debug = False
    config_manager.write_user_default_config_file(settings)
    assert config_manager.initialize_cli_config().basic.debug is False
    assert old_config_file.read_bytes() == b"[basic]\ndebug = false\n"
