import sys
from types import SimpleNamespace

import pytest
from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel


@pytest.fixture
def gui(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["pytest"])
    import pdf2zh_next.gui as module

    return module


@pytest.mark.parametrize(
    ("mode", "inputs", "expected"),
    [
        ("RPM", {"rpm_input": 59}, (1, 10)),
        ("RPM", {"rpm_input": 120}, (2, 20)),
        ("Concurrent Threads", {"concurrent_threads": 1}, (1, 1)),
        ("Concurrent Threads", {"concurrent_threads": 20}, (1, 1)),
        ("Concurrent Threads", {"concurrent_threads": 40}, (20, 20)),
        ("Concurrent Threads", {"concurrent_threads": 10000}, (1000, 1000)),
        ("Custom", {"custom_qps": 8.0, "custom_pool_workers": 0}, (8, None)),
        ("Custom", {"custom_qps": 2, "custom_pool_workers": 3}, (2, 3)),
    ],
)
def test_gui_rate_conversion_boundaries(gui, mode, inputs, expected):
    assert gui._calculate_rate_limit_params(mode, inputs) == expected


@pytest.mark.parametrize(
    ("mode", "inputs"),
    [
        ("RPM", {"rpm_input": 1.5}),
        ("Concurrent Threads", {"concurrent_threads": 0}),
        ("Custom", {"custom_qps": -1}),
        ("Custom", {"custom_qps": 1, "custom_pool_workers": -1}),
    ],
)
def test_gui_rate_conversion_rejects_original_invalid_values(gui, mode, inputs):
    with pytest.raises(ValueError):
        gui._calculate_rate_limit_params(mode, inputs)


def test_positional_controls_keep_main_and_term_field_order(gui):
    values = list(range(49))
    values[0], values[19] = "Google", None
    state = {}
    values[34] = state
    main_names = gui.__gui_service_arg_names
    term_names = gui.__gui_term_service_arg_names
    details = [f"main-{i}" for i in range(len(main_names))]
    terms = [f"term-{i}" for i in range(len(term_names))]
    result = gui.build_ui_inputs(*values, *details, *terms)
    assert result["state"] is state
    assert result["concurrent_threads"] == 12
    assert result["custom_qps"] == 13
    assert result["custom_pool_workers"] == 14
    assert result["term_custom_pool_workers"] == 48
    assert result["glossaries"] is None
    assert [result[name] for name in main_names] == details
    assert [result[name] for name in term_names] == terms


@pytest.mark.parametrize(
    ("mode_name", "disable_auto_save", "should_save"),
    [
        ("never", False, False),
        ("always", True, True),
        ("follow_settings", False, True),
        ("follow_settings", True, False),
    ],
)
def test_settings_save_modes_preserve_saved_and_current_run_values(
    gui, monkeypatch, tmp_path, mode_name, disable_auto_save, should_save
):
    function = next(
        fn
        for fn in gui.demo.fns.values()
        if fn.fn and fn.fn.__name__ == "translate_files"
    )
    inputs = gui.build_ui_inputs(*(control.value for control in function.inputs[3:]))
    inputs.update(service="Google", page_range="Range", page_input="1-2")
    base = CLIEnvSettingsModel()
    base.translation.output = "previous-output"
    base.pdf.pages = "2-3"
    base.gui_settings.disable_config_auto_save = disable_auto_save
    saved = []
    manager = SimpleNamespace(
        config_cli_settings=base.clone(),
        write_user_default_config_file=lambda settings: saved.append(settings),
    )
    monkeypatch.setattr(gui, "config_manager", manager)
    monkeypatch.setattr(gui, "settings", base)
    pdf = tmp_path / "input.pdf"
    pdf.write_bytes(b"%PDF-1.4\n")
    result = gui._build_translate_settings(
        base, pdf, tmp_path, getattr(gui.SaveMode, mode_name), inputs
    )
    assert result.translation.output == str(tmp_path)
    assert result.pdf.pages == "1-2"
    assert base.translation.output == "previous-output" and base.pdf.pages == "2-3"
    assert len(saved) == int(should_save)
    if should_save:
        assert gui.settings is saved[0]
        assert saved[0].translation.output == "previous-output"
        assert saved[0].pdf.pages == "2-3"
        assert saved[0].basic.gui is False and saved[0].basic.debug is False
    else:
        assert gui.settings is base
