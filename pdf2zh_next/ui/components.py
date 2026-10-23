"""Declare provider controls from field metadata without GUI session globals."""

import typing

import gradio as gr

from pdf2zh_next.ui.fields import _field_gui_extra
from pdf2zh_next.ui.fields import _gui_field_value
from pdf2zh_next.ui.fields import _gui_field_visible


def create_engine_field(
    name, field, value, service_visible, values, *, password=False, scope="translation"
):
    """Share widget declaration while callers retain provider order and policy."""
    type_hint = field.annotation
    type_args = typing.get_args(type_hint)
    extra = _field_gui_extra(field)
    common = {
        "label": field.description,
        "interactive": True,
        "visible": _gui_field_visible(name, field, service_visible, values),
    }
    if extra.get("widget") == "dropdown":
        component = gr.Dropdown(
            choices=extra["choices"], value=_gui_field_value(field, value), **common
        )
    elif type_hint is str or str in type_args or type_hint is int or int in type_args:
        component = gr.Textbox(
            value=value, type="password" if password else "text", **common
        )
    elif type_hint is bool or bool in type_args:
        component = gr.Checkbox(value=value, **common)
    else:
        raise Exception(
            f"Unsupported type {type_hint} for field {name} in gui {scope} engine settings"
        )
    return component, extra
