"""The Windows launcher must not print a command-line secret on failure."""

import multiprocessing
import runpy
import sys
from pathlib import Path

import pytest
from pdf2zh_next import main


@pytest.mark.parametrize("inline", [False, True])
def test_pystand_redacts_early_exception(monkeypatch, capsys, inline):
    secret = "SYNTHETIC_PYSTAND_ONLY_9b37c"  # noqa: S105 - test value
    argv = ["pdf2zh.exe", "--openai-api-key"]
    if inline:
        argv[-1] += f"={secret}"
    else:
        argv.append(secret)

    async def fail_before_config():
        raise RuntimeError(f"upstream failure: {secret}")

    monkeypatch.setattr(sys, "argv", argv)
    monkeypatch.setattr(main, "main", fail_before_config)
    monkeypatch.setattr(multiprocessing, "set_executable", lambda _path: None)
    launcher = Path(__file__).resolve().parents[1] / "script/_pystand_static.int"

    with pytest.raises(SystemExit) as result:
        runpy.run_path(str(launcher))

    captured = capsys.readouterr()
    assert result.value.code == 1
    assert secret not in captured.out + captured.err
    assert "[REDACTED]" in captured.err
    assert "RuntimeError" in captured.err
