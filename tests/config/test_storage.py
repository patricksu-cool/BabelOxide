from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from threading import Barrier
from threading import Event
from threading import Lock

import pdf2zh_next.config.main as config_main
import pytest
import tomlkit
from pdf2zh_next.config import storage
from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel
from pdf2zh_next.config.main import ConfigManager


def _local_manager(monkeypatch, tmp_path):
    monkeypatch.setattr(config_main, "DEFAULT_CONFIG_DIR", tmp_path)
    monkeypatch.setattr(config_main, "DEFAULT_CONFIG_FILE", tmp_path / "config.v3.toml")
    monkeypatch.setattr(
        config_main, "WRITE_TEMP_CONFIG_FILE", tmp_path / "shared.temp.toml"
    )
    monkeypatch.setattr(config_main, "VERSION_DEFAULT_CONFIG_DIR", tmp_path / "default")
    return ConfigManager()


def test_concurrent_saves_both_finish_with_complete_config(monkeypatch, tmp_path):
    manager = _local_manager(monkeypatch, tmp_path)
    original_write = manager._write_toml_file
    barrier = Barrier(2)
    original_replace = Path.replace
    replacement_started = Lock()
    replacement_finished = Event()

    def replace_in_order(path, target):
        # Model the valid interleaving where one save renames before the other.
        if path.name != "shared.temp.toml":
            return original_replace(path, target)
        first = replacement_started.acquire(blocking=False)
        if not first:
            assert replacement_finished.wait(timeout=10)
        try:
            return original_replace(path, target)
        finally:
            if first:
                replacement_finished.set()

    def write_then_wait(path, content):
        original_write(path, content)
        barrier.wait(timeout=10)

    monkeypatch.setattr(manager, "_write_toml_file", write_then_wait)
    monkeypatch.setattr(Path, "replace", replace_in_order)
    settings = [CLIEnvSettingsModel(), CLIEnvSettingsModel()]
    settings[0].translation.lang_out = "fr"
    settings[1].translation.lang_out = "de"
    with ThreadPoolExecutor(max_workers=2) as executor:
        futures = [
            executor.submit(manager.write_user_default_config_file, s) for s in settings
        ]
        for future in futures:
            future.result(timeout=15)

    result = manager._read_toml_file(tmp_path / "config.v3.toml")
    assert result["translation"]["lang_out"] in {"fr", "de"}
    assert set(result) == set(settings[0].model_dump())
    assert not list(tmp_path.glob("*.temp.toml"))


def test_interrupted_write_preserves_existing_file(monkeypatch, tmp_path):
    manager = _local_manager(monkeypatch, tmp_path)
    target = tmp_path / "config.v3.toml"
    original = b'[translation]\nlang_out = "fr"\n'
    target.write_bytes(original)

    def interrupted_dump(_content, stream):
        stream.write("[translation]\nlang_out =")
        raise OSError("synthetic interrupted write")

    monkeypatch.setattr(tomlkit, "dump", interrupted_dump)
    with pytest.raises(OSError, match="synthetic interrupted write"):
        manager._write_toml_file(target, {"translation": {"lang_out": "de"}})

    assert target.read_bytes() == original
    assert sorted(p.name for p in tmp_path.iterdir()) == [target.name]


def test_replace_failure_preserves_existing_file_and_cleans_temp(monkeypatch, tmp_path):
    target = tmp_path / "config.toml"
    target.write_bytes(b"value = 1\n")

    def denied_replace(source, destination):
        assert Path(source).parent == target.parent
        assert destination == target
        raise PermissionError("synthetic sharing violation")

    monkeypatch.setattr(storage.os, "replace", denied_replace)
    with pytest.raises(PermissionError, match="synthetic sharing violation"):
        storage.write_toml_atomic(target, {"value": 2})
    assert target.read_bytes() == b"value = 1\n"
    assert list(tmp_path.iterdir()) == [target]


def test_independent_writers_use_distinct_temporary_files(monkeypatch, tmp_path):
    target = tmp_path / "config.toml"
    original_dump = tomlkit.dump
    barrier = Barrier(2)
    temporary_names = []

    def dump_then_wait(content, stream):
        temporary_names.append(stream.name)
        original_dump(content, stream)
        barrier.wait(timeout=10)

    monkeypatch.setattr(tomlkit, "dump", dump_then_wait)
    with ThreadPoolExecutor(max_workers=2) as executor:
        futures = [
            executor.submit(storage.write_toml_atomic, target, {"value": value})
            for value in (1, 2)
        ]
        for future in futures:
            future.result(timeout=15)
    assert len(set(temporary_names)) == 2
    assert tomlkit.parse(target.read_text())["value"] in {1, 2}
    assert list(tmp_path.iterdir()) == [target]
