"""The Windows launcher keeps failures private and interruptions orderly."""

import asyncio
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


def test_pystand_interrupt_closes_pending_tasks_without_fatal_traceback(
    monkeypatch, capsys
):
    released = []

    async def pending_worker():
        try:
            await asyncio.Event().wait()
        finally:
            released.append(True)

    async def interrupt_with_pending_work():
        task = asyncio.create_task(pending_worker())
        await asyncio.sleep(0)
        assert not task.done()
        raise KeyboardInterrupt

    monkeypatch.setattr(sys, "argv", ["pdf2zh.exe", "--gui"])
    monkeypatch.setattr(main, "main", interrupt_with_pending_work)
    monkeypatch.setattr(multiprocessing, "set_executable", lambda _path: None)
    launcher = Path(__file__).resolve().parents[1] / "script/_pystand_static.int"

    with pytest.raises(SystemExit) as result:
        runpy.run_path(str(launcher))

    captured = capsys.readouterr()
    assert result.value.code == 130
    assert released == [True]
    assert "Interrupted by user." in captured.err
    assert "Traceback" not in captured.err
