"""Gradio control metadata and conditional visibility rules."""

import typing

import gradio as gr


def _field_gui_extra(field) -> dict:
    return (field.json_schema_extra or {}).get("gui", {})


def _type_hint_allows_none(type_hint: typing.Any) -> bool:
    return type(None) in typing.get_args(type_hint)


def _gui_dependency_field_name(field_name: str, dependency_field_name: str) -> str:
    if field_name.startswith("term_") and not dependency_field_name.startswith("term_"):
        return f"term_{dependency_field_name}"
    return dependency_field_name


def _gui_field_visible(
    field_name: str,
    field,
    base_visible: bool,
    field_values: dict[str, typing.Any],
) -> bool:
    gui_extra = _field_gui_extra(field)
    visible_when = gui_extra.get("visible_when")
    if not visible_when:
        return base_visible
    dependency_field_name = _gui_dependency_field_name(
        field_name, visible_when["field"]
    )
    return (
        base_visible
        and field_values.get(dependency_field_name) == visible_when["equals"]
    )


def _gui_field_value(field, value):
    gui_extra = _field_gui_extra(field)
    if value:
        return value
    return gui_extra.get("default_on_show", value)


def _gui_field_update(field, visible: bool, current_value=None):
    gui_extra = _field_gui_extra(field)
    update_kwargs = {"visible": visible}
    if "choices" in gui_extra:
        update_kwargs["choices"] = gui_extra["choices"]
    if "default_on_show" in gui_extra or gui_extra.get("preserve_current_value"):
        update_kwargs["value"] = _gui_field_value(field, current_value)
    return gr.update(**update_kwargs)
