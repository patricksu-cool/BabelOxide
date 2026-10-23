from __future__ import annotations

import ast
import copy
import logging
import os
import threading
import typing
from pathlib import Path
from types import NoneType
from typing import Any
from typing import get_args
from typing import get_origin

import tomlkit
from pydantic import BaseModel

from pdf2zh_next.config.cli import MagicDefault as MagicDefault
from pdf2zh_next.config.cli import _no_duplicate_field_name
from pdf2zh_next.config.cli import build_args_parser as build_args_parser
from pdf2zh_next.config.cli_env_model import CLIEnvSettingsModel
from pdf2zh_next.config.model import SettingsModel
from pdf2zh_next.config.storage import decode_toml_content
from pdf2zh_next.config.storage import write_toml_atomic
from pdf2zh_next.config.translate_engine_model import TRANSLATION_ENGINE_METADATA
from pdf2zh_next.const import DEFAULT_CONFIG_DIR
from pdf2zh_next.const import DEFAULT_CONFIG_FILE
from pdf2zh_next.const import LEGACY_CONFIG_FILE
from pdf2zh_next.const import VERSION_DEFAULT_CONFIG_DIR
from pdf2zh_next.const import VERSION_DEFAULT_CONFIG_FILE
from pdf2zh_next.const import WRITE_TEMP_CONFIG_FILE as WRITE_TEMP_CONFIG_FILE
from pdf2zh_next.security_logging import install_redacting_filters
from pdf2zh_next.security_logging import redact_for_log
from pdf2zh_next.security_logging import register_secret_values

# The following is magic code,
# if you need to modify it,
# please contact the maintainer!

_translation_engine_flag_names = [x.cli_flag_name for x in TRANSLATION_ENGINE_METADATA]
_term_translation_engine_flag_names = [
    f"term_{x.cli_flag_name}" for x in TRANSLATION_ENGINE_METADATA if x.support_llm
]

log = logging.getLogger(__name__)


