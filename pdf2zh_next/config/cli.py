"""Build CLI arguments from typed settings, without configuration persistence."""

import argparse
import logging
import typing
from inspect import getdoc
from types import NoneType
from typing import Any
from typing import Literal

from pydantic import BaseModel

from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel

log = logging.getLogger(__name__)

_no_duplicate_field_name = set()
_no_duplicate_field_name.add("translate_engine_type")
_no_duplicate_field_name.add("support_llm")


class MagicDefault:
    pass


def build_args_parser(
    parser: argparse.ArgumentParser | None = None,
    settings_model: type[BaseModel] | None = None,
    field_name2type: dict[str, Any] | None = None,
    recursion_depth: int = 0,
    set_count: int = 0,
) -> tuple[argparse.ArgumentParser, dict[str, Any]]:
    if parser is None:
        parser = argparse.ArgumentParser()

    if field_name2type is None:
        field_name2type = {}

    if settings_model is None:
        settings_model = CLIEnvSettingsModel
    elif isinstance(parser, argparse.ArgumentParser):
        parser = parser.add_argument_group(
            title=settings_model.__name__,
            description=getdoc(settings_model),
        )

    for field_name, field_detail in settings_model.model_fields.items():
        if field_name not in _no_duplicate_field_name and field_name in field_name2type:
            log.critical(f"duplicate field name: {field_name}")
            raise ValueError(f"duplicate field name: {field_name}")
        field_name2type[field_name] = field_detail.annotation
        if field_detail.default_factory is not None:
            if recursion_depth > 0:
                raise ValueError("not supported nested settings models")
            build_args_parser(
                parser,
                field_detail.default_factory,
                field_name2type,
                recursion_depth + 1,
                set_count,
            )
        else:
            type_hint = typing.get_type_hints(settings_model)[field_name]
            original_type = typing.get_origin(type_hint)
            args = typing.get_args(type_hint)
            log.debug(
                f"field_name: {field_name}, type_hint: {type_hint}, original_type: {original_type}, args: {args}"
            )
            if original_type is Literal:
                continue
            if original_type is None:
                args = [type_hint]
            args_name = field_name.replace("_", "-").lower()

            if original_type is set:
                if set_count > 0:
                    raise ValueError("not supported multiple set arguments")

                if len(args) > 1:
                    raise ValueError("not supported multiple set arguments")

                if args[0] is not str:
                    raise ValueError("set type must be str")

                set_count += 1
                parser.add_argument(
                    f"{args_name}",
                    nargs="*",
                    type=str,
                    help=field_detail.description,
                )
                continue

            for arg in args:
                if arg is bool:
                    parser.add_argument(
                        f"--{args_name}",
                        action="store_true"
                        if field_detail.default is False
                        else "store_false",
                        default=MagicDefault,
                        help=field_detail.description,
                    )
                elif arg == NoneType:
                    continue
                else:
                    parser.add_argument(
                        f"--{args_name}",
                        type=arg,
                        default=MagicDefault,
                        help=field_detail.description,
                    )
    return parser, field_name2type
