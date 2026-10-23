"""The launcher must never widen network access implicitly."""

import sys

import pytest

# Input to a mocked launcher; these tests never bind an external interface.
ANY_INTERFACE = "0.0.0.0"  # noqa: S104


@pytest.fixture
def gui(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["pytest"])
    from pdf2zh_next import gui

    return gui


def test_default_binding_overrides_gradio_environment(gui, monkeypatch):
    calls = []
    monkeypatch.setenv("GRADIO_SERVER_NAME", ANY_INTERFACE)
    monkeypatch.setenv("GRADIO_SHARE", "true")
    monkeypatch.setattr(gui.demo, "launch", lambda **kw: calls.append(kw))
    gui.setup_gui(inbrowser=False)
    assert calls[0]["server_name"] == "127.0.0.1"
    assert calls[0]["share"] is False
    assert str(gui.DEFAULT_CONFIG_DIR) in calls[0]["blocked_paths"]


def test_explicit_network_binding_warns(gui, monkeypatch, caplog):
    calls = []
    monkeypatch.setattr(gui.demo, "launch", lambda **kw: calls.append(kw))
    gui.setup_gui(server_name=ANY_INTERFACE)
    assert calls[0]["server_name"] == ANY_INTERFACE
    assert "Security warning" in caplog.text


def test_launch_failure_does_not_enable_sharing(gui, monkeypatch):
    calls = []

    def fail(**kw):
        calls.append(kw)
        raise OSError("synthetic bind failure")

    monkeypatch.setattr(gui.demo, "launch", fail)
    with pytest.raises(OSError, match="synthetic bind failure"):
        gui.setup_gui()
    assert len(calls) == 1
    assert calls[0]["share"] is False


def test_empty_auth_file_fails_closed(gui, monkeypatch):
    monkeypatch.setattr(gui, "parse_user_passwd", lambda *_: ([], None))
    with pytest.raises(ValueError, match="no valid users"):
        gui.setup_gui(auth_file="empty.txt")


def test_cli_accepts_explicit_server_name():
    from pdf2zh_next.config.main import build_args_parser

    parser, _ = build_args_parser()
    args = parser.parse_args(["--gui", "--server-name", ANY_INTERFACE])
    assert args.server_name == ANY_INTERFACE