class ConfigManager:
    """Singleton configuration manager"""

    _instance: ConfigManager | None = None
    _settings: SettingsModel | None = None
    config_cli_settings: CLIEnvSettingsModel | None = None
    _default_config_file_path = DEFAULT_CONFIG_FILE
    _config_file_lock = threading.Lock()

    def __new__(cls) -> ConfigManager:
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

    def _ensure_config_dir(self) -> None:
        """Ensure the configuration directory exists"""
        DEFAULT_CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        VERSION_DEFAULT_CONFIG_DIR.mkdir(parents=True, exist_ok=True)

    def _migrate_legacy_user_config(self) -> None:
        """Copy the legacy TOML once without overwriting local settings."""
        if DEFAULT_CONFIG_FILE.exists() or not LEGACY_CONFIG_FILE.is_file():
            return

        created = False
        try:
            original_bytes = LEGACY_CONFIG_FILE.read_bytes()
            with DEFAULT_CONFIG_FILE.open("xb") as destination:
                created = True
                destination.write(original_bytes)
        except FileExistsError:
            return
        except OSError as exc:
            if created:
                DEFAULT_CONFIG_FILE.unlink(missing_ok=True)
            log.warning(
                "Could not migrate legacy configuration %s: %s", LEGACY_CONFIG_FILE, exc
            )
            return

        log.info("Copied legacy configuration to %s", DEFAULT_CONFIG_FILE)

    def _read_toml_file(self, file_path: Path) -> dict:
        """Read and parse a TOML file

        Args:
            file_path: Path to the TOML file

        Returns:
            Parsed TOML content as dictionary
        """
        try:
            with self._config_file_lock:
                with file_path.open(encoding="utf-8") as f:
                    content = tomlkit.load(f)
            # Convert "null" strings back to None
            parsed = self._process_toml_content(dict(content))
            register_secret_values(parsed)
            return parsed
        except FileNotFoundError:
            log.debug(f"Config file not found: {file_path}")
            return {}
        except Exception as e:
            log.warning("Error reading config file %s: %s", file_path, type(e).__name__)
            return {}

    def _process_toml_content(self, content: dict) -> dict:
        """Decode the existing TOML null representation."""
        return decode_toml_content(content)

    def _write_toml_file(self, file_path: Path, content: dict) -> None:
        """Persist a complete TOML file without exposing partial writes."""
        try:
            with self._config_file_lock:
                write_toml_atomic(file_path, content)
        except Exception as exc:
            log.warning(
                "Error writing config file %s: %s", file_path, type(exc).__name__
            )
            raise

    def _is_file_content_identical(self, file_path: Path, content: dict) -> bool:
        """Check if file content is identical to the given content

        Args:
            file_path: Path to the file to check
            content: Content to compare with

        Returns:
            True if content is identical, False otherwise
        """
        try:
            existing_content = self._read_toml_file(file_path)
            return existing_content == content
        except Exception as e:
            log.warning(f"Error comparing file content: {e}")
            return False

    def _get_default_config(self) -> dict:
        """Get default configuration from model

        Returns:
            Default configuration as dictionary
        """
        default_model = CLIEnvSettingsModel()
        config_dict = default_model.model_dump(mode="json")

        # Flatten the dictionary to match TOML processing behavior
        return config_dict

    def _update_version_default_config(self) -> None:
        """Update version default configuration file if needed"""
        self._ensure_config_dir()
        default_config = self._get_default_config()

        # Always write if file doesn't exist
        if (
            not VERSION_DEFAULT_CONFIG_FILE.exists()
            or not self._is_file_content_identical(
                VERSION_DEFAULT_CONFIG_FILE, default_config
            )
        ):
            self._write_toml_file(VERSION_DEFAULT_CONFIG_FILE, default_config)
            log.debug("Updated version default configuration file")

    def parse_cli_args(self) -> None:
        """Parse command line arguments"""
        parser, _ = build_args_parser()
        args = parser.parse_args()
        cli_args = {
            k.replace("-", "_"): v
            for k, v in vars(args).items()
            if v is not MagicDefault
        }

        cli_parsed_args = self.parse_dict_vars(
            dict_vars=cli_args,
        )
        log.debug("CLI args: %s", redact_for_log(cli_parsed_args))
        self._settings = self._build_model_from_args(
            CLIEnvSettingsModel, cli_parsed_args
        ).to_settings_model()
        log.debug("Settings from CLI: %s", redact_for_log(self._settings))

    def parse_env_vars(
        self,
        dedup_field_name: set[str] | None = None,
        recursion_depth: int = 0,
        settings_model: type[BaseModel] | None = None,
    ) -> dict:
        return self.parse_dict_vars(
            dedup_field_name,
            recursion_depth,
            settings_model,
            os.environ,
            prefix="PDF2ZH_",
        )

    def parse_dict_vars(
        self,
        dedup_field_name: set[str] | None = None,
        recursion_depth: int = 0,
        settings_model: type[BaseModel] | None = None,
        dict_vars: dict | None = None,
        prefix: str = "",
    ) -> dict:
        """Parse environment variables into a settings dictionary

        Returns:
            Dictionary with settings derived from environment variables
        """

        env_settings = {}
        if dedup_field_name is None:
            dedup_field_name = set()
        if settings_model is None:
            settings_model = CLIEnvSettingsModel

        dict_vars = {k.replace("-", "_").upper(): v for k, v in dict_vars.items()}
        for field_name, field_detail in settings_model.model_fields.items():
            if (
                field_name not in _no_duplicate_field_name
                and field_name in dedup_field_name
            ):
                log.critical(f"duplicate field name: {field_name}")
                raise ValueError(f"duplicate field name: {field_name}")
            dedup_field_name.add(field_name)
            if field_detail.default_factory is not None:
                if recursion_depth > 0:
                    raise ValueError("not supported nested settings models")
                parsed = self.parse_dict_vars(
                    dedup_field_name,
                    recursion_depth + 1,
                    field_detail.default_factory,
                    dict_vars,
                    prefix=prefix,
                )
                if parsed:
                    env_settings[field_name] = parsed
            else:
                type_hint = typing.get_type_hints(settings_model)[field_name]
                env_name = f"{prefix}{field_name.upper()}"

                if env_name in dict_vars:
                    # Get the value from environment
                    env_value = dict_vars[env_name]
                    register_secret_values({field_name: env_value})

                    # Get type information for proper conversion
                    origin_type = get_origin(type_hint)
                    type_args = get_args(type_hint)

                    # Convert the value to the appropriate type
                    try:
                        converted_value = self._convert_env_value(
                            env_value, type_hint, origin_type, type_args
                        )
                        # Add to settings dict
                        env_settings[field_name] = converted_value
                    except (ValueError, TypeError) as e:
                        log.warning(
                            "Could not convert environment variable %s: %s",
                            env_name,
                            type(e).__name__,
                        )
                    else:
                        log.debug(f"Field {field_name} not found in type hints")

        if recursion_depth == 0:
            log.debug("Environment settings: %s", redact_for_log(env_settings))
        return env_settings

    def _convert_env_value(
        self, value: str, type_hint: Any, origin_type: Any, type_args: tuple
    ) -> Any:
        """
        Convert environment variable string value to the appropriate type

        Args:
            value: String value from environment variable
            type_hint: The type hint for the field
            origin_type: The origin type (e.g., list, dict)
            type_args: Type arguments for generic types

        Returns:
            Converted value matching the expected type
        """
        # Handle Union types (including Optional)
        if origin_type is typing.Union:
            # Try each type in the Union until one works
            for arg in type_args:
                if arg is NoneType and value.lower() in ("none", ""):
                    return None
                try:
                    return self._convert_env_value(
                        value, arg, get_origin(arg), get_args(arg)
                    )
                except (ValueError, TypeError):
                    continue
            # If no conversion worked, raise error
            raise ValueError(
                f"Could not convert '{value}' to any of the types: {type_args}"
            )
        elif origin_type is set:
            if isinstance(value, list):
                return set(value)
            elif isinstance(value, set):
                return value
            elif isinstance(value, str):
                literal_eval = ast.literal_eval(value)
                if literal_eval is None:
                    return set()
                if isinstance(literal_eval, set):
                    return literal_eval
                if isinstance(literal_eval, list):
                    return set(literal_eval)
            else:
                raise ValueError(f"Could not convert '{value}' to set")
        # Handle basic types
        if type_hint is bool:
            if isinstance(value, bool):
                return value
            return value.lower() in ("true", "1", "yes", "y", "on")
        elif type_hint is int:
            return int(value)
        elif type_hint is float:
            return float(value)
        elif type_hint is str:
            return value
        args = typing.get_args(type_hint)
        for arg in args:
            if arg is bool:
                if isinstance(value, bool):
                    return value
                return value.lower() in ("true", "1", "yes", "y", "on")
            elif arg is int:
                return int(value)
            elif arg is float:
                return float(value)
            elif arg is str:
                return value
        # For other types, try direct conversion
        return type_hint(value)

    def _deep_merge(self, target: dict, source: dict) -> dict:
        """
        Deep merge two dictionaries

        Args:
            target: Target dictionary to merge into
            source: Source dictionary to merge from

        Returns:
            Merged dictionary
        """
        for key, value in source.items():
            if (
                key in target
                and isinstance(target[key], dict)
                and isinstance(value, dict)
            ):
                self._deep_merge(target[key], value)
            elif (
                key in target
                and isinstance(target[key], list)
                and isinstance(value, list)
            ):
                target[key].extend(value)
            else:
                target[key] = value
        return target

    def merge_settings(self, config_dicts: list[dict]) -> dict:
        """
        Merge multiple configuration dictionaries based on priority

        Args:
            config_dicts: List of config dictionaries, ordered by priority (highest first)

        Returns:
            Merged configuration dictionary
        """
        result = {}
        enabled_engine = None
        enabled_term_engine = None

        # Determine which translation and term engines are enabled based on priority.
        # config_dicts is ordered from highest to lowest priority, so once an engine
        # type is selected, it must not be overridden by lower-priority configs.
        for config in config_dicts:
            if enabled_engine is None:
                for engine_name in _translation_engine_flag_names:
                    if config.get(engine_name, False):
                        enabled_engine = engine_name
                        break

            if enabled_term_engine is None:
                for term_engine_name in _term_translation_engine_flag_names:
                    if config.get(term_engine_name, False):
                        enabled_term_engine = term_engine_name
                        break

            # If both engine types have been selected we can stop scanning for flags.
            if enabled_engine is not None and enabled_term_engine is not None:
                break

        # Process from lowest to highest priority
        for config in reversed(config_dicts):
            # Deep merge config into result
            self._deep_merge(result, config)

        if enabled_engine:
            for engine_name in _translation_engine_flag_names:
                if engine_name in result:
                    result[engine_name] = False
            result[enabled_engine] = True

        if enabled_term_engine:
            for engine_name in _term_translation_engine_flag_names:
                if engine_name in result:
                    result[engine_name] = False
            result[enabled_term_engine] = True

        return result

    def initialize_config(self) -> SettingsModel:
        """Initialize configuration from all sources"""
        # Parse CLI arguments (highest priority)
        install_redacting_filters()
        self._settings = self.initialize_cli_config().to_settings_model()
        register_secret_values(self._settings)
        log.debug("Initialized settings: %s", redact_for_log(self._settings))

        return self._settings

    def test_config(self, args):
        try:
            self._build_model_from_args(CLIEnvSettingsModel, args)
            return True
        except Exception as e:
            log.warning("Configuration validation failed: %s", type(e).__name__)
            return False

    def initialize_cli_config(self) -> CLIEnvSettingsModel:
        parser, _ = build_args_parser()
        args = parser.parse_args()
        cli_args: dict[str, Any] = {
            k.replace("-", "_"): v
            for k, v in vars(args).items()
            if v is not MagicDefault
        }
        cli_parsed_args = self.parse_dict_vars(
            dict_vars=cli_args,
        )
        # Parse environment variables (middle priority)
        env_vars = self.parse_env_vars()
        # Read default configuration file (lower priority)
        self._ensure_config_dir()
        self._migrate_legacy_user_config()
        default_config_file = self._read_toml_file(self._default_config_file_path)

        if not self.test_config(default_config_file):
            default_config_file = {}
            log.error(
                f"Error in test_config: {self._default_config_file_path}, skip it"
            )

        # Merge all settings by priority
        merged_args = self.merge_settings(
            [
                cli_parsed_args,
                env_vars,
            ]
        )
        config_from_file_dicts = []
        if "config_file" in merged_args:
            user_config = self._read_toml_file(Path(merged_args["config_file"]))
            if not self.test_config(user_config):
                log.error(
                    f"Error in test_config: {merged_args['config_file']}, skip it"
                )
                user_config = {}

            config_from_file_dicts.append(copy.deepcopy(user_config))
            del merged_args["config_file"]
            merged_args = self.merge_settings(
                [merged_args, user_config, default_config_file]
            )
        else:
            merged_args = self.merge_settings([merged_args, default_config_file])
        # Create settings model from merged dictionary
        self._update_version_default_config()
        config_from_file_dicts.append(copy.deepcopy(default_config_file))
        config_from_file_args = self.merge_settings(config_from_file_dicts)

        self.config_cli_settings = self._build_model_from_args(
            CLIEnvSettingsModel, config_from_file_args
        )

        cli_settings = self._build_model_from_args(CLIEnvSettingsModel, merged_args)
        cli_settings.validate_settings()
        return cli_settings

    def write_user_default_config_file(self, settings: CLIEnvSettingsModel):
        self._ensure_config_dir()
        # clear input file
        settings.basic.input_files = set()

        content = settings.model_dump(mode="json")
        if self._is_file_content_identical(DEFAULT_CONFIG_FILE, content):
            log.info(
                f"Config file {DEFAULT_CONFIG_FILE} is identical to the settings, skip it"
            )
            return

        self._write_toml_file(DEFAULT_CONFIG_FILE, content)
        log.info(f"Written user default config file to {DEFAULT_CONFIG_FILE}")

    def _build_model_from_args(
        self, model_class: type[BaseModel], args_dict: dict
    ) -> BaseModel:
        """
        Recursively build model instances from arguments dictionary

        Args:
            model_class: Pydantic model class to instantiate
            args_dict: Dictionary of arguments

        Returns:
            Instance of the specified model class
        """

        # Create and return the model instance
        return model_class(**args_dict)

    @property
    def settings(self) -> SettingsModel:
        """Get current settings"""
        if self._settings is None:
            raise RuntimeError("Settings not initialized")
        return self._settings


# disable not necessary log for normal usage
log.setLevel(logging.INFO)

if __name__ == "__main__":
    # only for debug
    log.setLevel(logging.DEBUG)
    logging.basicConfig(level=logging.DEBUG)

    config_manager = ConfigManager()
    # Use the new initialization method that combines CLI and env vars
    config_manager.initialize_config()
